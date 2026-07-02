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
//!
//! `--midds-type all` ([`run_all`]) splits the count round-robin across every
//! V1 MIDDS type and runs the three sub-runs back-to-back through [`run_inner`],
//! aggregating the sub-reports into a single [`SeedAllReport`].

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Result, anyhow};
use midds_client::MiddsClient;
use midds_fixtures::{
    MiddsFixtures, musical_work::MusicalWorkFixtures, recording::RecordingFixtures,
    release::ReleaseFixtures,
};
use serde::Serialize;
use tokio::{sync::Semaphore, task::JoinSet};

use crate::{
    bench::{
        util::{sanitize_signer_concurrency, split_into_batches, write_json_report},
        worker::{
            ApiOf, RunnerHandles, RunnerInputs, fetch_signer_nonce, join_workers_and_consumer,
            run_progress_consumer, setup_runner,
        },
    },
    interactive,
};

const DEFAULT_RNG_SEED: u64 = 0xDEAD_BEEF_CAFE_F00D;

/// JSON payload written to `--report`. `verify-state` consumes this file (or
/// the human re-types the values) to validate the resulting on-chain state.
///
/// On a `--midds-type all` run, one of these is emitted per type and bundled
/// into a [`SeedAllReport`]; `midds_kind` is set in that case so consumers
/// can distinguish sub-runs.
#[derive(Serialize)]
pub(crate) struct SeedReport {
    scenario: &'static str,
    /// MIDDS type covered by this sub-run, set only when emitted as part of a
    /// [`SeedAllReport`]. `None` on a standalone single-type run.
    #[serde(skip_serializing_if = "Option::is_none")]
    midds_kind: Option<&'static str>,
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

/// Aggregated JSON payload emitted by `seed --midds-type all`. Carries the
/// global totals plus the per-type sub-reports so a downstream verifier can
/// replay each instance independently.
#[derive(Serialize)]
struct SeedAllReport {
    scenario: &'static str,
    node_url: String,
    rng_seed_hex: String,
    count_requested_total: u32,
    count_succeeded_total: u32,
    count_failed_total: u32,
    duration_ms: u128,
    sub_reports: Vec<SeedReport>,
}

/// Inputs for [`run`] and [`run_all`]. Grouped because the CLI wiring already
/// feeds 11+ fields and a positional list is a footgun-by-construction.
/// `Clone` lets [`run_all`] derive one [`Args`] per sub-run from the shared
/// resolved config.
#[derive(Clone)]
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

/// Public single-type entrypoint. Resolves the config (CLI args or wizard),
/// runs [`run_inner`], writes the JSON report when requested, and surfaces a
/// failure-summary error if any deposit failed.
///
/// Generic over the MIDDS payload via `F: MiddsFixtures`; `api_of` selects
/// the `pallet-midds` instance façade (`musical_works` / `recordings`),
/// dispatched per `--midds-type` in `main.rs`.
pub async fn run<F>(args: Args<'_>, api_of: ApiOf<F::Item>) -> Result<()>
where
    F: MiddsFixtures,
    F::Item: Send + Sync + 'static,
{
    let cfg = resolve_seed_config(&args)?;
    // Resolved here (not from `args`) so a report path typed in the wizard
    // is honoured exactly like `--report`.
    let report_path = cfg.report_path.clone();
    let report = run_inner::<F>(cfg, args.url, args.assume_yes, api_of, None).await?;
    if let Some(path) = report_path.as_deref() {
        write_json_report(path, &report)?;
    }
    if report.count_failed > 0 {
        return Err(failure_error(&report));
    }
    Ok(())
}

/// `seed --midds-type all` entrypoint. Splits the requested count round-robin
/// across the three V1 MIDDS types ([`split_round_robin`]) and runs each
/// sub-run back-to-back through [`run_inner`], with a single global
/// confirmation prompt and a single aggregated [`SeedAllReport`].
///
/// Sub-runs receive `assume_yes = true` and `report_path = None` so they don't
/// re-prompt the user or scribble per-type files alongside the aggregate.
pub async fn run_all(args: Args<'_>) -> Result<()> {
    let cfg = resolve_seed_config(&args)?;
    let (signer_count, concurrency) =
        sanitize_signer_concurrency(cfg.signer_count, cfg.concurrency);
    let batch_size = cfg.batch_size.max(1);
    let fund_margin = cfg.fund_margin.max(1.0);
    let fund_batch_size = cfg.fund_batch_size.max(1);

    if cfg.count == 0 {
        println!("count = 0, nothing to do");
        return Ok(());
    }

    let [c_mw, c_rec, c_rel] = split_round_robin(cfg.count);

    println!(
        "seed (all): count={count} rng_seed=0x{rng_seed:016x} \
         split = {c_mw} MusicalWork / {c_rec} Recording / {c_rel} Release \
         signers={signer_count} concurrency={concurrency} batch_size={batch_size}",
        count = cfg.count,
        rng_seed = cfg.rng_seed,
    );

    if !args.assume_yes {
        let funding_clause = if cfg.auto_fund {
            format!(" auto-funded from `{}`", cfg.funder)
        } else {
            String::new()
        };
        let prompt = format!(
            "About to deposit {count} records ({c_mw} MusicalWork + {c_rec} Recording + \
             {c_rel} Release) over {signer_count} signer(s) on `{url}`{funding_clause}. \
             Continue?",
            count = cfg.count,
            url = args.url,
        );
        interactive::confirm_or_abort(&prompt, true)?;
    }

    // Sub-runs get `assume_yes = true` (one global prompt above) and no
    // per-type report path (the aggregate below carries the sub-reports).
    let sub_cfg = |kind_count: u32| interactive::SeedConfig {
        count: kind_count,
        rng_seed: cfg.rng_seed,
        base_signer: cfg.base_signer.clone(),
        signer_count,
        concurrency,
        batch_size,
        auto_fund: cfg.auto_fund,
        funder: cfg.funder.clone(),
        fund_margin,
        fund_batch_size,
        report_path: None,
    };

    let started = Instant::now();
    let mut sub_reports: Vec<SeedReport> = Vec::with_capacity(3);

    if c_mw > 0 {
        let r = run_inner::<MusicalWorkFixtures>(
            sub_cfg(c_mw),
            args.url,
            true,
            MiddsClient::musical_works,
            Some("musical-work"),
        )
        .await?;
        sub_reports.push(r);
    }
    if c_rec > 0 {
        let r = run_inner::<RecordingFixtures>(
            sub_cfg(c_rec),
            args.url,
            true,
            MiddsClient::recordings,
            Some("recording"),
        )
        .await?;
        sub_reports.push(r);
    }
    if c_rel > 0 {
        let r = run_inner::<ReleaseFixtures>(
            sub_cfg(c_rel),
            args.url,
            true,
            MiddsClient::releases,
            Some("release"),
        )
        .await?;
        sub_reports.push(r);
    }

    let duration_ms = started.elapsed().as_millis();
    let count_succeeded_total: u32 = sub_reports.iter().map(|r| r.count_succeeded).sum();
    let count_failed_total: u32 = sub_reports.iter().map(|r| r.count_failed).sum();

    println!(
        "done (all): {ok}/{total} succeeded, {failed} failed in {duration_ms}ms",
        ok = count_succeeded_total,
        total = cfg.count,
        failed = count_failed_total,
    );

    if let Some(path) = cfg.report_path.as_deref() {
        let report = SeedAllReport {
            scenario: "seed_all",
            node_url: args.url.to_string(),
            rng_seed_hex: format!("0x{:016x}", cfg.rng_seed),
            count_requested_total: cfg.count,
            count_succeeded_total,
            count_failed_total,
            duration_ms,
            sub_reports,
        };
        write_json_report(path, &report)?;
    }

    if count_failed_total > 0 {
        return Err(anyhow!(
            "seed (all) completed with failures: \
             {count_succeeded_total}/{total} succeeded, {count_failed_total} failed. \
             Common causes: identifier collision with a previous run (try `--rng-seed`), \
             exhausted funder balance, or a runtime rejecting payloads the local \
             validator accepts.",
            total = cfg.count,
        ));
    }

    Ok(())
}

/// Resolve the seed config either by replaying CLI args verbatim or by walking
/// the wizard once when `count` is missing. Shared between [`run`]'s inner
/// path and [`run_all`] so the wizard never fires more than once per command.
fn resolve_seed_config(args: &Args<'_>) -> Result<interactive::SeedConfig> {
    let defaults = interactive::SeedConfig {
        count: args.count.unwrap_or(0),
        rng_seed: args.rng_seed.unwrap_or(DEFAULT_RNG_SEED),
        base_signer: args.base_signer.to_string(),
        signer_count: args.signer_count,
        concurrency: args.concurrency,
        batch_size: args.batch_size,
        auto_fund: args.auto_fund,
        funder: args.funder.to_string(),
        fund_margin: args.fund_margin,
        fund_batch_size: args.fund_batch_size,
        report_path: args.report_path.map(PathBuf::from),
    };
    if args.count.is_some() {
        Ok(defaults)
    } else {
        interactive::seed_wizard(defaults)
    }
}

/// Round-robin split of `count` across `[MusicalWork, Recording, Release]`.
/// The remainder cascades to the earlier types so `count = 1000` yields
/// `[334, 333, 333]` (sums back to 1000) and `count = 2` yields `[1, 1, 0]`.
fn split_round_robin(count: u32) -> [u32; 3] {
    let base = count / 3;
    let rem = count % 3;
    [base + u32::from(rem >= 1), base + u32::from(rem >= 2), base]
}

/// Format the user-facing error returned by [`run`] when at least one record
/// failed. Held in a helper so the message stays in lock-step with the
/// equivalent inline error in [`run_all`].
fn failure_error(report: &SeedReport) -> anyhow::Error {
    anyhow!(
        "seed completed with failures: {ok}/{count} succeeded, {failed} failed \
         (next_midds_id = {next_midds_id}). Common causes: identifier collision \
         with a previous run (try `--rng-seed`), exhausted funder balance, or \
         a runtime rejecting payloads the local validator accepts.",
        ok = report.count_succeeded,
        count = report.count_requested,
        failed = report.count_failed,
        next_midds_id = report.next_midds_id,
    )
}

/// Generic over the MIDDS payload via `F: MiddsFixtures`; `api_of` selects
/// the `pallet-midds` instance façade (`musical_works` / `recordings` / …),
/// dispatched per `--midds-type` in `main.rs`.
///
/// Takes an already-resolved [`interactive::SeedConfig`] — the CLI-vs-wizard
/// resolution happens once, in [`run`] / [`run_all`] — and returns the
/// assembled [`SeedReport`] instead of writing it directly so [`run_all`]
/// can aggregate sub-reports into a [`SeedAllReport`].
async fn run_inner<F>(
    cfg: interactive::SeedConfig,
    url: &str,
    assume_yes: bool,
    api_of: ApiOf<F::Item>,
    midds_kind: Option<&'static str>,
) -> Result<SeedReport>
where
    F: MiddsFixtures,
    F::Item: Send + Sync + 'static,
{
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
        report_path: _,
    } = cfg;
    let (signer_count, concurrency) = sanitize_signer_concurrency(signer_count, concurrency);
    let batch_size = batch_size.max(1);
    let fund_margin = fund_margin.max(1.0);
    let fund_batch_size = fund_batch_size.max(1);

    let kind_tag = midds_kind.map(|k| format!(" ({k})")).unwrap_or_default();
    println!(
        "seed{kind_tag}: count={count} rng_seed=0x{rng_seed:016x} signers={signer_count} \
         concurrency={concurrency} batch_size={batch_size}"
    );

    let payloads = F::gen_n(rng_seed, count);

    let confirm_prompt = if auto_fund {
        format!(
            "About to deposit {count}{kind_tag} records over {signer_count} signer(s) and \
             auto-fund them from `{funder}` on `{url}`. Continue?"
        )
    } else {
        format!(
            "About to deposit {count}{kind_tag} records over {signer_count} signer(s) on `{url}`. \
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
            let Ok(_permit) = semaphore.acquire_owned().await else {
                return;
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
            let batches = split_into_batches(payloads, batch_size);
            let chunk_sizes: Vec<u32> = batches.iter().map(|c| c.len() as u32).collect();
            let total_chunks = batches.len();
            for (chunk_idx, chunk) in batches.into_iter().enumerate() {
                let chunk_size = chunk.len() as u32;
                let result = if nonce == u64::MAX {
                    api.deposit_batch(&signer, chunk).await
                } else {
                    api.deposit_batch_with_nonce(&signer, chunk, nonce).await
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
                total.ok += u64::from(ok);
                total.failed += u64::from(failed);
            }
            WorkerEvent::Notice(msg) => progress.log(&msg),
        },
        |total| {
            let processed = (total.ok + total.failed) as u32;
            (processed, total.ok as u32, total.failed as u32)
        },
    ));

    let total = join_workers_and_consumer(set, consumer).await?;
    let duration_ms = started.elapsed().as_millis();

    let next_midds_id = api_of(&client).next_midds_id().await?;

    println!(
        "done{kind_tag}: {ok}/{count} succeeded, {failed} failed in {duration_ms}ms \
         (next_midds_id = {next_midds_id})",
        ok = total.ok,
        failed = total.failed,
    );

    Ok(SeedReport {
        scenario: "seed",
        midds_kind,
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
        base_signer,
        next_midds_id,
    })
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

#[cfg(test)]
mod tests {
    use super::split_round_robin;

    #[test]
    fn split_round_robin_sums_back_to_count() {
        for c in [0u32, 1, 2, 3, 4, 5, 10, 99, 100, 1000, u32::MAX] {
            let parts = split_round_robin(c);
            assert_eq!(
                u64::from(parts[0]) + u64::from(parts[1]) + u64::from(parts[2]),
                u64::from(c),
                "split must preserve total for count={c}",
            );
        }
    }

    #[test]
    fn split_round_robin_cascades_remainder_to_earlier_types() {
        assert_eq!(split_round_robin(1), [1, 0, 0]);
        assert_eq!(split_round_robin(2), [1, 1, 0]);
        assert_eq!(split_round_robin(3), [1, 1, 1]);
        assert_eq!(split_round_robin(1000), [334, 333, 333]);
        assert_eq!(split_round_robin(1001), [334, 334, 333]);
    }
}
