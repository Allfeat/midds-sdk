//! Connection helpers.
//!
//! [`try_connect`] returns `None` instead of panicking when the node is
//! unreachable, so the test body can `return` and the run stays green in
//! environments where no node is booted (the workspace default).

use std::time::Duration;

use midds_client::MiddsClient;
use midds_traits::MiddsId;
use tokio::time::timeout;

use crate::{env, poll};

/// How long to wait for the WS handshake before giving up and skipping.
///
/// Three seconds covers a cold local node (subxt's metadata fetch is the
/// long pole) without making the skip path hang on hosts where the port is
/// simply silent.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Connect to the node at [`env::ws_url`], or return `None` and log a skip
/// reason when no node responds.
pub async fn try_connect() -> Option<MiddsClient> {
    let url = env::ws_url();
    match timeout(CONNECT_TIMEOUT, MiddsClient::connect(&url)).await {
        Ok(Ok(client)) => Some(client),
        Ok(Err(e)) => {
            eprintln!("[e2e] skip: connect to {url} failed: {e}");
            None
        }
        Err(_) => {
            eprintln!(
                "[e2e] skip: no node at {url} after {CONNECT_TIMEOUT:?} — boot `allfeat --dev` to run e2e",
            );
            None
        }
    }
}

/// Block until the deposit at `id` is observable on the `MusicalWorks`
/// instance at the current read view.
///
/// Now that [`MiddsClient::at_best_block`] pins reads to the best block,
/// this is mostly a defensive no-op — `next_midds_id` reflects the deposit
/// as soon as `wait_for_in_block` returns. The helper stays around as a
/// belt-and-braces sync point for tests that intermix subxt-driven and
/// out-of-band writers (e.g. `cli_smoke`'s subprocess) where the parent
/// process can't observe the child's `wait_for_in_block` directly.
///
/// `next_midds_id` is the cheapest signal we can read from `midds-client`'s
/// public surface — it strictly advances past every deposited id, so
/// `next > id` means our deposit hit at least one block the read view
/// covers.
pub async fn wait_for_visible_musical_works_deposit(client: &MiddsClient, id: MiddsId) {
    poll::wait_until("deposit visible at read view", || async {
        // A transient RPC failure is "not visible yet", not a test abort —
        // the poll's own attempt cap turns a persistent failure into a
        // labelled panic.
        match client.musical_works().next_midds_id().await {
            Ok(next) => next > id,
            Err(e) => {
                eprintln!("[e2e] next_midds_id read failed (retrying): {e}");
                false
            }
        }
    })
    .await;
}
