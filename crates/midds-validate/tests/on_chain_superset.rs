//! On-chain ⊆ off-chain — section 15.3 of `docs/testing.md`.
//!
//! Statement of the invariant: every payload accepted by the on-chain
//! `Midds::validate_format` is *also* accepted by the off-chain validation
//! surface (parsers + warning-only checksums). The reverse is not true:
//! off-chain tolerates separators, lowercase, and surfaces checksum failures
//! as warnings rather than rejections.
//!
//! Concretely the invariants asserted here are:
//!
//! 1. The canonical identifier extracted from any `arb_musical_work()` payload
//!    parses cleanly through the tolerant parser and round-trips to itself.
//!    (Canonical bytes are the parsers' "narrowest" valid input.)
//!
//! 2. Every IPI / ISNI / ISWC produced by `midds-fixtures` is checksum-clean,
//!    so on-chain validation never excludes a checksum-correct payload.
//!
//! 3. For every Medley / Mashup / Adaptation source-ref ISWC carried by an
//!    on-chain-valid payload, the same parser round-trip and checksum
//!    invariants hold. Catches the case where derived-variant ISWCs
//!    accidentally take a different validation path.

use midds_fixtures::musical_work::strategy::arb_musical_work;
use midds_traits::Midds as _;
use midds_types::{CreatorId, MusicalWork, WorkType};
use midds_validate::{
    CheckResult, parse_ipi, parse_isni, parse_iswc, verify_ipi_checksum, verify_isni_checksum,
    verify_iswc_checksum,
};
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

/// Canonical bytes parse without normalisation: ASCII text identical to the
/// stored bound is the parsers' narrowest valid input.
fn assert_iswc_round_trips(bytes: &[u8]) -> Result<(), TestCaseError> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| TestCaseError::fail(format!("ISWC not utf-8: {e}")))?;
    let parsed = parse_iswc(s)
        .map_err(|e| TestCaseError::fail(format!("canonical ISWC `{s}` rejected: {e}")))?;
    prop_assert_eq!(parsed.as_slice(), bytes);
    prop_assert_eq!(verify_iswc_checksum(&parsed), CheckResult::Pass);
    Ok(())
}

proptest! {
    #![proptest_config(proptest_config())]

    /// On-chain valid → off-chain ISWC parser accepts the canonical bytes
    /// and the warning-only checksum verifier returns `Pass`.
    #[test]
    fn on_chain_iswc_round_trips_off_chain(work in arb_musical_work()) {
        prop_assume!(work.validate_format().is_ok());

        let canonical = work.identifier();
        assert_iswc_round_trips(canonical.as_slice())?;
    }

    /// Every creator identifier carried by an on-chain-valid payload is also
    /// off-chain canonical — the parser accepts it as-is and the warning-only
    /// checksum verifier returns `Pass` (fixture identifiers are
    /// checksum-correct by construction).
    #[test]
    fn on_chain_creator_ids_round_trip_off_chain(work in arb_musical_work()) {
        prop_assume!(work.validate_format().is_ok());

        let MusicalWork::V1(v1) = work;
        for creator in &v1.creators {
            match &creator.id {
                CreatorId::Ipi(ipi) => {
                    let s = std::str::from_utf8(ipi.as_slice())
                        .map_err(|e| TestCaseError::fail(format!("IPI not utf-8: {e}")))?;
                    let parsed = parse_ipi(s)
                        .map_err(|e| TestCaseError::fail(
                            format!("canonical IPI `{s}` rejected: {e}")
                        ))?;
                    prop_assert_eq!(parsed.as_slice(), ipi.as_slice());
                    prop_assert_eq!(verify_ipi_checksum(&parsed), CheckResult::Pass);
                }
                CreatorId::Isni(isni) => {
                    let s = std::str::from_utf8(isni.as_slice())
                        .map_err(|e| TestCaseError::fail(format!("ISNI not utf-8: {e}")))?;
                    let parsed = parse_isni(s)
                        .map_err(|e| TestCaseError::fail(
                            format!("canonical ISNI `{s}` rejected: {e}")
                        ))?;
                    prop_assert_eq!(parsed.as_slice(), isni.as_slice());
                    prop_assert_eq!(verify_isni_checksum(&parsed), CheckResult::Pass);
                }
            }
        }
    }

    /// Source-reference ISWCs carried by Medley / Mashup / Adaptation must
    /// satisfy the same off-chain invariants as the canonical ISWC. Pinned
    /// separately so a future variant whose refs take a different validation
    /// path doesn't silently slip through.
    #[test]
    fn on_chain_work_type_refs_round_trip_off_chain(work in arb_musical_work()) {
        prop_assume!(work.validate_format().is_ok());

        let MusicalWork::V1(v1) = work;
        match &v1.work_type {
            WorkType::Original => {}
            WorkType::Medley(refs) | WorkType::Mashup(refs) => {
                for r in refs {
                    assert_iswc_round_trips(r.as_slice())?;
                }
            }
            WorkType::Adaptation(r) => {
                assert_iswc_round_trips(r.as_slice())?;
            }
        }
    }
}
