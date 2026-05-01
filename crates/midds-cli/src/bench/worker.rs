//! Shared concurrent-worker scaffolding for the `bench` subcommands.
//!
//! `seed`, `bench fees`, and `bench throughput` all spread deposits across
//! a pool of derived signers and feed events into a central consumer that
//! drives a [`ProgressPrinter`]. The plumbing is identical from one to the
//! next; only the per-payload submission and event payloads differ. This
//! module owns the plumbing so the per-command files only carry the
//! interesting work.
//!
//! The shared [`setup_runner`] additionally factorises the pre-loop phase:
//! confirmation prompt, signer derivation, payload partitioning, node
//! connection, pricing snapshot, and optional auto-fund. Without it, each
//! mode rebuilt the same six-step preamble and drifted in non-obvious ways
//! (e.g. throughput shipped without funding or nonce caching for months —
//! see the rationale on [`fetch_signer_nonce`]).

use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use midds_client::{
    Balance, ChainConfig, MiddsClient, PricingSnapshot,
    subxt::tx::Signer,
    subxt_signer::sr25519::Keypair,
};
use midds_types::MusicalWork;
use tokio::{
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};

use crate::{
    admin,
    bench::{progress::ProgressPrinter, util::partition_round_robin},
    interactive,
    signers::{build_signer, derive_signers},
};

/// Fetch the chain's view of `signer`'s nonce.
///
/// Workers track the nonce locally instead of letting subxt auto-resolve it
/// per submit. Subxt reads from the latest *finalised* block, which lags
/// 2-3 blocks behind best — back-to-back submits without a GRANDPA wait
/// would re-use the same counter and the chain would reject the duplicate
/// as `Transaction is outdated`. Re-fetching from the same point on
/// recovery is enough to absorb any drift.
pub async fn fetch_signer_nonce(client: &MiddsClient, signer: &Keypair) -> Result<u64> {
    let account_id = Signer::<ChainConfig>::account_id(signer);
    client
        .account_nonce(&account_id)
        .await
        .map_err(|e| anyhow!("fetch signer nonce: {e}"))
}

/// Inputs for [`setup_runner`]. Bundled so each per-mode caller passes a
/// labelled struct instead of a 10-positional argument list.
pub(crate) struct RunnerInputs<'a> {
    pub url: &'a str,
    pub base_signer: &'a str,
    pub signer_count: u32,
    /// With `signer_count == 1`, controls whether to sign with the base URI
    /// verbatim (`build_signer`) or still derive `<base>//1`
    /// (`derive_signers`). `bench fees` uses `true` to preserve the historical
    /// `//Alice` solo flow on a fresh chain that has no pre-funded
    /// `//Alice//1` derivative; `seed` and `throughput` always derive.
    pub solo_uses_base_uri: bool,
    pub payloads: Vec<MusicalWork>,
    pub auto_fund: bool,
    pub funder: &'a str,
    pub fund_margin: f64,
    pub fund_batch_size: u32,
    pub assume_yes: bool,
    /// Y/N message shown via [`interactive::confirm_or_abort`] unless
    /// `assume_yes` is set. Kept caller-supplied because each mode prints a
    /// slightly different summary line (auto-fund clause, target counts).
    pub confirm_prompt: String,
}

/// Resources produced by [`setup_runner`] and consumed by per-mode worker
/// loops.
pub(crate) struct RunnerHandles {
    pub client: MiddsClient,
    pub signers: Vec<Keypair>,
    pub partitions: Vec<Vec<MusicalWork>>,
    /// `Some` iff `auto_fund` was enabled — the per-signer funded amount.
    /// `seed` reports it in the JSON payload; other modes ignore it.
    pub fund_amount_per_signer: Option<Balance>,
    /// Pricing snapshot read at run start. Drives the auto-fund announce
    /// line and the `bench fees` report's "pricing(start)" row. Always
    /// populated so callers don't have to redo the RPC.
    pub pricing_at_start: PricingSnapshot,
}

/// Run the pre-loop phase shared by every `bench` subcommand: optional
/// confirmation prompt, signer derivation, payload partitioning, node
/// connection, pricing snapshot, and optional auto-fund. Returns the
/// resources the per-mode worker loops need.
///
/// Encapsulating this here keeps `seed`, `bench fees`, and `bench throughput`
/// end-to-end consistent — the modes diverge only at the per-deposit submit
/// step, never at setup.
pub(crate) async fn setup_runner(inputs: RunnerInputs<'_>) -> Result<RunnerHandles> {
    if !inputs.assume_yes {
        interactive::confirm_or_abort(&inputs.confirm_prompt, true)?;
    }

    let signers = if inputs.signer_count == 1 && inputs.solo_uses_base_uri {
        vec![build_signer(inputs.base_signer).context("build signer")?]
    } else {
        derive_signers(inputs.base_signer, inputs.signer_count).context("derive signers")?
    };
    let partitions = partition_round_robin(inputs.payloads, inputs.signer_count as usize);

    let client = MiddsClient::connect(inputs.url).await?;
    let pricing_at_start = client
        .musical_works()
        .pricing_snapshot()
        .await
        .context("read pricing snapshot at run start")?;

    let fund_amount_per_signer = if inputs.auto_fund {
        let amount = admin::announce_and_fund(admin::AnnounceAndFundArgs {
            client: &client,
            partitions: &partitions,
            signers: &signers,
            funder_uri: inputs.funder,
            fund_margin: inputs.fund_margin,
            fund_batch_size: inputs.fund_batch_size,
            pricing: pricing_at_start,
        })
        .await?;
        Some(amount)
    } else {
        None
    };

    Ok(RunnerHandles {
        client,
        signers,
        partitions,
        fund_amount_per_signer,
        pricing_at_start,
    })
}

/// Run a single consumer task that drains `rx`, accumulates state, and
/// drives [`ProgressPrinter`] with a 500ms heartbeat so wall-time keeps
/// advancing even when no events have arrived.
///
/// `handler` mutates `state` for each incoming event and may render
/// out-of-band lines via the borrowed printer (call `progress.clear()`
/// first so the next print lands on a fresh row).
///
/// `progress_of` projects the current `state` into the
/// `(processed, ok, failed)` counters that the printer renders.
///
/// The receiver naturally closes once every worker has dropped its sender
/// clone, so callers must `drop(tx)` after the spawn loop. The function
/// returns the terminal `state` so the caller can inspect samples,
/// failure counts, abort signals, etc.
pub async fn run_progress_consumer<E, S>(
    mut rx: mpsc::UnboundedReceiver<E>,
    total: u32,
    started: Instant,
    mut state: S,
    mut handler: impl FnMut(E, &mut S, &mut ProgressPrinter),
    progress_of: impl Fn(&S) -> (u32, u32, u32),
) -> S {
    let mut progress = ProgressPrinter::new(total);
    // Render once immediately so the user sees `0/N elapsed=0s` instead of
    // a blank terminal during the ~6s it takes the first block to finalise.
    progress.tick(0, 0, 0, started);
    let mut heartbeat = interval(Duration::from_millis(500));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // Tokio intervals fire immediately on first poll; consume that so the
    // first scheduled tick happens 500 ms from now, not at t=0 (we already
    // covered t=0 with the manual render above).
    heartbeat.tick().await;
    loop {
        tokio::select! {
            // `biased` so events drain ahead of timer ticks under load —
            // we'd rather be slightly late on a refresh than starve the
            // channel and let it grow unbounded.
            biased;
            maybe_event = rx.recv() => {
                let Some(event) = maybe_event else { break };
                handler(event, &mut state, &mut progress);
            }
            _ = heartbeat.tick() => {}
        }
        let (processed, ok, failed) = progress_of(&state);
        progress.tick(processed, ok, failed, started);
    }
    progress.finish();
    state
}
