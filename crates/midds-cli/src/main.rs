//! `midds` debug CLI entry point.

mod admin;
mod bench;
mod cli;
mod format;
mod interactive;
mod signers;

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

async fn run(cli: Cli) -> Result<()> {
    let command = resolve_command(cli.command)?;
    match command {
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
            // Bind the MIDDS-type-generic harness to the chosen kind: each
            // arm picks the `MiddsFixtures` impl and the `midds-client`
            // pallet-instance accessor. Adding `Release` later is one more
            // arm here plus the `MiddsKind` variant.
            match midds_type {
                MiddsKind::MusicalWork => {
                    bench::seed::run::<MusicalWorkFixtures>(args, MiddsClient::musical_works).await
                }
                MiddsKind::Recording => {
                    bench::seed::run::<RecordingFixtures>(args, MiddsClient::recordings).await
                }
                MiddsKind::Release => {
                    bench::seed::run::<ReleaseFixtures>(args, MiddsClient::releases).await
                }
            }
        }
        Command::Bench { kind } => match kind {
            // `resolve_command` guarantees this arm always carries a concrete
            // sub-scenario by the time we get here.
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
                match midds_type {
                    MiddsKind::MusicalWork => {
                        bench::fees::run::<MusicalWorkFixtures>(args, MiddsClient::musical_works)
                            .await
                    }
                    MiddsKind::Recording => {
                        bench::fees::run::<RecordingFixtures>(args, MiddsClient::recordings).await
                    }
                    MiddsKind::Release => {
                        bench::fees::run::<ReleaseFixtures>(args, MiddsClient::releases).await
                    }
                }
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
                match midds_type {
                    MiddsKind::MusicalWork => {
                        bench::throughput::run::<MusicalWorkFixtures>(
                            args,
                            MiddsClient::musical_works,
                        )
                        .await
                    }
                    MiddsKind::Recording => {
                        bench::throughput::run::<RecordingFixtures>(args, MiddsClient::recordings)
                            .await
                    }
                    MiddsKind::Release => {
                        bench::throughput::run::<ReleaseFixtures>(args, MiddsClient::releases).await
                    }
                }
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
