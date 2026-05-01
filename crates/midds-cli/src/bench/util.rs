//! Helpers shared across the `bench` subcommands (`seed`, `fees`,
//! `throughput`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use midds_types::MusicalWork;
use serde::Serialize;

/// Round-robin a flat list of payloads into `partitions` buckets so each bucket
/// gets either `floor(n/p)` or `ceil(n/p)` items.
pub fn partition_round_robin(
    payloads: Vec<MusicalWork>,
    partitions: usize,
) -> Vec<Vec<MusicalWork>> {
    let mut batches: Vec<Vec<MusicalWork>> = (0..partitions).map(|_| Vec::new()).collect();
    for (i, p) in payloads.into_iter().enumerate() {
        batches[i % partitions].push(p);
    }
    batches
}

/// Clamp `signer_count` and `concurrency` to sensible values shared by every
/// bench command: at least one signer, at least one concurrent worker, and
/// concurrency never exceeds signer count (an idle permit on every excess
/// would just be dead weight).
pub fn sanitize_signer_concurrency(signer_count: u32, concurrency: u32) -> (u32, u32) {
    let signers = signer_count.max(1);
    let concurrency = concurrency.max(1).min(signers);
    (signers, concurrency)
}

/// Linear-interpolation-free percentile over a sorted ascending slice.
/// `q` ∈ `[0.0, 1.0]`. Returns 0 for empty input.
pub fn percentile(sorted: &[u128], q: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let q = q.clamp(0.0, 1.0);
    let idx = ((sorted.len() as f64 - 1.0) * q).round() as usize;
    sorted[idx]
}

/// Mean of the projected values; returns 0 on empty input.
pub fn mean<T>(items: &[T], f: impl Fn(&T) -> u128) -> u128 {
    if items.is_empty() {
        return 0;
    }
    let sum: u128 = items.iter().map(&f).sum();
    sum / items.len() as u128
}

/// Write `body` to `path`, creating parent directories on demand. Prints the
/// resolved path on success so operators can tail it from the same run.
pub fn write_report_to(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create report dir {}", parent.display()))?;
    }
    let path: PathBuf = path.to_path_buf();
    std::fs::write(&path, body).with_context(|| format!("write report {}", path.display()))?;
    println!("report → {}", path.display());
    Ok(())
}

/// Pretty-print `report` as JSON and persist it via [`write_report_to`].
/// Used by the `seed` and `bench throughput` reports, which both target a
/// JSON shape consumable by `verify-state` and humans alike.
pub fn write_json_report<R: Serialize>(path: &Path, report: &R) -> Result<()> {
    let json = serde_json::to_string_pretty(report).context("serialize report")?;
    write_report_to(path, &json)
}
