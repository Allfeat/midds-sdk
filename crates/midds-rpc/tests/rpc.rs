//! JSON-RPC shape tests for the generated per-instance RPC traits —
//! section 15.4 of `docs/testing.md`.
//!
//! Spinning up a real `ProvideRuntimeApi` + `HeaderBackend` test harness would
//! drag in `substrate-test-runtime` and most of `sc-client`, none of which the
//! SDK depends on. The pragmatic alternative — and what `docs/testing.md`
//! §15.4 actually asks for — is a hand-written stub that implements the
//! generated `MusicalWorkRpcApiServer` trait directly. The shape of the JSON
//! response is determined by the `#[rpc(server)]` macro, so the stub
//! exercises the same wire format the production handler emits. Every MIDDS
//! kind is stamped from the same `midds_rpc_instance!` body, so the
//! `MusicalWork` surface validated here also pins `RecordingRpc` by
//! construction — only the `midds_recordings_` namespace differs.
//!
//! The cardinal assertion: the lookup family returns JSON `null` (or an
//! empty list, for the multi-claim case) when the queried entry is absent —
//! never a JSON-RPC error object. Front-ends rely on this, and a regression
//! would either crash their happy path or surface user-hostile error pop-ups.

use std::{collections::BTreeMap, sync::Mutex};

use jsonrpsee::core::RpcResult;
use midds_fixtures::musical_work::MusicalWorkBuilder;
use midds_rpc::{BondLayerOf, DepositInfoOf, MusicalWorkRpcApiServer};
use midds_traits::{Iswc, Midds as _, MiddsId};
use midds_types::MusicalWork;
use sp_runtime::FixedU128;

/// Concrete generic instantiation used across the tests.
type AccountId = u64;
type Balance = u128;
type BlockHash = u64;

/// In-memory backing store implementing the generated `MiddsRpcApiServer`.
#[derive(Default)]
struct Stub {
    state: Mutex<StubState>,
}

#[derive(Default)]
struct StubState {
    items: BTreeMap<MiddsId, MusicalWork>,
    /// Multi-claim — every identifier maps to a list of ids.
    claims: BTreeMap<Iswc, Vec<MiddsId>>,
    deposit_info: BTreeMap<MiddsId, DepositInfoOf<AccountId, Balance>>,
}

impl Stub {
    fn insert(
        &self,
        id: MiddsId,
        work: MusicalWork,
        depositor: AccountId,
        bond: Balance,
        base_bond: Balance,
        finalized: bool,
    ) {
        let mut s = self.state.lock().expect("stub state");
        s.claims
            .entry(work.identifier().clone())
            .or_default()
            .push(id);
        s.items.insert(id, work);
        s.deposit_info.insert(
            id,
            DepositInfoOf {
                depositor,
                sponsor_layer: BondLayerOf {
                    payer: depositor,
                    amount: bond,
                    base: base_bond,
                },
                owner_layer: None,
                finalized,
            },
        );
    }
}

impl MusicalWorkRpcApiServer<BlockHash, Iswc, MusicalWork, AccountId, Balance> for Stub {
    fn lookup_by_identifier(
        &self,
        identifier: Iswc,
        _at: Option<BlockHash>,
    ) -> RpcResult<Vec<MiddsId>> {
        Ok(self
            .state
            .lock()
            .expect("stub state")
            .claims
            .get(&identifier)
            .cloned()
            .unwrap_or_default())
    }

    fn lookup_by_identifier_paged(
        &self,
        identifier: Iswc,
        after: Option<MiddsId>,
        limit: u32,
        _at: Option<BlockHash>,
    ) -> RpcResult<Vec<MiddsId>> {
        let mut ids: Vec<MiddsId> = self
            .state
            .lock()
            .expect("stub state")
            .claims
            .get(&identifier)
            .cloned()
            .unwrap_or_default();
        ids.sort_unstable();
        ids.retain(|id| match after {
            Some(after_id) => *id > after_id,
            None => true,
        });
        ids.truncate(limit as usize);
        Ok(ids)
    }

    fn count_by_identifier(&self, identifier: Iswc, _at: Option<BlockHash>) -> RpcResult<u32> {
        Ok(self
            .state
            .lock()
            .expect("stub state")
            .claims
            .get(&identifier)
            .map(|v| v.len() as u32)
            .unwrap_or(0))
    }

    fn get(&self, id: MiddsId, _at: Option<BlockHash>) -> RpcResult<Option<MusicalWork>> {
        Ok(self
            .state
            .lock()
            .expect("stub state")
            .items
            .get(&id)
            .cloned())
    }

    fn deposit_info(
        &self,
        id: MiddsId,
        _at: Option<BlockHash>,
    ) -> RpcResult<Option<DepositInfoOf<AccountId, Balance>>> {
        Ok(self
            .state
            .lock()
            .expect("stub state")
            .deposit_info
            .get(&id)
            .cloned())
    }

    fn current_deposit_price(&self, size: u32, _at: Option<BlockHash>) -> RpcResult<Balance> {
        Ok(10 + size as Balance)
    }

    fn current_multipliers(&self, _at: Option<BlockHash>) -> RpcResult<(FixedU128, FixedU128)> {
        Ok((FixedU128::from_u32(1), FixedU128::from_u32(1)))
    }

    fn weekly_target(&self, _at: Option<BlockHash>) -> RpcResult<u32> {
        Ok(200_000)
    }

    fn weekly_actual(&self, _at: Option<BlockHash>) -> RpcResult<u32> {
        Ok(0)
    }
}

/// Send `request` through the in-memory `RpcModule` and return the parsed
/// response object so tests can assert on `result` vs `error` directly.
async fn call(stub: Stub, request: &str) -> serde_json::Value {
    let module = stub.into_rpc();
    let (raw, _rx) = module
        .raw_json_request(request, 1)
        .await
        .expect("raw_json_request");
    serde_json::from_str(&raw).expect("response is JSON")
}

fn sample_work() -> MusicalWork {
    MusicalWorkBuilder::new()
        .title(b"Sample")
        .creation_year(2024)
        .build()
}

#[tokio::test]
async fn lookup_missing_identifier_returns_empty_array() {
    let stub = Stub::default();
    let iswc = midds_fixtures::identifiers::iswc_for_index(99_999).to_vec();
    let request = format!(
        r#"{{"jsonrpc":"2.0","method":"midds_musicalWorks_lookupByIdentifier",
             "params":[{iswc:?}, null],"id":0}}"#
    );

    let resp = call(stub, &request).await;
    let result = resp.get("result").expect("result present");
    assert!(
        result.as_array().map(|a| a.is_empty()).unwrap_or(false),
        "missing identifier must surface as an empty JSON array, got: {resp}",
    );
    assert!(
        resp.get("error").is_none(),
        "missing identifier must not be reported as a JSON-RPC error: {resp}",
    );
}

#[tokio::test]
async fn lookup_present_identifier_returns_ids() {
    let stub = Stub::default();
    let work = sample_work();
    let iswc = work.identifier();
    stub.insert(7, work.clone(), 1, 100, 100, false);

    let bytes = iswc.to_vec();
    let request = format!(
        r#"{{"jsonrpc":"2.0","method":"midds_musicalWorks_lookupByIdentifier",
             "params":[{bytes:?}, null],"id":0}}"#
    );

    let resp = call(stub, &request).await;
    let result = resp.get("result").expect("result present");
    let arr = result.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_u64(), Some(7));
}

#[tokio::test]
async fn lookup_returns_all_claims_for_an_identifier() {
    let stub = Stub::default();
    let work = sample_work();
    let other = MusicalWorkBuilder::new()
        .title(b"Other")
        .creation_year(2024)
        .build();
    stub.insert(7, work.clone(), 1, 100, 100, false);
    stub.insert(11, other.clone(), 2, 100, 100, false);

    let bytes = work.identifier().to_vec();
    let request = format!(
        r#"{{"jsonrpc":"2.0","method":"midds_musicalWorks_lookupByIdentifier",
             "params":[{bytes:?}, null],"id":0}}"#
    );
    let resp = call(stub, &request).await;
    let result = resp.get("result").expect("result present");
    let arr = result.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    let mut ids: Vec<u64> = arr.iter().map(|v| v.as_u64().expect("u64")).collect();
    ids.sort();
    assert_eq!(ids, vec![7, 11]);
}

#[tokio::test]
async fn get_missing_id_returns_json_null() {
    let stub = Stub::default();
    let request =
        r#"{"jsonrpc":"2.0","method":"midds_musicalWorks_get","params":[42, null],"id":0}"#;

    let resp = call(stub, request).await;
    assert!(
        resp.get("result").map(|v| v.is_null()).unwrap_or(false),
        "missing id must surface as JSON null, got: {resp}",
    );
    assert!(resp.get("error").is_none(), "must not error: {resp}");
}

#[tokio::test]
async fn get_present_id_returns_serialized_work() {
    let stub = Stub::default();
    let work = sample_work();
    stub.insert(11, work.clone(), 2, 250, 250, false);

    let request =
        r#"{"jsonrpc":"2.0","method":"midds_musicalWorks_get","params":[11, null],"id":0}"#;
    let resp = call(stub, request).await;
    let result = resp.get("result").expect("result present");
    assert!(!result.is_null(), "present id must not return null: {resp}");

    let roundtripped: MusicalWork =
        serde_json::from_value(result.clone()).expect("deserialize work");
    assert_eq!(roundtripped, work);
}

#[tokio::test]
async fn deposit_info_missing_id_returns_json_null() {
    let stub = Stub::default();
    let request =
        r#"{"jsonrpc":"2.0","method":"midds_musicalWorks_depositInfo","params":[5, null],"id":0}"#;

    let resp = call(stub, request).await;
    assert!(
        resp.get("result").map(|v| v.is_null()).unwrap_or(false),
        "missing id must surface as JSON null, got: {resp}",
    );
}

#[tokio::test]
async fn deposit_info_present_id_returns_full_view() {
    let stub = Stub::default();
    let work = sample_work();
    stub.insert(3, work, 99, 12_345, 10_000, false);

    let request =
        r#"{"jsonrpc":"2.0","method":"midds_musicalWorks_depositInfo","params":[3, null],"id":0}"#;
    let resp = call(stub, request).await;
    let result = resp.get("result").expect("result present");
    let obj = result
        .as_object()
        .expect("DepositInfoOf serializes as JSON object");
    assert_eq!(
        obj.len(),
        4,
        "DepositInfoOf has exactly 4 fields: depositor, sponsor_layer, owner_layer, finalized"
    );
    assert_eq!(obj.get("depositor").and_then(|v| v.as_u64()), Some(99));
    let sponsor = obj
        .get("sponsor_layer")
        .and_then(|v| v.as_object())
        .expect("sponsor_layer object");
    assert_eq!(sponsor.get("payer").and_then(|v| v.as_u64()), Some(99));
    assert_eq!(sponsor.get("amount").and_then(|v| v.as_u64()), Some(12_345));
    assert_eq!(sponsor.get("base").and_then(|v| v.as_u64()), Some(10_000));
    assert!(
        obj.get("owner_layer").is_some_and(|v| v.is_null()),
        "self-deposit stub leaves owner_layer null"
    );
    assert_eq!(obj.get("finalized").and_then(|v| v.as_bool()), Some(false));
}

#[tokio::test]
async fn current_deposit_price_returns_balance() {
    let stub = Stub::default();
    let request = r#"{"jsonrpc":"2.0","method":"midds_musicalWorks_currentDepositPrice",
                       "params":[150, null],"id":0}"#;
    let resp = call(stub, request).await;
    let result = resp.get("result").expect("result");
    assert_eq!(result.as_u64(), Some(160));
}

#[tokio::test]
async fn weekly_gauge_endpoints_dispatch() {
    for (method, expected) in [
        ("midds_musicalWorks_weeklyTarget", 200_000u64),
        ("midds_musicalWorks_weeklyActual", 0),
    ] {
        let stub = Stub::default();
        let request = format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":[null],"id":0}}"#);
        let resp = call(stub, &request).await;
        assert_eq!(
            resp.get("result").and_then(|v| v.as_u64()),
            Some(expected),
            "method `{method}` returned the wrong value: {resp}"
        );
    }
}

#[tokio::test]
async fn unknown_method_is_rejected() {
    let stub = Stub::default();
    let request =
        r#"{"jsonrpc":"2.0","method":"midds_musicalWorks_doesNotExist","params":[],"id":0}"#;
    let resp = call(stub, request).await;
    assert!(
        resp.get("error").is_some(),
        "unknown method must produce an error response: {resp}",
    );
}

#[tokio::test]
async fn rpc_method_names_are_stable() {
    for method in [
        ("midds_musicalWorks_lookupByIdentifier", "[[0],null]"),
        ("midds_musicalWorks_get", "[0, null]"),
        ("midds_musicalWorks_depositInfo", "[0, null]"),
        ("midds_musicalWorks_currentDepositPrice", "[0, null]"),
        ("midds_musicalWorks_currentMultipliers", "[null]"),
        ("midds_musicalWorks_weeklyTarget", "[null]"),
        ("midds_musicalWorks_weeklyActual", "[null]"),
    ] {
        let (name, params) = method;
        let stub = Stub::default();
        let request = format!(r#"{{"jsonrpc":"2.0","method":"{name}","params":{params},"id":0}}"#);
        let resp = call(stub, &request).await;
        assert!(
            resp.get("result").is_some(),
            "method `{name}` must dispatch to the stub: {resp}"
        );
    }
}
