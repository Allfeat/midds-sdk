//! Clap definitions for the `midds` debug CLI.
//!
//! Three commands: `create` (offline, interactive MIDDS builder that emits a
//! validated SCALE / JSON payload — no node), and the operator debug tooling
//! `seed` (deterministic mass-seed of a development node) and `bench`
//! (per-deposit fees + aggregate throughput). Run `midds` with no argument
//! for the interactive launcher. On-chain user commands (deposit/update/
//! query/...) stay the job of `midds-client`, not this CLI.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

const DEFAULT_URL: &str = "ws://localhost:9944";
/// Shared default for `--base-signer` and `--funder`. The pre-funded `//Alice`
/// dev account is always present on a freshly-booted Substrate dev chain.
const DEFAULT_SIGNER_URI: &str = "//Alice";
/// Records per inner `Utility::batch_all` call. 100 keeps inclusion latency
/// reasonable without driving the per-block weight to the cap on realistic
/// payloads. Applies to deposits *and* funding transfers (`--fund-batch-size`).
const DEFAULT_BATCH_SIZE: u32 = 100;
/// Safety multiplier on the computed auto-fund amount: 1.3 covers worst-case
/// multiplier drift over the run plus per-tx fees.
const DEFAULT_FUND_MARGIN: f64 = 1.3;
const DEFAULT_FEES_DISTRIBUTION: SizeDistribution = SizeDistribution::Real;
/// `throughput` defaults to 4 derived signers (and 4-wide concurrency) — what
/// roughly saturates a dev node's block pipeline. `seed` and `bench fees` stay
/// at a single signer so the simple flow remains snappy and reproducible.
const DEFAULT_THROUGHPUT_SIGNERS: u32 = 4;
const DEFAULT_THROUGHPUT_CONCURRENCY: u32 = 4;

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
    /// Build a MIDDS payload interactively and emit it as validated SCALE
    /// hex / JSON. Fully offline — never connects to a node.
    ///
    /// Every field is prompted with inline validation; the assembled payload
    /// is then run through the on-chain `validate_format` before it is
    /// emitted, so what you get is wire-ready. Omit `--type` to pick the
    /// MIDDS kind interactively.
    Create {
        /// MIDDS payload type to build. Omit to be prompted.
        #[arg(long = "type", value_enum)]
        midds_type: Option<MiddsKind>,
        /// Write the payload to this file instead of stdout (JSON, unless
        /// `--format hex`). The complementary form is still echoed to stderr.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Representation emitted to stdout when `--out` is not given.
        #[arg(long, value_enum, default_value_t = CreateFormat::Both)]
        format: CreateFormat,
    },

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
        #[arg(long = "base-signer", default_value = DEFAULT_SIGNER_URI)]
        base_signer: String,
        /// Number of derived signers used to spread deposits.
        #[arg(long = "signer-count", default_value_t = 1)]
        signer_count: u32,
        /// Maximum signers active at once. Capped at `signer-count`.
        #[arg(long, default_value_t = 1)]
        concurrency: u32,
        /// Inner deposits per `batch_all` call. Lower if you hit a
        /// per-block weight limit on large payloads; higher to amortise
        /// the per-batch fee. `1` falls back to one extrinsic per record.
        #[arg(long = "batch-size", default_value_t = DEFAULT_BATCH_SIZE)]
        batch_size: u32,
        /// Pre-fund the derived signers before seeding. Funding amount is
        /// computed exactly from the generated payloads + chain constants.
        #[arg(long = "auto-fund")]
        auto_fund: bool,
        /// Funder URI used by `--auto-fund` (defaults to `//Alice`).
        #[arg(long = "funder", default_value = DEFAULT_SIGNER_URI)]
        funder: String,
        /// Safety multiplier on the computed funding amount. `1.0` = exact
        /// bond cost, no headroom for tx fees / variance.
        #[arg(long = "fund-margin", default_value_t = DEFAULT_FUND_MARGIN)]
        fund_margin: f64,
        /// Inner transfers per funding `batch_all` call. Independent of
        /// `--batch-size`, which controls deposit batching.
        #[arg(long = "fund-batch-size", default_value_t = DEFAULT_BATCH_SIZE)]
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
        #[arg(long = "base-signer", default_value = DEFAULT_SIGNER_URI)]
        base_signer: String,
        /// Number of derived signers used to spread deposits. `1` keeps the
        /// classic single-signer behaviour and submits with `--base-signer`
        /// directly.
        #[arg(long = "signer-count", default_value_t = 1)]
        signer_count: u32,
        /// Maximum signers active at once. Capped at `signer-count`.
        #[arg(long, default_value_t = 1)]
        concurrency: u32,
        /// Inner deposits per `batch_all` call. Lower for tighter per-record
        /// `tx_fee` attribution (the runtime emits one `TransactionFeePaid`
        /// per outer extrinsic, so the report amortises the fee across the
        /// batch); higher to amortise the per-batch cost across more records.
        /// `1` falls back to one `batch_all` per record.
        #[arg(long = "batch-size", default_value_t = DEFAULT_BATCH_SIZE)]
        batch_size: u32,
        /// Pre-fund the derived signers from `--funder` before measuring.
        /// Only meaningful with `--signer-count` > 1; the funding amount is
        /// computed from the chain's `DepositBase` / `DepositPerByte` plus a
        /// margin so each signer can cover its bond + tx fees.
        #[arg(long = "auto-fund")]
        auto_fund: bool,
        /// Funder URI used by `--auto-fund` (defaults to `//Alice`).
        #[arg(long = "funder", default_value = DEFAULT_SIGNER_URI)]
        funder: String,
        /// Safety multiplier on the computed funding amount. `1.0` = exact
        /// bond cost, no headroom for tx fees / variance.
        #[arg(long = "fund-margin", default_value_t = DEFAULT_FUND_MARGIN)]
        fund_margin: f64,
        /// Inner transfers per funding `batch_all` call.
        #[arg(long = "fund-batch-size", default_value_t = DEFAULT_BATCH_SIZE)]
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
        #[arg(long = "base-signer", default_value = DEFAULT_SIGNER_URI)]
        base_signer: String,
        /// Number of derived signers used to spread submissions.
        #[arg(long = "signer-count", default_value_t = DEFAULT_THROUGHPUT_SIGNERS)]
        signer_count: u32,
        /// Maximum signers active at once. Capped at `signer-count`.
        #[arg(long, default_value_t = DEFAULT_THROUGHPUT_CONCURRENCY)]
        concurrency: u32,
        /// Inner deposits per `batch_all` call. Lower if you hit a
        /// per-block weight limit on large payloads; higher to amortise
        /// the per-batch fee. `1` falls back to one extrinsic per record.
        #[arg(long = "batch-size", default_value_t = DEFAULT_BATCH_SIZE)]
        batch_size: u32,
        /// Pre-fund the derived signers from `--funder` before measuring.
        /// Funding amount is computed from the chain's `DepositBase` /
        /// `DepositPerByte` plus a margin so each signer can cover its bond
        /// + tx fees end-to-end.
        #[arg(long = "auto-fund")]
        auto_fund: bool,
        /// Funder URI used by `--auto-fund` (defaults to `//Alice`).
        #[arg(long = "funder", default_value = DEFAULT_SIGNER_URI)]
        funder: String,
        /// Safety multiplier on the computed funding amount. `1.0` = exact
        /// bond cost, no headroom for tx fees / variance.
        #[arg(long = "fund-margin", default_value_t = DEFAULT_FUND_MARGIN)]
        fund_margin: f64,
        /// Inner transfers per funding `batch_all` call.
        #[arg(long = "fund-batch-size", default_value_t = DEFAULT_BATCH_SIZE)]
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

/// MIDDS payload type the operator commands target. Each variant maps to a
/// `MiddsFixtures` impl and a `midds-client` pallet-instance accessor at the
/// dispatch sites in `main.rs`; the whole bench harness is generic over the
/// MIDDS type so adding `Release` later is a one-variant change here plus the
/// two dispatch arms.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
pub enum MiddsKind {
    /// `MusicalWork::V1`, deposited against the `MusicalWorks` instance.
    #[default]
    #[value(name = "musical-work")]
    MusicalWork,
    /// `Recording::V1`, deposited against the `Recordings` instance. The
    /// runtime-side `Recordings` instance lives in `../Allfeat` and is not
    /// wired yet, so live-node runs need that in place first; payload
    /// generation and reporting are fully functional offline.
    #[value(name = "recording")]
    Recording,
    /// `Release::V1`, deposited against the `Releases` instance. Like
    /// `Recording`, the runtime-side `Releases` instance lives in
    /// `../Allfeat` and is not wired yet; payload generation and reporting
    /// are fully functional offline.
    #[value(name = "release")]
    Release,
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

/// Representation `create` emits. `both` prints the SCALE hex *and* the JSON
/// (the default for a human run); `hex` / `json` isolate one — handy for
/// piping (`midds create --type release --format hex | …`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
pub enum CreateFormat {
    /// SCALE hex (no `0x` prefix) followed by pretty JSON.
    #[default]
    #[value(name = "both")]
    Both,
    /// SCALE hex only, no `0x` prefix.
    #[value(name = "hex")]
    Hex,
    /// Canonical pretty JSON only.
    #[value(name = "json")]
    Json,
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
