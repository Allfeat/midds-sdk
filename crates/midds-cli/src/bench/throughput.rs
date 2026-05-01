//! `bench throughput` — measure aggregate deposit submission rate.
//!
//! Submits up to `count` `MusicalWork::deposit` extrinsics across
//! `signer-count` signers (deterministic derivation under `--base-signer`,
//! same scheme as `seed`), with at most `concurrency` signers active at
//! once. Each signer's slice is chunked into `--batch-size` deposits and
//! submitted via `pallet_utility::batch_all` — atomic per batch — so the
//! per-signer wait shrinks from `count` finalisations to
//! `ceil(count / batch_size)` (mirrors `seed`).
//!
//! Per-signer nonces are tracked locally (same scheme as `seed` /
//! `bench fees`): subxt's auto-resolve reads the latest *finalised* block,
//! which lags 2-3 blocks behind best, so back-to-back submits without
//! GRANDPA finality between them would re-use the same counter and the
//! chain would reject the duplicate as `Transaction is outdated`. A
//! sentinel (`u64::MAX`) re-engages subxt's auto-nonce path when the
//! initial RPC fails, keeping the worker alive even on a flaky node.
//!
//! `--duration-secs` is a hard wall-clock cap; whichever of
//! `(count, duration)` triggers first stops the run cleanly. Reports TPS
//! based on finalised records and per-batch finalisation-latency
//! percentiles. Use `bench fees` for per-record fee breakdowns; this
//! command intentionally drops per-record receipts in exchange for the
//! batched submission rate.
//!
//! On batch failure the responsible worker exits (matching `seed`) — a
//! failed batch typically means broken nonce/funding state, and continuing
//! the same worker would just compound the error. Surviving workers keep
//! draining their partitions; the run's terminal `0/N succeeded` line is
//! enough signal when every worker fails their first batch.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use midds_fixtures::gen_n;
use serde::Serialize;
use tokio::{sync::Semaphore, task::JoinSet};

use crate::{
    bench::{
        util::{mean, percentile, sanitize_signer_concurrency, write_json_report},
        worker::{
            RunnerHandles, RunnerInputs, fetch_signer_nonce, run_progress_consumer, setup_runner,
        },
    },
    interactive,
};

const DEFAULT_RNG_SEED: u64 = 0x7470_5470_5470_5470;

#[derive(Serialize)]
struct LatencyStatsMs {
    min: u128,
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
    mean: u128,
}

#[derive(Serialize)]
struct ThroughputReport {
    scenario: &'static str,
    node_url: String,
    rng_seed_hex: String,
    base_signer: String,
    signer_count: u32,
    concurrency: u32,
    batch_size: u32,
    count_target: u32,
    count_submitted: u32,
    count_finalized: u32,
    count_failed: u32,
    batch_count_finalized: u32,
    duration_target_secs: Option<u64>,
    wall_time_ms: u128,
    tps_finalized: f64,
    /// Per-batch finalisation latency. With batched submission, "per-tx
    /// latency" is no longer a meaningful number — divide by `batch_size` if
    /// you want an amortised per-record figure.
    latency_batch_finalization_ms: LatencyStatsMs,
    auto_fund: bool,
    fund_amount_per_signer: Option<u128>,
}

/// Inputs for [`run`]. Bundles the CLI args so call sites don't drag a long
/// positional list around (mirrors `seed::Args` and `fees::Args`).
pub struct Args<'a> {
    pub url: &'a str,
    pub count: Option<u32>,
    pub duration_secs: Option<u64>,
    pub base_signer: &'a str,
    pub signer_count: u32,
    pub concurrency: u32,
    pub batch_size: u32,
    pub auto_fund: bool,
    pub funder: &'a str,
    pub fund_margin: f64,
    pub fund_batch_size: u32,
    pub rng_seed: Option<u64>,
    pub out: Option<&'a Path>,
    /// Skip the pre-flight confirmation prompt (CI / unattended runs).
    pub assume_yes: bool,
}

pub async fn run(args: Args<'_>) -> Result<()> {
    let Args {
        url,
        count,
        duration_secs,
        base_signer,
        signer_count,
        concurrency,
        batch_size,
        auto_fund,
        funder,
        fund_margin,
        fund_batch_size,
        rng_seed,
        out,
        assume_yes,
    } = args;

    // Build the config from CLI flags (with `count = 0` as a placeholder when
    // missing) and only hand off to the wizard if `count` was omitted. Both
    // paths converge at the destructure below.
    let cfg = interactive::ThroughputConfig {
        count: count.unwrap_or(0),
        duration_secs,
        base_signer: base_signer.to_string(),
        signer_count,
        concurrency,
        batch_size,
        auto_fund,
        funder: funder.to_string(),
        fund_margin,
        fund_batch_size,
        rng_seed: rng_seed.unwrap_or(DEFAULT_RNG_SEED),
        out: out.map(PathBuf::from),
    };
    let cfg = if count.is_some() {
        cfg
    } else {
        interactive::throughput_wizard(cfg)?
    };

    let interactive::ThroughputConfig {
        count,
        duration_secs,
        base_signer,
        signer_count,
        concurrency,
        batch_size,
        auto_fund,
        funder,
        fund_margin,
        fund_batch_size,
        rng_seed,
        out,
    } = cfg;

    if count == 0 {
        println!("count = 0, nothing to do");
        return Ok(());
    }

    let (signer_count, concurrency) = sanitize_signer_concurrency(signer_count, concurrency);
    let batch_size = batch_size.max(1);
    let fund_margin = fund_margin.max(1.0);
    let fund_batch_size = fund_batch_size.max(1);

    println!(
        "bench throughput: count={count} signers={signer_count} concurrency={concurrency} \
         batch_size={batch_size} duration={} rng_seed=0x{rng_seed:016x}",
        match duration_secs {
            Some(s) => format!("{s}s"),
            None => "unbounded".to_string(),
        }
    );
    if !auto_fund {
        println!(
            "  note: derived signers `{base_signer}//1..{base_signer}//{signer_count}` must \
             hold balance — pre-fund via `seed --auto-fund` or pass `--auto-fund` here"
        );
    }

    let payloads = gen_n(rng_seed, count);

    let confirm_prompt = if auto_fund {
        format!(
            "About to submit up to {count} deposits over {signer_count} signer(s) and \
             auto-fund them from `{funder}` on `{url}`. Continue?"
        )
    } else {
        format!(
            "About to submit up to {count} deposits over {signer_count} signer(s) on `{url}`. \
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
        // Always derive — `signer_count == 1` means `<base>//1`, never the
        // base URI verbatim. Multi-signer is the same scheme used by `seed`.
        // Without auto-fund, even the solo run needs `<base>//1` pre-funded.
        solo_uses_base_uri: false,
        payloads,
        auto_fund,
        funder: &funder,
        fund_margin,
        fund_batch_size,
        assume_yes,
        confirm_prompt,
    })
    .await?;

    let semaphore = Arc::new(Semaphore::new(concurrency as usize));
    // `stop` is flipped by the wall-clock timer when `--duration-secs` is set.
    // Workers poll between batches and bail out, so the timer truncates a
    // long run cleanly without waiting for the in-flight batch to drain.
    let stop = Arc::new(AtomicBool::new(false));

    if let Some(secs) = duration_secs {
        let stop = stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            stop.store(true, Ordering::Relaxed);
        });
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WorkerEvent>();
    let mut set: JoinSet<()> = JoinSet::new();
    let started = Instant::now();
    for (signer_idx, (signer, partition)) in signers.into_iter().zip(partitions).enumerate() {
        if partition.is_empty() {
            continue;
        }
        let semaphore = semaphore.clone();
        let client = client.clone();
        let tx = tx.clone();
        let stop = stop.clone();
        let batch_size_usize = batch_size as usize;
        // Acquire the permit *inside* the spawn so workers can be enqueued
        // eagerly: outside-the-spawn would serialise on the semaphore when
        // `concurrency < signer_count`. Same shape as `seed` / `bench fees`.
        set.spawn(async move {
            let _permit = match semaphore.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let api = client.musical_works();
            // Fall back to subxt's auto-nonce path if the initial RPC fails:
            // the counter never wraps in practice (a signer would have to
            // send 2^64 extrinsics before this aliases) so `u64::MAX` is a
            // safe sentinel. Same trick as `seed`.
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
            let chunk_sizes: Vec<u32> = partition
                .chunks(batch_size_usize)
                .map(|c| c.len() as u32)
                .collect();
            let total_chunks = chunk_sizes.len();
            for (chunk_idx, chunk) in partition.chunks(batch_size_usize).enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let chunk_records = chunk.len() as u32;
                let t_submit = Instant::now();
                let result = if nonce == u64::MAX {
                    api.deposit_batch(&signer, chunk.to_vec()).await
                } else {
                    api.deposit_batch_with_nonce(&signer, chunk.to_vec(), nonce)
                        .await
                };
                let elapsed = t_submit.elapsed();
                match result {
                    Ok(()) => {
                        if nonce != u64::MAX {
                            nonce = nonce.saturating_add(1);
                        }
                        let _ = tx.send(WorkerEvent::BatchOk {
                            records: chunk_records,
                            latency: elapsed,
                        });
                    }
                    Err(e) => {
                        // Mirror `seed`: bail the whole worker on first
                        // failure. Continuing is hopeless — runtime errors
                        // (e.g. `BondHoldFailed` when a signer is broke)
                        // consume the nonce, so the chain advances under
                        // us. Re-fetching from finalised storage lags 2-3
                        // blocks behind best, so the next submit picks up a
                        // stale nonce and rejects as `Transaction is
                        // outdated`. Surviving signers keep draining their
                        // partitions.
                        let remaining_records: u32 = chunk_sizes[chunk_idx..].iter().sum();
                        let remaining_chunks = total_chunks - chunk_idx;
                        let _ = tx.send(WorkerEvent::Notice(format!(
                            "signer #{signer_idx} stopped at batch {}/{total_chunks}: {e} \
                             — skipping {remaining_chunks} remaining batch(es) \
                             ({remaining_records} records)",
                            chunk_idx + 1,
                        )));
                        let _ = tx.send(WorkerEvent::BatchFailed {
                            records: remaining_records,
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
        started,
        ThroughputConsumerState::default(),
        |event, st, progress| match event {
            WorkerEvent::BatchOk { records, latency } => {
                st.successes += records;
                st.batch_count += 1;
                st.latencies_ns.push(latency.as_nanos());
            }
            WorkerEvent::BatchFailed { records } => {
                st.failures += records;
            }
            WorkerEvent::Notice(msg) => {
                progress.clear();
                eprintln!("  {msg}");
            }
        },
        |st| (st.successes + st.failures, st.successes, st.failures),
    ));

    while let Some(joined) = set.join_next().await {
        joined.map_err(|e| anyhow!("worker panicked: {e}"))?;
    }
    let ThroughputConsumerState {
        successes: count_finalized,
        failures: count_failed,
        batch_count: batch_count_finalized,
        latencies_ns,
    } = consumer
        .await
        .map_err(|e| anyhow!("consumer panicked: {e}"))?;
    let wall_time = started.elapsed();

    let count_submitted = count_finalized + count_failed;
    let tps_finalized = if wall_time.as_secs_f64() > 0.0 {
        count_finalized as f64 / wall_time.as_secs_f64()
    } else {
        0.0
    };

    let mut latencies_ms: Vec<u128> = latencies_ns.iter().map(|ns| ns / 1_000_000).collect();
    latencies_ms.sort_unstable();

    let report = ThroughputReport {
        scenario: "throughput",
        node_url: url.to_string(),
        rng_seed_hex: format!("0x{rng_seed:016x}"),
        base_signer: base_signer.clone(),
        signer_count,
        concurrency,
        batch_size,
        count_target: count,
        count_submitted,
        count_finalized,
        count_failed,
        batch_count_finalized,
        duration_target_secs: duration_secs,
        wall_time_ms: wall_time.as_millis(),
        tps_finalized,
        latency_batch_finalization_ms: LatencyStatsMs {
            min: *latencies_ms.first().unwrap_or(&0),
            p50: percentile(&latencies_ms, 0.50),
            p95: percentile(&latencies_ms, 0.95),
            p99: percentile(&latencies_ms, 0.99),
            max: *latencies_ms.last().unwrap_or(&0),
            mean: mean(&latencies_ms, |&v| v),
        },
        auto_fund,
        fund_amount_per_signer,
    };

    println!(
        "done: finalized={count_finalized}/{count_submitted} \
         failed={count_failed} batches={batch_count_finalized} tps={tps_finalized:.2} \
         batch_p50={}ms batch_p95={}ms batch_p99={}ms wall={}ms",
        report.latency_batch_finalization_ms.p50,
        report.latency_batch_finalization_ms.p95,
        report.latency_batch_finalization_ms.p99,
        report.wall_time_ms,
    );

    match out.as_deref() {
        Some(path) => write_json_report(path, &report)?,
        None => {
            let json = serde_json::to_string_pretty(&report)
                .map_err(|e| anyhow!("serialize throughput report: {e}"))?;
            println!("\n{json}");
        }
    }

    Ok(())
}

/// Events emitted by per-signer workers to the central consumer.
enum WorkerEvent {
    /// A `batch_all` chunk landed successfully. `records` is the number of
    /// inner deposits in the batch; `latency` is the wall-clock from submit
    /// to subxt's await completion (one batch = one in-block wait).
    BatchOk { records: u32, latency: Duration },
    /// A `batch_all` chunk failed atomically. `records` is the number of
    /// records skipped — including the failed batch *and* every chunk the
    /// worker won't attempt because it bailed out (cf. seed's pattern).
    BatchFailed { records: u32 },
    /// Out-of-band log line surfaced above the progress line.
    Notice(String),
}

#[derive(Default)]
struct ThroughputConsumerState {
    successes: u32,
    failures: u32,
    /// Number of `batch_all` calls that landed successfully. Drives the
    /// "batches=" line in the summary and pairs with `latencies_ns.len()`
    /// (they should always agree).
    batch_count: u32,
    /// Per-batch finalisation latency in nanoseconds. Stored ns-precision so
    /// the consumer can append in O(1); convert to milliseconds once at
    /// report time.
    latencies_ns: Vec<u128>,
}
