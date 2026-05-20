//! End-to-end test scaffolding for the MIDDS SDK.
//!
//! The crate's `[[test]]` targets in `tests/` exercise the full
//! types → pallet → runtime-api → rpc → client → cli stack against a real
//! running `allfeat --dev` node, so a passing `cargo test -p midds-e2e`
//! means every inter-crate seam is in lock-step.
//!
//! # Running locally
//!
//! 1. Boot the node in another terminal:
//!    ```bash
//!    ../Allfeat/target/release/allfeat --dev --tmp
//!    ```
//! 2. Run the suite:
//!    ```bash
//!    cargo test -p midds-e2e
//!    ```
//!
//! When no node responds at [`env::ws_url`] (default `ws://127.0.0.1:9944`)
//! every test returns early with a `[e2e] skip:` log line, so the workspace's
//! `cargo test` stays green for contributors who don't have a node booted on
//! the side.
//!
//! # Cross-run isolation
//!
//! Tests share the same chain, so deposits accumulate forever. To stop
//! `AlreadyExists` collisions, [`session`] mints a process-unique base index
//! derived from system time at nanosecond resolution and an atomic per-test
//! slot. Two `cargo test` invocations a millisecond apart hand out disjoint
//! identifiers; tests inside a single run also never collide.

pub mod client;
pub mod env;
pub mod poll;
pub mod session;
pub mod signer;
pub mod tx;
