// Shared curated `clippy::pedantic` policy — identical in every crate root;
// anything not listed here must stay pedantic-clean.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::if_not_else,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::option_option,
    clippy::return_self_not_must_use,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
//! `midds` debug CLI entry point.

mod bench;
mod cli;
mod create;
mod format;
mod funding;
mod interactive;
mod signers;
mod ui;

use anyhow::{Result, anyhow};
use clap::Parser;
use midds_client::MiddsClient;
use midds_fixtures::{
    musical_work::MusicalWorkFixtures, recording::RecordingFixtures, release::ReleaseFixtures,
};

use crate::cli::{BenchArgs, Cli, Command, MiddsKind};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(run(cli))
}

/// Bind `bench::$module::run::<F>(args, accessor)` to the chosen
/// [`MiddsKind`]. Each arm pairs the `MiddsFixtures` impl with the
/// `midds-client` pallet-instance accessor. Adding a new MIDDS kind later is
/// one variant on [`MiddsKind`] and one arm here — the rest of the harness
/// stays generic.
///
/// [`MiddsKind::All`] is handled outside this macro: `seed` routes it to
/// [`bench::seed::run_all`], and `bench fees` / `bench throughput` reject it
/// upfront because their reports are per-type by construction. The macro
/// itself returns a clear error if `All` ever reaches it.
macro_rules! dispatch_kind {
    ($module:ident, $kind:expr, $args:expr) => {
        match $kind {
            MiddsKind::MusicalWork => {
                bench::$module::run::<MusicalWorkFixtures>($args, MiddsClient::musical_works).await
            }
            MiddsKind::Recording => {
                bench::$module::run::<RecordingFixtures>($args, MiddsClient::recordings).await
            }
            MiddsKind::Release => {
                bench::$module::run::<ReleaseFixtures>($args, MiddsClient::releases).await
            }
            MiddsKind::All => Err(anyhow!(
                "`--midds-type all` is only supported by `seed`; \
                 pick `musical-work`, `recording`, or `release` for the bench commands"
            )),
        }
    };
}

async fn run(cli: Cli) -> Result<()> {
    if cli.command.is_none() {
        ui::banner();
    }
    let command = resolve_command(cli.command)?;
    match command {
        Command::Create {
            midds_type,
            out,
            format,
        } => create::run(midds_type, out, format),
        Command::Seed {
            midds_type,
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
            report,
            yes,
        } => {
            let args = bench::seed::Args {
                url: &cli.url,
                count,
                rng_seed,
                base_signer: &base_signer,
                signer_count,
                concurrency,
                batch_size,
                auto_fund,
                funder: &funder,
                fund_margin,
                fund_batch_size,
                report_path: report.as_deref(),
                assume_yes: yes,
            };
            match midds_type {
                MiddsKind::All => bench::seed::run_all(args).await,
                other => dispatch_kind!(seed, other, args),
            }
        }
        Command::Bench { kind } => match kind {
            Some(BenchArgs::Fees {
                midds_type,
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
                yes,
            }) => {
                let args = bench::fees::Args {
                    url: &cli.url,
                    count,
                    distribution: size_distribution,
                    base_signer: &base_signer,
                    signer_count,
                    concurrency,
                    batch_size,
                    auto_fund,
                    funder: &funder,
                    fund_margin,
                    fund_batch_size,
                    rng_seed,
                    out: out.as_deref(),
                    assume_yes: yes,
                };
                dispatch_kind!(fees, midds_type, args)
            }
            Some(BenchArgs::Throughput {
                midds_type,
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
                yes,
            }) => {
                let args = bench::throughput::Args {
                    url: &cli.url,
                    count,
                    duration_secs,
                    base_signer: &base_signer,
                    signer_count,
                    concurrency,
                    batch_size,
                    auto_fund,
                    funder: &funder,
                    fund_margin,
                    fund_batch_size,
                    rng_seed,
                    out: out.as_deref(),
                    assume_yes: yes,
                };
                dispatch_kind!(throughput, midds_type, args)
            }
            None => unreachable!("resolve_command always promotes Bench.kind to Some"),
        },
    }
}

/// Promote a (possibly missing) parsed [`Command`] into a fully concrete one
/// by interactively asking the user when the subcommand or sub-scenario is
/// missing. The downstream wizard for the chosen command then fires because
/// `count = None` propagates through clap's defaults.
fn resolve_command(cmd: Option<Command>) -> Result<Command> {
    match cmd {
        None => promote_command_choice(interactive::pick_command()?),
        Some(Command::Bench { kind: None }) => {
            let inner = match interactive::pick_bench_kind()? {
                interactive::BenchKindChoice::Fees => interactive::CommandChoice::BenchFees,
                interactive::BenchKindChoice::Throughput => {
                    interactive::CommandChoice::BenchThroughput
                }
            };
            promote_command_choice(inner)
        }
        Some(c) => Ok(c),
    }
}

/// Materialise a [`Command`] populated with clap's declared defaults for the
/// given choice. Re-using clap as the defaults engine keeps the wizard
/// initial values in lock-step with the non-interactive CLI without
/// duplicating the constants here.
fn promote_command_choice(choice: interactive::CommandChoice) -> Result<Command> {
    let argv: &[&str] = match choice {
        interactive::CommandChoice::Create => &["midds", "create"],
        interactive::CommandChoice::Seed => &["midds", "seed"],
        interactive::CommandChoice::BenchFees => &["midds", "bench", "fees"],
        interactive::CommandChoice::BenchThroughput => &["midds", "bench", "throughput"],
    };
    let parsed =
        Cli::try_parse_from(argv).map_err(|e| anyhow!("build defaults for {choice:?}: {e}"))?;
    parsed
        .command
        .ok_or_else(|| anyhow!("clap returned no command for {choice:?}"))
}
