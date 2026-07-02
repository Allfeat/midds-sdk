//! Polling helpers for read-after-write.
//!
//! After [`MiddsClient::at_best_block`](midds_client::MiddsClient::at_best_block)
//! routed every read through the latest best block, the original
//! inclusion/finalisation race that motivated this module mostly evaporated
//! — reads now see the deposit as soon as `wait_for_in_block` returns.
//!
//! The polling stays around for two scenarios it still owns:
//! - **Out-of-band writers**, like the `cli_smoke` test which spawns
//!   `midds-cli seed` as a subprocess: the parent test can only observe
//!   the state after the child exits, and a small lag between the child's
//!   `wait_for_in_block` and the parent's read can still race.
//! - **RPC reads that aren't pinned via `at_best_block`** — e.g. the
//!   `rpc_namespace` test hits the JSON-RPC bridge directly, which reads
//!   at `best_hash` but on a different code path; the poll absorbs any
//!   handover latency.
//!
//! On the happy path the closure returns on first invocation, so these
//! helpers add nanoseconds, not seconds.

use std::future::Future;
use std::time::Duration;

/// 60 × 500 ms = 30 s budget. `allfeat --dev` produces a block every ~6 s
/// and GRANDPA finalises with a 2-3 block lag, so finality of a freshly-
/// included extrinsic lands ~18-24 s later. 30 s leaves comfortable
/// headroom over that worst case without making a green run noticeably
/// slow (each poll returns on first success).
const MAX_ATTEMPTS: u32 = 60;
const SLEEP_BETWEEN: Duration = Duration::from_millis(500);

/// Repeatedly run `f` until it yields `Some(_)`, then return it. Panics with
/// a labelled message after `MAX_ATTEMPTS` tries.
///
/// `label` is a short description of what's being waited on — gets surfaced
/// in the failure message so the panic identifies the failing read site.
pub async fn wait_until_some<T, F, Fut>(label: &str, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    for attempt in 0..MAX_ATTEMPTS {
        if let Some(v) = f().await {
            return v;
        }
        if attempt + 1 < MAX_ATTEMPTS {
            tokio::time::sleep(SLEEP_BETWEEN).await;
        }
    }
    panic!(
        "[e2e] timed out waiting for `{label}` after {MAX_ATTEMPTS} attempts \
         ({:.1} s) — likely a finalisation lag or a missing state update",
        f64::from(MAX_ATTEMPTS) * SLEEP_BETWEEN.as_secs_f64(),
    );
}

/// Variant for predicates: repeatedly run `f` until it yields `true`.
/// Panics with a labelled message on timeout, same budget as
/// [`wait_until_some`].
pub async fn wait_until<F, Fut>(label: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    for attempt in 0..MAX_ATTEMPTS {
        if f().await {
            return;
        }
        if attempt + 1 < MAX_ATTEMPTS {
            tokio::time::sleep(SLEEP_BETWEEN).await;
        }
    }
    panic!(
        "[e2e] timed out waiting for `{label}` after {MAX_ATTEMPTS} attempts \
         ({:.1} s)",
        f64::from(MAX_ATTEMPTS) * SLEEP_BETWEEN.as_secs_f64(),
    );
}
