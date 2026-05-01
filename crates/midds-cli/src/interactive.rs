//! Interactive prompts for the `midds` debug CLI.
//!
//! The CLI is wizard-first: every command has a full wizard that fires when a
//! required argument is missing, and `midds` (no subcommand) launches a
//! top-level picker that chooses which wizard to run. CLI-provided values
//! always become wizard defaults so partial invocations stay ergonomic.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};

use crate::cli::{SizeDistribution, parse_u64_seed};

/// Top-level command choices presented when `midds` is invoked without a
/// subcommand. Mirrors the visible `Command` variants exposed by clap.
#[derive(Copy, Clone, Debug)]
pub enum CommandChoice {
    Seed,
    BenchFees,
    BenchThroughput,
}

/// Bench-only choices presented when `midds bench` is invoked without a
/// sub-scenario.
#[derive(Copy, Clone, Debug)]
pub enum BenchKindChoice {
    Fees,
    Throughput,
}

/// Ask the user which top-level command they want to run.
pub fn pick_command() -> Result<CommandChoice> {
    let theme = ColorfulTheme::default();
    let labels = [
        "Seed — mass-seed a dev node with deterministic MIDDS records",
        "Bench fees — measure bond + tx fee per deposit",
        "Bench throughput — measure aggregate TPS and latency",
    ];
    let idx = Select::with_theme(&theme)
        .with_prompt("What do you want to do?")
        .items(&labels)
        .default(0)
        .interact()
        .context("pick command")?;
    Ok(match idx {
        0 => CommandChoice::Seed,
        1 => CommandChoice::BenchFees,
        2 => CommandChoice::BenchThroughput,
        _ => unreachable!("Select clamped to items.len()"),
    })
}

/// Ask the user which bench scenario they want to run.
pub fn pick_bench_kind() -> Result<BenchKindChoice> {
    let theme = ColorfulTheme::default();
    let labels = [
        "Fees — bond + tx fee per deposit (sequential per signer; single-signer by default)",
        "Throughput — aggregate TPS and finalisation latency",
    ];
    let idx = Select::with_theme(&theme)
        .with_prompt("Which bench scenario?")
        .items(&labels)
        .default(0)
        .interact()
        .context("pick bench kind")?;
    Ok(match idx {
        0 => BenchKindChoice::Fees,
        1 => BenchKindChoice::Throughput,
        _ => unreachable!("Select clamped to items.len()"),
    })
}

/// Interactive y/N confirmation that bails with an `aborted` error on `false`.
/// Use for irreversible or expensive ops.
pub fn confirm_or_abort(prompt: &str, default: bool) -> Result<()> {
    let answer = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .default(default)
        .interact()
        .map_err(|e| anyhow!("read confirmation: {e}"))?;
    if answer {
        Ok(())
    } else {
        Err(anyhow!("aborted by user"))
    }
}

/// Resolved configuration for `seed`. Doubles as the input to [`seed_wizard`]:
/// every field is treated as the default presented at its prompt, except
/// `count` which is always asked without a default (no sensible value).
#[derive(Clone, Debug)]
pub struct SeedConfig {
    pub count: u32,
    pub rng_seed: u64,
    pub base_signer: String,
    pub signer_count: u32,
    pub concurrency: u32,
    pub batch_size: u32,
    pub auto_fund: bool,
    pub funder: String,
    pub fund_margin: f64,
    pub fund_batch_size: u32,
    pub report_path: Option<PathBuf>,
}

/// Resolved configuration for `bench fees`.
#[derive(Clone, Debug)]
pub struct FeesConfig {
    pub count: u32,
    pub size_distribution: SizeDistribution,
    pub base_signer: String,
    pub signer_count: u32,
    pub concurrency: u32,
    pub batch_size: u32,
    pub auto_fund: bool,
    pub funder: String,
    pub fund_margin: f64,
    pub fund_batch_size: u32,
    pub rng_seed: u64,
    pub out: Option<PathBuf>,
}

/// Resolved configuration for `bench throughput`.
#[derive(Clone, Debug)]
pub struct ThroughputConfig {
    pub count: u32,
    pub duration_secs: Option<u64>,
    pub base_signer: String,
    pub signer_count: u32,
    pub concurrency: u32,
    pub batch_size: u32,
    pub auto_fund: bool,
    pub funder: String,
    pub fund_margin: f64,
    pub fund_batch_size: u32,
    pub rng_seed: u64,
    pub out: Option<PathBuf>,
}

/// Walk the user through every `seed` parameter. Pressing Enter accepts the
/// default carried by `defaults` for that field; `count` is always prompted.
///
/// Funder-related fields (`funder`, `fund_margin`, `fund_batch_size`) are
/// only asked when `auto_fund` is set, otherwise their `defaults` values are
/// returned untouched.
pub fn seed_wizard(defaults: SeedConfig) -> Result<SeedConfig> {
    let theme = ColorfulTheme::default();
    println!("Interactive seed config. Press Enter to accept the [default].\n");

    let count = prompt_u32("Number of records to deposit")?;

    let rng_seed = prompt_u64_seed(&theme, "RNG seed (decimal or 0x..)", defaults.rng_seed)?;

    let base_signer: String = Input::with_theme(&theme)
        .with_prompt("Base signer URI")
        .default(defaults.base_signer.clone())
        .interact_text()
        .context("read base signer URI")?;

    let signer_count = prompt_positive_u32(
        &theme,
        "Signer count (derived //1, //2, ...)",
        defaults.signer_count.max(1),
    )?;

    let concurrency = prompt_positive_u32(
        &theme,
        "Concurrency (capped at signer count)",
        defaults.concurrency.max(1).min(signer_count),
    )?;

    let batch_size = prompt_positive_u32(
        &theme,
        "Batch size (records per `Utility::batch_all`)",
        defaults.batch_size.max(1),
    )?;

    let auto_fund: bool = Confirm::with_theme(&theme)
        .with_prompt("Auto-fund derived signers from funder before seeding?")
        .default(defaults.auto_fund)
        .interact()
        .context("read auto-fund flag")?;

    let (funder, fund_margin, fund_batch_size) = if auto_fund {
        prompt_fund_details(
            &theme,
            &defaults.funder,
            defaults.fund_margin,
            defaults.fund_batch_size,
        )?
    } else {
        (
            defaults.funder.clone(),
            defaults.fund_margin,
            defaults.fund_batch_size,
        )
    };

    let report_path = prompt_optional_path(
        &theme,
        "Report path (blank to skip)",
        defaults.report_path.as_deref(),
    )?;

    Ok(SeedConfig {
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
    })
}

/// Walk the user through every `bench fees` parameter.
pub fn fees_wizard(defaults: FeesConfig) -> Result<FeesConfig> {
    let theme = ColorfulTheme::default();
    println!("Interactive bench fees config. Press Enter to accept the [default].\n");

    let count = prompt_u32("Number of records to deposit and measure")?;

    let size_distribution = prompt_size_distribution(&theme, defaults.size_distribution)?;

    let base_signer: String = Input::with_theme(&theme)
        .with_prompt("Base signer URI (single-signer if signer count = 1)")
        .default(defaults.base_signer.clone())
        .interact_text()
        .context("read base signer URI")?;

    let signer_count = prompt_positive_u32(
        &theme,
        "Signer count (1 = solo; >1 derives //1, //2, … and requires they be funded)",
        defaults.signer_count.max(1),
    )?;

    let concurrency = prompt_positive_u32(
        &theme,
        "Concurrency (capped at signer count)",
        defaults.concurrency.max(1).min(signer_count),
    )?;

    let batch_size = prompt_positive_u32(
        &theme,
        "Batch size (records per `Utility::batch_all`; 1 = per-tx)",
        defaults.batch_size.max(1),
    )?;

    // Funding only makes sense for derived signers — solo `//Alice`-style
    // runs already use a pre-funded account and skip this whole block.
    let (auto_fund, funder, fund_margin, fund_batch_size) = if signer_count > 1 {
        // Multi-signer flows on a fresh chain almost always need funding —
        // make the default `yes` so the happy path is one Enter press, while
        // still letting the user opt out for pre-funded environments.
        let auto_fund: bool = Confirm::with_theme(&theme)
            .with_prompt("Auto-fund derived signers from funder before measuring?")
            .default(true)
            .interact()
            .context("read auto-fund flag")?;
        if auto_fund {
            let (funder, fund_margin, fund_batch_size) = prompt_fund_details(
                &theme,
                &defaults.funder,
                defaults.fund_margin,
                defaults.fund_batch_size,
            )?;
            (true, funder, fund_margin, fund_batch_size)
        } else {
            (
                false,
                defaults.funder.clone(),
                defaults.fund_margin,
                defaults.fund_batch_size,
            )
        }
    } else {
        (
            false,
            defaults.funder.clone(),
            defaults.fund_margin,
            defaults.fund_batch_size,
        )
    };

    let rng_seed = prompt_u64_seed(&theme, "RNG seed (decimal or 0x..)", defaults.rng_seed)?;

    let out = prompt_optional_path(
        &theme,
        "Markdown report path (blank to print to stdout)",
        defaults.out.as_deref(),
    )?;

    Ok(FeesConfig {
        count,
        size_distribution,
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
    })
}

/// Walk the user through every `bench throughput` parameter.
pub fn throughput_wizard(defaults: ThroughputConfig) -> Result<ThroughputConfig> {
    let theme = ColorfulTheme::default();
    println!("Interactive bench throughput config. Press Enter to accept the [default].\n");

    let count = prompt_u32("Target number of records to deposit")?;

    let duration_secs = prompt_optional_u64(
        &theme,
        "Wall-clock cap in seconds (blank for unbounded)",
        defaults.duration_secs,
    )?;

    let base_signer: String = Input::with_theme(&theme)
        .with_prompt("Base signer URI")
        .default(defaults.base_signer.clone())
        .interact_text()
        .context("read base signer URI")?;

    let signer_count = prompt_positive_u32(
        &theme,
        "Signer count (derived //1, //2, ...)",
        defaults.signer_count.max(1),
    )?;

    let concurrency = prompt_positive_u32(
        &theme,
        "Concurrency (capped at signer count)",
        defaults.concurrency.max(1).min(signer_count),
    )?;

    let batch_size = prompt_positive_u32(
        &theme,
        "Batch size (records per `Utility::batch_all`)",
        defaults.batch_size.max(1),
    )?;

    // Funding only makes sense for derived signers — solo runs against
    // `//Alice//1` on a fresh chain need it the most, and a `signer_count == 1`
    // throughput run uses `//Alice//1` (not `//Alice` verbatim, unlike fees) so
    // it benefits from auto-fund just as much as multi-signer.
    let auto_fund: bool = Confirm::with_theme(&theme)
        .with_prompt("Auto-fund derived signers from funder before measuring?")
        .default(defaults.auto_fund)
        .interact()
        .context("read auto-fund flag")?;

    let (funder, fund_margin, fund_batch_size) = if auto_fund {
        prompt_fund_details(
            &theme,
            &defaults.funder,
            defaults.fund_margin,
            defaults.fund_batch_size,
        )?
    } else {
        (
            defaults.funder.clone(),
            defaults.fund_margin,
            defaults.fund_batch_size,
        )
    };

    let rng_seed = prompt_u64_seed(&theme, "RNG seed (decimal or 0x..)", defaults.rng_seed)?;

    let out = prompt_optional_path(
        &theme,
        "JSON report path (blank to print to stdout)",
        defaults.out.as_deref(),
    )?;

    Ok(ThroughputConfig {
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
    })
}

/// Walk through the three follow-up questions that always come after the
/// `auto-fund?` toggle: funder URI, safety margin, and batch size for the
/// funding `batch_all`. Shared between [`seed_wizard`] and [`fees_wizard`]
/// because both gate funding the same way and ask identical questions.
fn prompt_fund_details(
    theme: &ColorfulTheme,
    funder_default: &str,
    margin_default: f64,
    batch_size_default: u32,
) -> Result<(String, f64, u32)> {
    let funder: String = Input::with_theme(theme)
        .with_prompt("Funder URI")
        .default(funder_default.to_string())
        .interact_text()
        .context("read funder URI")?;
    let fund_margin: f64 = Input::with_theme(theme)
        .with_prompt("Fund margin (multiplier on computed bond)")
        .default(margin_default.max(1.0))
        .validate_with(|f: &f64| -> Result<(), &'static str> {
            if *f >= 1.0 {
                Ok(())
            } else {
                Err("must be >= 1.0")
            }
        })
        .interact_text()
        .context("read fund margin")?;
    let fund_batch_size = prompt_positive_u32(
        theme,
        "Fund batch size (transfers per `batch_all`)",
        batch_size_default.max(1),
    )?;
    Ok((funder, fund_margin, fund_batch_size))
}

fn prompt_u32(prompt: &str) -> Result<u32> {
    Input::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .validate_with(|n: &u32| -> Result<(), &'static str> {
            if *n == 0 { Err("must be > 0") } else { Ok(()) }
        })
        .interact_text()
        .context("read count")
}

fn prompt_positive_u32(
    theme: &ColorfulTheme,
    label: impl Into<String>,
    default: u32,
) -> Result<u32> {
    let label = label.into();
    Input::with_theme(theme)
        .with_prompt(label.clone())
        .default(default)
        .validate_with(|n: &u32| -> Result<(), &'static str> {
            if *n == 0 { Err("must be >= 1") } else { Ok(()) }
        })
        .interact_text()
        .with_context(|| format!("read `{label}`"))
}

fn prompt_u64_seed(theme: &ColorfulTheme, label: &str, default: u64) -> Result<u64> {
    let raw: String = Input::with_theme(theme)
        .with_prompt(label)
        .default(format!("0x{default:016x}"))
        .validate_with(|s: &String| -> Result<(), String> { parse_u64_seed(s).map(|_| ()) })
        .interact_text()
        .with_context(|| format!("read `{label}`"))?;
    Ok(parse_u64_seed(&raw).expect("validated above"))
}

fn prompt_optional_u64(
    theme: &ColorfulTheme,
    label: &str,
    default: Option<u64>,
) -> Result<Option<u64>> {
    let default_str = default.map(|n| n.to_string()).unwrap_or_default();
    let raw: String = Input::with_theme(theme)
        .with_prompt(label)
        .default(default_str)
        .allow_empty(true)
        .validate_with(|s: &String| -> Result<(), String> {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(());
            }
            trimmed
                .parse::<u64>()
                .map(|_| ())
                .map_err(|e| format!("not a valid u64: {e}"))
        })
        .interact_text()
        .with_context(|| format!("read `{label}`"))?;
    let trimmed = raw.trim();
    Ok(if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.parse().expect("validated above"))
    })
}

fn prompt_optional_path(
    theme: &ColorfulTheme,
    label: &str,
    default: Option<&std::path::Path>,
) -> Result<Option<PathBuf>> {
    let default_str = default.map(|p| p.display().to_string()).unwrap_or_default();
    let raw: String = Input::with_theme(theme)
        .with_prompt(label)
        .default(default_str)
        .allow_empty(true)
        .interact_text()
        .with_context(|| format!("read `{label}`"))?;
    let trimmed = raw.trim();
    Ok(if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    })
}

fn prompt_size_distribution(
    theme: &ColorfulTheme,
    default: SizeDistribution,
) -> Result<SizeDistribution> {
    const LABELS: [&str; 3] = [
        "Real (realistic mix, ~50–250 byte payloads)",
        "Max (worst-case, MaxEncodedLen)",
        "Mixed (round-robin min/real/max)",
    ];
    const VALUES: [SizeDistribution; 3] = [
        SizeDistribution::Real,
        SizeDistribution::Max,
        SizeDistribution::Mixed,
    ];
    let default_idx = VALUES.iter().position(|v| *v == default).unwrap_or(0);
    let idx = Select::with_theme(theme)
        .with_prompt("Payload size distribution")
        .items(&LABELS)
        .default(default_idx)
        .interact()
        .context("pick size distribution")?;
    Ok(VALUES[idx])
}
