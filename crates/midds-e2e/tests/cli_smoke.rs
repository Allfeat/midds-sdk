//! `midds seed --count 5 --auto-fund --yes` succeeds against the dev node
//! and produces a coherent post-state.
//!
//! Closes the loop on the operator-facing surface: the binary, its argument
//! parsing, the interactive `--yes` bypass, the auto-fund pipeline, the
//! batched deposits, and the runtime API read that `midds-cli` performs at
//! the end — all exercised in one shot. A regression in any of the layers
//! that feed `midds-cli` (fixtures, client, RPC) will surface here as a
//! non-zero exit or a `next_midds_id` that didn't move.
//!
//! Spawning via `cargo run -p midds-cli` makes the test slow on first
//! invocation (~30s to build the binary), so this lives in its own test
//! file and is excluded from quick re-runs by virtue of the network-skip
//! contract: no node → instant `return`.

use midds_e2e::{client, env, poll, session};
use tokio::process::Command;

const SEED_COUNT: u32 = 5;

#[tokio::test]
async fn seed_command_smoke_against_dev_node() {
    let Some(client) = client::try_connect().await else {
        return;
    };

    let before = client
        .musical_works()
        .next_midds_id()
        .await
        .expect("read next_midds_id before seed");

    let url = env::ws_url();
    let rng_seed = format!("0x{:016x}", session::base_index() as u64);
    let output = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "-p",
            "midds-cli",
            "--",
            "--url",
            &url,
            "seed",
            "--count",
            &SEED_COUNT.to_string(),
            "--rng-seed",
            &rng_seed,
            "--auto-fund",
            "--yes",
        ])
        .output()
        .await
        .expect("spawn cargo run -p midds-cli");

    assert!(
        output.status.success(),
        "`midds seed` exited {:?}\n--- stderr ---\n{}\n--- stdout ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );

    let after = poll::wait_until_some("next_midds_id reflects seed", || async {
        let v = client
            .musical_works()
            .next_midds_id()
            .await
            .expect("read next_midds_id after seed");
        (v >= before + u64::from(SEED_COUNT)).then_some(v)
    })
    .await;
    assert!(
        after >= before + u64::from(SEED_COUNT),
        "`midds seed --count {SEED_COUNT}` did not move next_midds_id far enough (before={before}, after={after})",
    );
}
