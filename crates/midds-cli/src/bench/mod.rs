//! End-to-end node debug tooling — Couche 5 of `docs/testing.md`.
//!
//! Operator-facing tooling that runs against a live node: deterministic
//! mass seeding for frontends/dev environments, fee measurement per deposit,
//! and aggregate throughput benchmarks.

pub(crate) mod fees;
pub(crate) mod progress;
pub(crate) mod seed;
pub(crate) mod throughput;
pub(crate) mod util;
pub(crate) mod worker;
