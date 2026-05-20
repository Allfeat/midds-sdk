//! Environment lookup for the e2e harness.
//!
//! Tests read the WebSocket URL of a node the operator booted on the side;
//! when nothing answers, the test skips cleanly instead of failing.

/// Environment variable holding the node WebSocket URL.
pub const WS_URL_VAR: &str = "MIDDS_E2E_WS";

/// Default node URL: a fresh `allfeat --dev` listens here.
pub const DEFAULT_WS_URL: &str = "ws://127.0.0.1:9944";

/// Resolved node URL — `MIDDS_E2E_WS` when set, otherwise [`DEFAULT_WS_URL`].
pub fn ws_url() -> String {
    std::env::var(WS_URL_VAR).unwrap_or_else(|_| DEFAULT_WS_URL.into())
}
