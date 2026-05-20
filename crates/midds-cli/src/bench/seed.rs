//! Deterministic mass-seed of a development node — Couche 5 / étape 5 of
//! `docs/testing.md`.
//!
//! Submits `count` `MusicalWork` records produced by `midds-fixtures::gen_n`
//! against the node at `--url`. Records are partitioned round-robin across
//! `signer-count` signers derived under `--base-signer`, with at most
//! `concurrency` signers active at once.
//!
//! Each signer's slice is then chunked into `--batch-size` deposits and
//! submitted via `pallet_utility::batch_all` — atomic per batch, so a
//! signer covering 1 000 deposits with `batch-size=100` only waits for
//! 10 finalisations instead of 1 000. Single-account nonces stay
//! sequential across batches; only inter-signer concurrency is enabled
//! by `--concurrency`.
//!
//! The pair `(rng-seed, count)` is the canonical reproducibility key: same
//! seed + same count means the same payload bytes, byte-for-byte. The
//! resulting on-chain state is therefore replayable across machines.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Result, anyhow};
use midds_fixtures::MiddsFixtures;
use serde::Serialize;
use tokio::{sync::Semaphore, task::JoinSet};

use crate::{
    bench::{
        util::{sanitize_signer_concurrency, write_json_report},
        worker::{
            ApiOf, RunnerHandles, RunnerInputs, fetch_signer_nonce, run_progress_consumer,
            setup_runner,
        },
    },
    interactive,
};

const DEFAULT_RNG_SEED: u64 = 0xDEAD_BEEF_CAFE_F00D;

/// JSON payload written to `--report`. `verify-state` consumes this file (or
/// the human re-types the values) to validate the resulting on-chain state.
#[derive(Serialize)]
struct SeedReport {
    scenario: &'static str,
    node_url: String,
    rng_seed_hex: String,
    count_requested: u32,
    count_succeeded: u32,
    count_failed: u32,
    duration_ms: u128,
    signer_count: u32,
    concurrency: u32,
    batch_size: u32,
    auto_fund: bool,
    fund_amount_per_signer: Option<u128>,
    base_signer: String,
    next_midds_id: u64,
}

/// Inputs for [`run`]. Grouped because the CLI wiring already feeds 11+
/// fields and a positional list is a footgun-by-construction.
pub struct Args<'a> {
    pub url: &'a str,
    /// `None` triggers an interactive prompt for the count.
    pub count: Option<u32>,
    pub rng_seed: Option<u64>,
    pub base_signer: &'a str,
    pub signer_count: u32,
    pub concurrency: u32,
    pub batch_size: u32,
    pub auto_fund: bool,
    pub funder: &'a str,
    pub fund_margin: f64,
    pub fund_batch_size: u32,
    pub report_path: Option<&'a Path>,
    /// Skip the pre-flight confirmation prompt (CI / unattended runs).
    pub assume_yes: bool,
}

/// Generic over the MIDDS payload via `F: MiddsFixtures`; `api_of` selects
/// the `pallet-midds` instance façade (`musical_works` / `recordings`),
/// dispatched per `--midds-type` in `main.rs`.
pub async fn run<F>(args: Args<'_>, api_of: ApiOf<F::Item>) -> Result<()>
where
    F: MiddsFixtures,
    F::Item: Send + Sync + 'static,
{
    let Args {
        url,
        count,
        rng_seed,
        base_signer,
        signer_count,
        concurrency,
        batch_size,
        auto_fund,
        funder,
        fund_margin,
        fund_batch_size,
        report_path,
        assume_yes,
    } = args;

    let cfg = interactive::SeedConfig {
        count: count.unwrap_or(0),
        rng_seed: rng_seed.unwrap_or(DEFAULT_RNG_SEED),
        base_signer: base_signer.to_string(),
        signer_count,
        concurrency,
        batch_size,
        auto_fund,
        funder: funder.to_string(),
        fund_margin,
        fund_batch_size,
        report_path: report_path.map(PathBuf::from),
    };
    let cfg = if count.is_some() {
        cfg
    } else {
        interactive::seed_wizard(cfg)?
    };

    let interactive::SeedConfig {
        count,
        rng_seed,
        base_signer,
        signer_count,
        concurrency,
        batch_size,
        auto_fund,
        funder,
        fund_margin,
        fund_batch_size,
        report_path,
    } = cfg;
    let (signer_count, concurrency) = sanitize_signer_concurrency(signer_count, concurrency);
    let batch_size = batch_size.max(1);
    let fund_margin = fund_margin.max(1.0);
    let fund_batch_size = fund_batch_size.max(1);

    if count == 0 {
        println!("count = 0, nothing to do");
        return Ok(());
    }

    println!(
        "seed: count={count} rng_seed=0x{rng_seed:016x} signers={signer_count} \
         concurrency={concurrency} batch_size={batch_size}"
    );

    let payloads = F::gen_n(rng_seed, count);

    let confirm_prompt = if auto_fund {
        format!(
            "About to deposit {count} records over {signer_count} signer(s) and \
             auto-fund them from `{funder}` on `{url}`. Continue?"
        )
    } else {
        format!(
            "About to deposit {count} records over {signer_count} signer(s) on `{url}`. \
             Continue?"
        )
    };

    let RunnerHandles {
        client,
        signers,
        partitions,
        fund_amount_per_signer,
        ..
    } = setup_runner(RunnerInputs {
        url,
        base_signer: &base_signer,
        signer_count,
        solo_uses_base_uri: false,
        payloads,
        api_of,
        auto_fund,
        funder: &funder,
        fund_margin,
        fund_batch_size,
        assume_yes,
        confirm_prompt,
    })
    .await?;

    let semaphore = Arc::new(Semaphore::new(concurrency as usize));
    let mut set: JoinSet<()> = JoinSet::new();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WorkerEvent>();

    let started = Instant::now();
    for (signer_idx, (signer, payloads)) in signers.into_iter().zip(partitions).enumerate() {
        if payloads.is_empty() {
            continue;
        }
        let semaphore = semaphore.clone();
        let client = client.clone();
        let batch_size = batch_size as usize;
        let tx = tx.clone();
        set.spawn(async move {
            let _permit = match semaphore.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let api = api_of(&client);
            let mut nonce = match fetch_signer_nonce(&client, &signer).await {
                Ok(n) => n,
                Err(e) => {
                    let _ = tx.send(WorkerEvent::Notice(format!(
                        "signer #{signer_idx}: {e} — falling back to subxt \
                         auto-nonce (may surface as `Transaction is outdated` \
                         after the first batch)"
                    )));
                    u64::MAX
                }
            };
            let chunk_sizes: Vec<u32> = payloads
                .chunks(batch_size)
                .map(|c| c.len() as u32)
                .collect();
            let total_chunks = chunk_sizes.len();
            for (chunk_idx, chunk) in payloads.chunks(batch_size).enumerate() {
                let chunk_size = chunk.len() as u32;
                let result = if nonce == u64::MAX {
                    api.deposit_batch(&signer, chunk.to_vec()).await
                } else {
                    api.deposit_batch_with_nonce(&signer, chunk.to_vec(), nonce)
                        .await
                };
                match result {
                    Ok(()) => {
                        if nonce != u64::MAX {
                            nonce = nonce.saturating_add(1);
                        }
                        let _ = tx.send(WorkerEvent::Chunk {
                            ok: chunk_size,
                            failed: 0,
                        });
                    }
                    Err(e) => {
                        let remaining_records: u32 = chunk_sizes[chunk_idx..].iter().sum();
                        let remaining_chunks = total_chunks - chunk_idx;
                        let _ = tx.send(WorkerEvent::Notice(format!(
                            "signer #{signer_idx} stopped at chunk {}/{total_chunks}: {e} \
                             — skipping {remaining_chunks} remaining chunk(s) \
                             ({remaining_records} records)",
                            chunk_idx + 1,
                        )));
                        let _ = tx.send(WorkerEvent::Chunk {
                            ok: 0,
                            failed: remaining_records,
                        });
                        return;
                    }
                }
            }
        });
    }
    drop(tx);

    let consumer = tokio::spawn(run_progress_consumer(
        rx,
        count,
        SignerOutcome::default(),
        |event, total, progress| match event {
            WorkerEvent::Chunk { ok, failed } => {
                total.ok += ok as u64;
                total.failed += failed as u64;
            }
            WorkerEvent::Notice(msg) => progress.log(&msg),
        },
        |total| {
            let processed = (total.ok + total.failed) as u32;
            (processed, total.ok as u32, total.failed as u32)
        },
    ));

    while let Some(joined) = set.join_next().await {
        joined.map_err(|e| anyhow!("worker panicked: {e}"))?;
    }
    let total = consumer
        .await
        .map_err(|e| anyhow!("consumer panicked: {e}"))?;
    let duration_ms = started.elapsed().as_millis();

    let next_midds_id = api_of(&client).next_midds_id().await?;

    println!(
        "done: {ok}/{count} succeeded, {failed} failed in {duration_ms}ms (next_midds_id = {next_midds_id})",
        ok = total.ok,
        failed = total.failed,
    );

    if let Some(path) = report_path.as_deref() {
        write_json_report(
            path,
            &SeedReport {
                scenario: "seed",
                node_url: url.to_string(),
                rng_seed_hex: format!("0x{rng_seed:016x}"),
                count_requested: count,
                count_succeeded: total.ok as u32,
                count_failed: total.failed as u32,
                duration_ms,
                signer_count,
                concurrency,
                batch_size,
                auto_fund,
                fund_amount_per_signer,
                base_signer: base_signer.clone(),
                next_midds_id,
            },
        )?;
    }

    if total.failed > 0 {
        return Err(anyhow!(
            "seed completed with failures: {ok}/{count} succeeded, {failed} failed \
             (next_midds_id = {next_midds_id}). Common causes: identifier collision \
             with a previous run (try `--rng-seed`), exhausted funder balance, or \
             a runtime rejecting payloads the local validator accepts.",
            ok = total.ok,
            failed = total.failed,
        ));
    }

    Ok(())
}

#[derive(Default)]
struct SignerOutcome {
    ok: u64,
    failed: u64,
}

/// Events from a per-signer worker to the consumer task that owns the
/// progress printer.
enum WorkerEvent {
    /// One `batch_all` chunk completed (or failed atomically). `ok` is the
    /// number of records that landed; `failed` is the number that didn't.
    /// At most one of the two is non-zero per event since batches are
    /// atomic.
    Chunk { ok: u32, failed: u32 },
    /// Out-of-band log line — printed above the progress line so it stays
    /// visible.
    Notice(String),
}
