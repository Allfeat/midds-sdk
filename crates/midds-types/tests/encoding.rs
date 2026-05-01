//! Property-based encoding invariants for `MusicalWork` — section 15.2 of
//! `docs/testing.md`.
//!
//! Three invariants on every `arb_musical_work()` payload:
//!
//! 1. SCALE encode → decode → bit-identical roundtrip.
//! 2. `encoded_size() <= max_encoded_len()` (the bound this pallet relies on
//!    for fee/bond calibration).
//! 3. Under `--features serde` (active here via `midds-fixtures/corpus`), JSON
//!    encode → decode → bit-identical roundtrip.
//!
//! `proptest_cases` defaults to 64 — same low budget the strategy module uses
//! internally, since each case round-trips a payload that already passes
//! `validate_format`. Override via `PROPTEST_CASES=<n>` for nightly.

use midds_fixtures::musical_work::strategy::{arb_musical_work, arb_musical_work_max_size};
use midds_types::MusicalWork;
use parity_scale_codec::{Decode, Encode, MaxEncodedLen};
use proptest::prelude::*;

fn proptest_config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);
    ProptestConfig {
        cases,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(proptest_config())]

    /// SCALE codec roundtrip: encoding then decoding must yield a bit-identical
    /// `MusicalWork`. Backstops every change to the type tree against an
    /// accidental encoding regression.
    #[test]
    fn scale_roundtrip(work in arb_musical_work()) {
        let encoded = work.encode();
        let decoded = MusicalWork::decode(&mut &encoded[..])
            .expect("SCALE-decode must succeed on freshly-encoded payload");
        prop_assert_eq!(decoded, work);
    }

    /// Encoded size must never exceed `MaxEncodedLen` — the bound the pallet
    /// uses to calibrate `DepositPerByte` and to size benchmarks. A regression
    /// here would let a payload silently drift past the size the runtime
    /// prepared for.
    #[test]
    fn encoded_size_within_max_encoded_len(work in arb_musical_work()) {
        let max = <MusicalWork as MaxEncodedLen>::max_encoded_len();
        prop_assert!(work.encoded_size() <= max);
    }

    /// JSON roundtrip via serde: serialize and deserialize must yield a
    /// bit-identical `MusicalWork`. Guards the off-chain wire (CLI, RPC,
    /// fixtures) which depends on lossless JSON.
    #[test]
    fn serde_json_roundtrip(work in arb_musical_work()) {
        let json = serde_json::to_string(&work)
            .expect("serde-serialize must succeed on a valid payload");
        let decoded: MusicalWork = serde_json::from_str(&json)
            .expect("serde-deserialize must succeed on freshly-serialized payload");
        prop_assert_eq!(decoded, work);
    }

    /// Worst-case payloads (every bounded field saturated) must still
    /// roundtrip and hit `MaxEncodedLen` exactly. Already covered in the
    /// strategy's own self-test, but re-asserting here pins the invariant
    /// to the encoding crate where the bound is consumed.
    #[test]
    fn max_size_payload_roundtrips_at_bound(work in arb_musical_work_max_size()) {
        let max = <MusicalWork as MaxEncodedLen>::max_encoded_len();
        prop_assert_eq!(work.encoded_size(), max);

        let encoded = work.encode();
        let decoded = MusicalWork::decode(&mut &encoded[..])
            .expect("SCALE-decode must succeed at MaxEncodedLen");
        prop_assert_eq!(decoded, work);
    }
}
