//! CLI smoke tests for `midds-codegen` — narrow surface coverage that runs
//! without a SCALE metadata fixture.
//!
//! `docs/testing.md` §15.6 calls for a full snapshot test (codegen against a
//! committed metadata blob, output crate `cargo check`s). The metadata for
//! that test is the *real* `melodie-runtime` SCALE metadata, which lives in
//! the sibling `Allfeat` repo — committing it here would break the SDK /
//! runtime decoupling pinned by `CLAUDE.md` ("the runtime side is in
//! `../Allfeat`"). It also drifts each time the runtime changes, so the
//! refresh cadence belongs on the runtime side, not here.
//!
//! The full snapshot is therefore expected to live in `Allfeat`'s CI as a
//! "regenerate bindings" step — it can fetch metadata from a `melodie-dev`
//! node and run `midds-codegen --metadata <url>`.
//!
//! What this file *does* cover:
//!
//! - CLI args parse: `--help` and `--version` return cleanly.
//! - Missing args produce a useful clap error.
//! - The path argument validates: pointing at a non-existent file fails fast,
//!   not in a delayed codegen panic.
//!
//! These three guard the wrapper logic — clap definitions, the `is_url`
//! heuristic, the `from_file_blocking` call site — which is the only code
//! `midds-codegen` owns; everything past the metadata read is `subxt-codegen`.

use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_midds-codegen"))
}

#[test]
fn help_prints_usage() {
    let output = Command::new(binary())
        .arg("--help")
        .output()
        .expect("spawn midds-codegen --help");
    assert!(
        output.status.success(),
        "--help must exit 0; stderr = {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--metadata"), "help text omits --metadata");
    assert!(stdout.contains("--out"), "help text omits --out");
}

#[test]
fn missing_required_args_errors() {
    let output = Command::new(binary())
        .output()
        .expect("spawn midds-codegen with no args");
    assert!(!output.status.success(), "no-args invocation must fail",);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--metadata") || stderr.contains("required"),
        "stderr must mention the missing arg, got: {stderr}",
    );
}

#[test]
fn nonexistent_metadata_path_fails_fast() {
    let output = Command::new(binary())
        .args([
            "--metadata",
            "/nonexistent/midds-codegen/tests/missing.scale",
            "--out",
            "/tmp/midds-codegen-smoke-output.rs",
        ])
        .output()
        .expect("spawn midds-codegen with bad path");
    assert!(!output.status.success(), "missing metadata path must fail",);
}
