//! Clap definitions for the `midds` debug CLI.
//!
//! V1 ships only the operator debug tooling: `seed` (deterministic mass-seed
//! of a development node) and `bench` (per-deposit fees + aggregate
//! throughput). User-facing commands (deposit/update/query/...) are
//! intentionally absent — those will land once the SDK has a stable client
//! shape worth exposing.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

const DEFAULT_URL: &str = "ws://localhost:9944";
const DEFAULT_SEED_BASE_SIGNER: &str = "//Alice";
const DEFAULT_SIGNER_COUNT: u32 = 1;
const DEFAULT_CONCURRENCY: u32 = 1;
const DEFAULT_SEED_BATCH_SIZE: u32 = 100;
const DEFAULT_SEED_FUNDER: &str = "//Alice";
const DEFAULT_SEED_FUND_MARGIN: f64 = 1.3;
const DEFAULT_SEED_FUND_BATCH_SIZE: u32 = 100;
const DEFAULT_FEES_BASE_SIGNER: &str = "//Alice";
const DEFAULT_FEES_DISTRIBUTION: SizeDistribution = SizeDistribution::Real;
const DEFAULT_FEES_SIGNER_COUNT: u32 = 1;
const DEFAULT_FEES_CONCURRENCY: u32 = 1;
const DEFAULT_FEES_BATCH_SIZE: u32 = 100;
const DEFAULT_FEES_FUNDER: &str = "//Alice";
const DEFAULT_FEES_FUND_MARGIN: f64 = 1.3;
const DEFAULT_FEES_FUND_BATCH_SIZE: u32 = 100;
const DEFAULT_THROUGHPUT_BASE_SIGNER: &str = "//Alice";
const DEFAULT_THROUGHPUT_SIGNER_COUNT: u32 = 4;
const DEFAULT_THROUGHPUT_CONCURRENCY: u32 = 4;
const DEFAULT_THROUGHPUT_BATCH_SIZE: u32 = 100;
const DEFAULT_THROUGHPUT_FUNDER: &str = "//Alice";
const DEFAULT_THROUGHPUT_FUND_MARGIN: f64 = 1.3;
const DEFAULT_THROUGHPUT_FUND_BATCH_SIZE: u32 = 100;

/// MIDDS debug CLI: mass-seed a dev node and benchmark deposits.
#[derive(Parser, Debug)]
#[command(name = "midds", version, about)]
pub struct Cli {
    /// WebSocket URL of an Allfeat node.
    #[arg(short, long, default_value = DEFAULT_URL, global = true)]
    pub url: String,

    /// Subcommand. Omit to launch the top-level interactive picker.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Mass-seed the node with deterministically-generated MIDDS records.
    ///
    /// `(rng-seed, count)` is the reproducibility key — same pair produces
    /// the same on-chain state. Records are bundled into
    /// `pallet_utility::batch_all` calls of `--batch-size`, atomic per
    /// batch, so `count` deposits cost `ceil(count / batch_size)` blocks
    /// per signer instead of `count` blocks.
    ///
    /// With `--auto-fund`, the command also pre-funds the derived signers
    /// from `--funder` (default `//Alice`) before seeding, computing the
    /// exact bond requirement from the chain's `DepositBase` /
    /// `DepositPerByte` constants. One self-contained command end-to-end.
    Seed {
        /// MIDDS payload type to seed. V1 only accepts `musical-work`.
        #[arg(long = "midds-type", value_enum, default_value_t = MiddsKind::default())]
        midds_type: MiddsKind,
        /// Number of records to deposit. Omit to be prompted interactively.
        #[arg(long)]
        count: Option<u32>,
        /// 64-bit RNG seed (decimal or `0x`-prefixed hex). Default is fixed
        /// so unattended runs stay reproducible.
        #[arg(long = "rng-seed", value_parser = parse_u64_seed)]
        rng_seed: Option<u64>,
        /// Base signer URI; signers are derived as `<base>//1`, `<base>//2`, …
        #[arg(long = "base-signer", default_value = DEFAULT_SEED_BASE_SIGNER)]
        base_signer: String,
        /// Number of derived signers used to spread deposits.
        #[arg(long = "signer-count", default_value_t = DEFAULT_SIGNER_COUNT)]
        signer_count: u32,
        /// Maximum signers active at once. Capped at `signer-count`.
        #[arg(long, default_value_t = DEFAULT_CONCURRENCY)]
        concurrency: u32,
        /// Inner deposits per `batch_all` call. Lower if you hit a
        /// per-block weight limit on large payloads; higher to amortise
        /// the per-batch fee. `1` falls back to one extrinsic per record.
        #[arg(long = "batch-size", default_value_t = DEFAULT_SEED_BATCH_SIZE)]
        batch_size: u32,
        /// Pre-fund the derived signers before seeding. Funding amount is
        /// computed exactly from the generated payloads + chain constants.
        #[arg(long = "auto-fund")]
        auto_fund: bool,
        /// Funder URI used by `--auto-fund` (defaults to `//Alice`).
        #[arg(long = "funder", default_value = DEFAULT_SEED_FUNDER)]
        funder: String,
        /// Safety multiplier on the computed funding amount. `1.0` = exact
        /// bond cost, no headroom for tx fees / variance.
        #[arg(long = "fund-margin", default_value_t = DEFAULT_SEED_FUND_MARGIN)]
        fund_margin: f64,
        /// Inner transfers per funding `batch_all` call. Independent of
        /// `--batch-size`, which controls deposit batching.
        #[arg(long = "fund-batch-size", default_value_t = DEFAULT_SEED_FUND_BATCH_SIZE)]
        fund_batch_size: u32,
        /// Optional path for the JSON seed report.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Skip the safety confirmation prompt (for CI / unattended runs).
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },

    /// Live-node measurements: per-deposit fees and aggregate throughput.
    Bench {
        /// Sub-scenario. Omit to launch the bench picker.
        #[command(subcommand)]
        kind: Option<BenchArgs>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BenchArgs {
    /// Deposit `count` records and report bond + tx fee per record.
    ///
    /// Each deposit waits for finalisation and reports its own
    /// `TransactionPayment::TransactionFeePaid` event, so fees stay
    /// attributable per record regardless of how many signers are running.
    /// `--signer-count` > 1 spreads the load across derived signers, which
    /// reflects realistic chain conditions (concurrent blocks, fee multiplier
    /// drift) at the price of slightly higher fee variance. Defaults to a
    /// single signer to preserve the simple `//Alice` flow; with multi-signer,
    /// pair with `--auto-fund` (or pre-fund manually) so the derived accounts
    /// can pay tx fees + bond.
    Fees {
        /// MIDDS payload type to benchmark. V1 only accepts `musical-work`.
        #[arg(long = "midds-type", value_enum, default_value_t = MiddsKind::default())]
        midds_type: MiddsKind,
        /// Number of records to deposit and measure. Omit to be prompted.
        #[arg(long)]
        count: Option<u32>,
        /// Payload size mix.
        #[arg(long = "size-distribution", value_enum, default_value_t = DEFAULT_FEES_DISTRIBUTION)]
        size_distribution: SizeDistribution,
        /// Base signer URI; with `--signer-count` > 1, signers are derived
        /// as `<base>//1`, `<base>//2`, … (same scheme as `seed`).
        #[arg(long = "base-signer", default_value = DEFAULT_FEES_BASE_SIGNER)]
        base_signer: String,
        /// Number of derived signers used to spread deposits. `1` keeps the
        /// classic single-signer behaviour and submits with `--base-signer`
        /// directly.
        #[arg(long = "signer-count", default_value_t = DEFAULT_FEES_SIGNER_COUNT)]
        signer_count: u32,
        /// Maximum signers active at once. Capped at `signer-count`.
        #[arg(long, default_value_t = DEFAULT_FEES_CONCURRENCY)]
        concurrency: u32,
        /// Inner deposits per `batch_all` call. Lower for tighter per-record
        /// `tx_fee` attribution (the runtime emits one `TransactionFeePaid`
        /// per outer extrinsic, so the report amortises the fee across the
        /// batch); higher to amortise the per-batch cost across more records.
        /// `1` falls back to one `batch_all` per record.
        #[arg(long = "batch-size", default_value_t = DEFAULT_FEES_BATCH_SIZE)]
        batch_size: u32,
        /// Pre-fund the derived signers from `--funder` before measuring.
        /// Only meaningful with `--signer-count` > 1; the funding amount is
        /// computed from the chain's `DepositBase` / `DepositPerByte` plus a
        /// margin so each signer can cover its bond + tx fees.
        #[arg(long = "auto-fund")]
        auto_fund: bool,
        /// Funder URI used by `--auto-fund` (defaults to `//Alice`).
        #[arg(long = "funder", default_value = DEFAULT_FEES_FUNDER)]
        funder: String,
        /// Safety multiplier on the computed funding amount. `1.0` = exact
        /// bond cost, no headroom for tx fees / variance.
        #[arg(long = "fund-margin", default_value_t = DEFAULT_FEES_FUND_MARGIN)]
        fund_margin: f64,
        /// Inner transfers per funding `batch_all` call.
        #[arg(long = "fund-batch-size", default_value_t = DEFAULT_FEES_FUND_BATCH_SIZE)]
        fund_batch_size: u32,
        /// 64-bit RNG seed (decimal or `0x`-prefixed hex). Default fixed for
        /// reproducibility across runs.
        #[arg(long = "rng-seed", value_parser = parse_u64_seed)]
        rng_seed: Option<u64>,
        /// Path for the markdown fees report (omit to print to stdout).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Skip the safety confirmation prompt (for CI / unattended runs).
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },

    /// Maximise deposit submission rate against the node and report TPS,
    /// finalisation latency percentiles, and success/failure counts.
    ///
    /// Records are bundled into `pallet_utility::batch_all` calls of
    /// `--batch-size` deposits each, atomic per batch — same scheme as
    /// `seed`. Each signer drains its own chunks sequentially with a
    /// locally-tracked nonce; back-to-back submits without GRANDPA finality
    /// between them would otherwise race the chain's view and reject as
    /// `Transaction is outdated`. Multi-signer concurrency spreads work
    /// across derived `<base>//1..<base>//N` accounts, which on a fresh
    /// chain need to be pre-funded; pair with `--auto-fund` to do that in
    /// one shot.
    Throughput {
        /// MIDDS payload type to benchmark. V1 only accepts `musical-work`.
        #[arg(long = "midds-type", value_enum, default_value_t = MiddsKind::default())]
        midds_type: MiddsKind,
        /// Target number of records to deposit. Omit to be prompted.
        #[arg(long)]
        count: Option<u32>,
        /// Hard wall-clock cap (seconds). Stops sooner if the count target is hit.
        #[arg(long = "duration-secs")]
        duration_secs: Option<u64>,
        /// Base signer URI; signers are derived as `<base>//1`, `<base>//2`, …
        #[arg(long = "base-signer", default_value = DEFAULT_THROUGHPUT_BASE_SIGNER)]
        base_signer: String,
        /// Number of derived signers used to spread submissions.
        #[arg(long = "signer-count", default_value_t = DEFAULT_THROUGHPUT_SIGNER_COUNT)]
        signer_count: u32,
        /// Maximum signers active at once. Capped at `signer-count`.
        #[arg(long, default_value_t = DEFAULT_THROUGHPUT_CONCURRENCY)]
        concurrency: u32,
        /// Inner deposits per `batch_all` call. Lower if you hit a
        /// per-block weight limit on large payloads; higher to amortise
        /// the per-batch fee. `1` falls back to one extrinsic per record.
        #[arg(long = "batch-size", default_value_t = DEFAULT_THROUGHPUT_BATCH_SIZE)]
        batch_size: u32,
        /// Pre-fund the derived signers from `--funder` before measuring.
        /// Funding amount is computed from the chain's `DepositBase` /
        /// `DepositPerByte` plus a margin so each signer can cover its bond
        /// + tx fees end-to-end.
        #[arg(long = "auto-fund")]
        auto_fund: bool,
        /// Funder URI used by `--auto-fund` (defaults to `//Alice`).
        #[arg(long = "funder", default_value = DEFAULT_THROUGHPUT_FUNDER)]
        funder: String,
        /// Safety multiplier on the computed funding amount. `1.0` = exact
        /// bond cost, no headroom for tx fees / variance.
        #[arg(long = "fund-margin", default_value_t = DEFAULT_THROUGHPUT_FUND_MARGIN)]
        fund_margin: f64,
        /// Inner transfers per funding `batch_all` call.
        #[arg(long = "fund-batch-size", default_value_t = DEFAULT_THROUGHPUT_FUND_BATCH_SIZE)]
        fund_batch_size: u32,
        /// 64-bit RNG seed for the deterministic payload generator.
        #[arg(long = "rng-seed", value_parser = parse_u64_seed)]
        rng_seed: Option<u64>,
        /// Path for the JSON throughput report (omit to print to stdout).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Skip the safety confirmation prompt (for CI / unattended runs).
        #[arg(short = 'y', long = "yes")]
        yes: bool,
    },
}

/// MIDDS payload type the operator commands target. V1 ships only
/// `MusicalWork`; the enum is in place so adding `Recording` / `Release`
/// is a one-variant change at every dispatch site rather than a parallel
/// command tree.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
pub enum MiddsKind {
    /// `MusicalWork::V1` — the only payload type wired in V1.
    #[default]
    #[value(name = "musical-work")]
    MusicalWork,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum SizeDistribution {
    /// Realistic mix from `midds-fixtures::gen_n` (~50–250 byte payloads).
    #[value(name = "real")]
    Real,
    /// Every payload saturated to `MaxEncodedLen` (worst-case bond + length).
    #[value(name = "max")]
    Max,
    /// Round-robin between `min`, `real`, and `max` payload shapes.
    #[value(name = "mixed")]
    Mixed,
}

/// Parse a 64-bit seed in decimal or `0x`-prefixed hex form.
pub(crate) fn parse_u64_seed(raw: &str) -> Result<u64, String> {
    let trimmed = raw.trim();
    let parsed = if let Some(hex) = trimmed.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        trimmed.parse::<u64>()
    };
    parsed.map_err(|e| format!("invalid u64 seed `{raw}`: {e}"))
}
