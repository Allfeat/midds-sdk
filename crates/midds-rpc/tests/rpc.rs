//! JSON-RPC shape tests for the `MiddsRpcApi` trait — section 15.4 of
//! `docs/testing.md`.
//!
//! Spinning up a real `ProvideRuntimeApi` + `HeaderBackend` test harness would
//! drag in `substrate-test-runtime` and most of `sc-client`, none of which the
//! SDK depends on. The pragmatic alternative — and what `docs/testing.md`
//! §15.4 actually asks for — is a hand-written stub that implements the
//! generated `MiddsRpcApiServer` trait directly. The shape of the JSON
//! response is determined by the `#[rpc(server)]` macro, so the stub
//! exercises the same wire format the production handler emits.
//!
//! The cardinal assertion: the lookup family returns JSON `null` (or an
//! empty list, for the multi-claim case) when the queried entry is absent —
//! never a JSON-RPC error object. Front-ends rely on this, and a regression
//! would either crash their happy path or surface user-hostile error pop-ups.

use std::{collections::BTreeMap, sync::Mutex};

use jsonrpsee::core::RpcResult;
use midds_fixtures::musical_work::MusicalWorkBuilder;
use midds_rpc::MiddsRpcApiServer;
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
    deposit_info: BTreeMap<MiddsId, (AccountId, Balance, Balance, bool)>,
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
        s.claims.entry(work.identifier()).or_default().push(id);
        s.items.insert(id, work);
        s.deposit_info
            .insert(id, (depositor, bond, base_bond, finalized));
    }
}

impl MiddsRpcApiServer<BlockHash, Iswc, MusicalWork, AccountId, Balance> for Stub {
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
    ) -> RpcResult<Option<(AccountId, Balance, Balance, bool)>> {
        Ok(self
            .state
            .lock()
            .expect("stub state")
            .deposit_info
            .get(&id)
            .copied())
    }

    fn current_deposit_price(&self, size: u32, _at: Option<BlockHash>) -> RpcResult<Balance> {
        // Stub formula — matches the unit-multiplier mock so tests can still
        // assert on a deterministic number.
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

// -----------------------------------------------------------------------------
// `lookup_by_identifier`
// -----------------------------------------------------------------------------

#[tokio::test]
async fn lookup_missing_identifier_returns_empty_array() {
    let stub = Stub::default();
    let iswc = midds_fixtures::identifiers::iswc_for_index(99_999).to_vec();
    let request = format!(
        r#"{{"jsonrpc":"2.0","method":"midds_lookupByIdentifier",
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
        r#"{{"jsonrpc":"2.0","method":"midds_lookupByIdentifier",
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
    // Multi-claim: same ISWC, two different `MiddsId`s. We insert twice
    // through the stub, which lists the identifier under both ids — exactly
    // the on-chain behaviour `IdentifierClaims` produces.
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
        r#"{{"jsonrpc":"2.0","method":"midds_lookupByIdentifier",
             "params":[{bytes:?}, null],"id":0}}"#
    );
    let resp = call(stub, &request).await;
    let result = resp.get("result").expect("result present");
    let arr = result.as_array().expect("array");
    // Both `work` and `other` come out of the builder with the same default
    // ISWC, so the lookup surfaces both ids.
    assert_eq!(arr.len(), 2);
    let mut ids: Vec<u64> = arr.iter().map(|v| v.as_u64().expect("u64")).collect();
    ids.sort();
    assert_eq!(ids, vec![7, 11]);
}

// -----------------------------------------------------------------------------
// `get`
// -----------------------------------------------------------------------------

#[tokio::test]
async fn get_missing_id_returns_json_null() {
    let stub = Stub::default();
    let request = r#"{"jsonrpc":"2.0","method":"midds_get","params":[42, null],"id":0}"#;

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

    let request = r#"{"jsonrpc":"2.0","method":"midds_get","params":[11, null],"id":0}"#;
    let resp = call(stub, request).await;
    let result = resp.get("result").expect("result present");
    assert!(!result.is_null(), "present id must not return null: {resp}");

    let roundtripped: MusicalWork =
        serde_json::from_value(result.clone()).expect("deserialize work");
    assert_eq!(roundtripped, work);
}

// -----------------------------------------------------------------------------
// `deposit_info`
// -----------------------------------------------------------------------------

#[tokio::test]
async fn deposit_info_missing_id_returns_json_null() {
    let stub = Stub::default();
    let request = r#"{"jsonrpc":"2.0","method":"midds_depositInfo","params":[5, null],"id":0}"#;

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

    let request = r#"{"jsonrpc":"2.0","method":"midds_depositInfo","params":[3, null],"id":0}"#;
    let resp = call(stub, request).await;
    let result = resp.get("result").expect("result present");
    let arr = result.as_array().expect("tuple serializes as JSON array");
    // 4-tuple: depositor, total_held, base_bond, finalized.
    assert_eq!(arr.len(), 4, "tuple must have exactly 4 components");
    assert_eq!(arr[0].as_u64(), Some(99), "account id");
    assert_eq!(arr[1].as_u64(), Some(12_345), "total held");
    assert_eq!(arr[2].as_u64(), Some(10_000), "base bond");
    assert_eq!(arr[3].as_bool(), Some(false), "finalized");
}

// -----------------------------------------------------------------------------
// pricing
// -----------------------------------------------------------------------------

#[tokio::test]
async fn current_deposit_price_returns_balance() {
    let stub = Stub::default();
    let request = r#"{"jsonrpc":"2.0","method":"midds_currentDepositPrice",
                       "params":[150, null],"id":0}"#;
    let resp = call(stub, request).await;
    let result = resp.get("result").expect("result");
    assert_eq!(result.as_u64(), Some(160));
}

#[tokio::test]
async fn weekly_gauge_endpoints_dispatch() {
    for (method, expected) in [
        ("midds_weeklyTarget", 200_000u64),
        ("midds_weeklyActual", 0),
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

// -----------------------------------------------------------------------------
// Method-name regression — `#[rpc(server)]` hardcodes the names that the
// integrating node hands to clients. A rename would silently break every
// existing front-end, so pin every method here.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn unknown_method_is_rejected() {
    let stub = Stub::default();
    let request = r#"{"jsonrpc":"2.0","method":"midds_doesNotExist","params":[],"id":0}"#;
    let resp = call(stub, request).await;
    assert!(
        resp.get("error").is_some(),
        "unknown method must produce an error response: {resp}",
    );
}

#[tokio::test]
async fn rpc_method_names_are_stable() {
    for method in [
        ("midds_lookupByIdentifier", "[[0],null]"),
        ("midds_get", "[0, null]"),
        ("midds_depositInfo", "[0, null]"),
        ("midds_currentDepositPrice", "[0, null]"),
        ("midds_currentMultipliers", "[null]"),
        ("midds_weeklyTarget", "[null]"),
        ("midds_weeklyActual", "[null]"),
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
