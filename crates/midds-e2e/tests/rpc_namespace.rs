//! Per-instance JSON-RPC namespace is reachable through `midds-rpc`.
//!
//! `midds-rpc` stamps one handler per pallet instance via `midds_rpc_instance!`
//! and publishes its methods under a `midds_<instance>_` prefix (see
//! `crates/midds-rpc/src/lib.rs`). This test goes around `midds-client`
//! entirely and pokes the JSON-RPC endpoint directly, validating:
//!
//! - the `MusicalWorks` instance is wired to the namespace
//!   `midds_musicalWorks_` (the commit `09e7e62` shape — symmetric
//!   namespacing across kinds);
//! - the runtime-API → JSON-RPC bridge correctly forwards reads (we hit
//!   `weeklyTarget`, a parameter-less u32 endpoint, then `get` against an
//!   id we just deposited).
//!
//! A regression in the macro (e.g. the namespace drifting, the method names
//! changing case) makes the JSON-RPC call return `MethodNotFound` and this
//! test fails loud — exactly the wire-shape drift we want to catch.

use midds_e2e::{client, env, poll, session, signer};
use subxt::rpcs::{RpcClient, client::rpc_params};

/// Method names mirror the `midds_rpc_instance!` invocation in
/// `crates/midds-rpc/src/lib.rs` for the `MusicalWorks` instance —
/// jsonrpsee composes them as `<namespace>_<methodName>`.
const NS_WEEKLY_TARGET: &str = "midds_musicalWorks_weeklyTarget";
const NS_GET: &str = "midds_musicalWorks_get";

#[tokio::test]
async fn musical_works_namespace_responds() {
    let Some(client) = client::try_connect().await else {
        return;
    };

    // Deposit something so the `_get` call has a non-`None` answer to
    // return — that proves the bridge round-trips runtime-API output as
    // JSON, not just that the method is reachable.
    let alice = signer::alice();
    let work = session::fresh_musical_work();
    let id = client
        .musical_works()
        .deposit(&alice, work)
        .await
        .expect("deposit");

    // Open a raw JSON-RPC channel to the same node. `from_insecure_url`
    // accepts the plain `ws://` URL we'd point at in dev — TLS isn't part
    // of the e2e contract.
    let url = env::ws_url();
    let rpc = RpcClient::from_insecure_url(&url)
        .await
        .expect("RpcClient::from_insecure_url");

    // 1. Parameter-less u32 endpoint: weekly target. If the namespace
    //    prefix is wrong (or the macro stopped emitting symmetric names),
    //    jsonrpsee responds with `Method not found` and the deserialize
    //    fails with a serde error from the RPC payload.
    let _: u32 = rpc
        .request(NS_WEEKLY_TARGET, rpc_params![])
        .await
        .expect("midds_musicalWorks_weeklyTarget must be reachable and return u32");

    // 2. `get(id, at=None)` returns `Option<MusicalWork>`. Decoding the
    //    response as `serde_json::Value` keeps the test agnostic to the
    //    exact wire shape of `MusicalWork` (which uses bounded vecs whose
    //    JSON representation we don't pin here). Polled because the RPC
    //    handler — like `midds-client` — reads at the finalised block, so
    //    it lags `wait_for_in_block`-style deposits by 1-2 blocks.
    let resp = poll::wait_until_some("RPC get returns the deposited record", || async {
        let v: serde_json::Value = rpc
            .request(NS_GET, rpc_params![id])
            .await
            .expect("midds_musicalWorks_get must be reachable");
        (!v.is_null()).then_some(v)
    })
    .await;
    assert!(
        !resp.is_null(),
        "get(id) must return a MusicalWork right after deposit, got null",
    );

    // Belt-and-braces sync for the next test in the suite — kept now
    // that `MiddsClient::at_best_block` makes cross-test nonce contention
    // disappear, but it's cheap and pins the contract.
    client::wait_for_visible_musical_works_deposit(&client, id).await;
}
