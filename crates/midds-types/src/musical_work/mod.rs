pub mod v1;

pub use v1::{
    CATALOG_NUMBER_MAX_LEN, CREATORS_MAX, CatalogNumber, ClassicalInfo, Creator, CreatorId,
    CreatorRole, Creators, Mode, MusicalKey, MusicalWorkV1, OPUS_MAX_LEN, Opus, PitchClass,
    TITLE_MAX_LEN, Title, WORK_REFERENCES_MAX, WorkReferences, WorkType,
};

use midds_traits::{Iswc, Midds, MiddsFormatError};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Top-level versioned `MusicalWork`. New on-chain versions are added as new
/// enum variants and migrated explicitly via `OnRuntimeUpgrade`.
///
/// JSON shape: internally tagged on a `"version"` field. `V1` serialises as
/// `{"version": "v1", "iswc": "T…", "title": "…", …}` (flat). When `V2` is
/// added, existing consumers can dispatch on the tag without re-parsing.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "version", rename_all = "lowercase"))]
pub enum MusicalWork {
    V1(MusicalWorkV1),
}

impl Midds for MusicalWork {
    const KIND: &'static str = "MusicalWork";

    type Identifier = Iswc;

    fn identifier(&self) -> &Iswc {
        match self {
            Self::V1(v) => &v.iswc,
        }
    }

    fn validate_format(&self) -> Result<(), MiddsFormatError> {
        match self {
            Self::V1(v) => v.validate_format(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod tests {
    use super::*;
    use crate::language::Language;
    use bounded_collections::BoundedVec;

    fn bv<const N: u32>(s: &[u8]) -> midds_traits::MiddsString<N> {
        BoundedVec::try_from(s.to_vec()).expect("bounded vec build")
    }

    fn ipi_creator(ipi: &[u8]) -> Creator {
        Creator {
            role: CreatorRole::Composer,
            id: CreatorId::Ipi(bv::<11>(ipi)),
        }
    }

    fn isni_creator(isni: &[u8]) -> Creator {
        Creator {
            role: CreatorRole::Author,
            id: CreatorId::Isni(bv::<16>(isni)),
        }
    }

    fn sample_v1() -> MusicalWorkV1 {
        MusicalWorkV1 {
            iswc: bv(b"T1234567890"),
            title: bv(b"My Work Title"),
            creation_year: 2024,
            instrumental: false,
            language: Some(Language::En),
            bpm: Some(120),
            key: Some(MusicalKey {
                pitch: PitchClass::C,
                mode: Mode::Major,
            }),
            work_type: WorkType::Original,
            creators: BoundedVec::try_from(vec![ipi_creator(b"123456789")]).unwrap(),
            classical_info: None,
            offchain_extension: None,
        }
    }

    #[test]
    fn roundtrip_encode_decode() {
        let original = MusicalWork::V1(sample_v1());
        let encoded = original.encode();
        let decoded = MusicalWork::decode(&mut &encoded[..]).expect("decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn identifier_returns_iswc() {
        let w = MusicalWork::V1(sample_v1());
        assert_eq!(w.identifier(), &bv::<11>(b"T1234567890"));
    }

    #[test]
    fn validate_pass_with_offchain_extension() {
        let mut v = sample_v1();
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));
        assert!(MusicalWork::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_pass_with_isni_creator() {
        let mut v = sample_v1();
        v.creators = BoundedVec::try_from(vec![isni_creator(b"0000000121032683")]).unwrap();
        assert!(MusicalWork::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_pass_with_classical_info() {
        let mut v = sample_v1();
        v.classical_info = Some(ClassicalInfo {
            opus: Some(bv(b"Op. 27 No. 2")),
            catalog_number: Some(bv(b"K. 545")),
            number_of_voices: Some(4),
        });
        assert!(MusicalWork::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_fails_invalid_iswc() {
        let mut v = sample_v1();
        v.iswc = bv(b"X1234567890");
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidIdentifierStructure),
        );
    }

    #[test]
    fn validate_fails_empty_title() {
        let mut v = sample_v1();
        v.title = bv(b"");
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_empty_creators() {
        let mut v = sample_v1();
        v.creators = BoundedVec::default();
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_invalid_creator_ipi() {
        let mut v = sample_v1();
        v.creators = BoundedVec::try_from(vec![ipi_creator(b"123A56789")]).unwrap();
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_invalid_creator_isni() {
        let mut v = sample_v1();
        v.creators = BoundedVec::try_from(vec![isni_creator(b"00000001A1032683")]).unwrap();
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_empty_opus() {
        let mut v = sample_v1();
        v.classical_info = Some(ClassicalInfo {
            opus: Some(bv(b"")),
            catalog_number: None,
            number_of_voices: None,
        });
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_empty_offchain_extension() {
        let mut v = sample_v1();
        v.offchain_extension = Some(bv(b""));
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_pass_medley_with_refs() {
        let mut v = sample_v1();
        v.work_type = WorkType::Medley(
            BoundedVec::try_from(vec![bv::<11>(b"T0345246801"), bv::<11>(b"T9876543210")]).unwrap(),
        );
        assert!(MusicalWork::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_pass_mashup_with_refs() {
        let mut v = sample_v1();
        // Mashup requires >= 2 source works (see docs/validation.md §4).
        v.work_type = WorkType::Mashup(
            BoundedVec::try_from(vec![bv::<11>(b"T0345246801"), bv::<11>(b"T9876543210")]).unwrap(),
        );
        assert!(MusicalWork::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_pass_adaptation_with_ref() {
        let mut v = sample_v1();
        v.work_type = WorkType::Adaptation(bv::<11>(b"T0345246801"));
        assert!(MusicalWork::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_fails_medley_fewer_than_two_refs() {
        // 0 refs and 1 ref both violate the >= 2 cardinality rule, surfacing
        // as OutOfBounds (min-cardinality), checked before per-ref structure.
        let mut empty = sample_v1();
        empty.work_type = WorkType::Medley(BoundedVec::default());
        assert_eq!(
            MusicalWork::V1(empty).validate_format(),
            Err(MiddsFormatError::OutOfBounds),
        );

        let mut single = sample_v1();
        single.work_type =
            WorkType::Medley(BoundedVec::try_from(vec![bv::<11>(b"T0345246801")]).unwrap());
        assert_eq!(
            MusicalWork::V1(single).validate_format(),
            Err(MiddsFormatError::OutOfBounds),
        );
    }

    #[test]
    fn validate_fails_invalid_medley_ref() {
        let mut v = sample_v1();
        // >= 2 refs so the cardinality check passes and the per-ref ISWC
        // structure check is reached; the second ref is malformed.
        v.work_type = WorkType::Medley(
            BoundedVec::try_from(vec![bv::<11>(b"T0345246801"), bv::<11>(b"X1234567890")]).unwrap(),
        );
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidIdentifierStructure),
        );
    }

    #[test]
    fn validate_fails_invalid_adaptation_ref() {
        let mut v = sample_v1();
        v.work_type = WorkType::Adaptation(bv::<11>(b"X1234567890"));
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidIdentifierStructure),
        );
    }

    #[test]
    fn max_encoded_len_is_finite() {
        let max = <MusicalWork as MaxEncodedLen>::max_encoded_len();
        assert!(max > 0);
        assert!(max < 4096, "MusicalWork max_encoded_len = {max}");
    }
}

#[cfg(all(test, feature = "serde"))]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod json_tests {
    //! Snapshot tests that pin the JSON wire format. These act as the
    //! cross-service schema contract — every producer (pallet RPC, explorer
    //! backend, SaaS API) and every consumer (frontend, indexer) must match
    //! the shapes asserted here. A field rename or shape change should fail
    //! one of these tests on purpose, forcing a deliberate version bump.

    use super::*;
    use crate::language::Language;
    use bounded_collections::BoundedVec;

    fn bv<const N: u32>(s: &[u8]) -> midds_traits::MiddsString<N> {
        BoundedVec::try_from(s.to_vec()).expect("bounded vec build")
    }

    fn minimal_v1() -> MusicalWorkV1 {
        MusicalWorkV1 {
            iswc: bv(b"T1234567890"),
            title: bv(b"My Work Title"),
            creation_year: 2024,
            instrumental: false,
            language: Some(Language::En),
            bpm: Some(120),
            key: Some(MusicalKey {
                pitch: PitchClass::C,
                mode: Mode::Major,
            }),
            work_type: WorkType::Original,
            creators: BoundedVec::try_from(vec![Creator {
                role: CreatorRole::Composer,
                id: CreatorId::Ipi(bv(b"123456789")),
            }])
            .unwrap(),
            classical_info: None,
            offchain_extension: None,
        }
    }

    #[test]
    fn shape_minimal() {
        let json = serde_json::to_value(MusicalWork::V1(minimal_v1())).unwrap();

        // Internal version tag, fields flat alongside.
        assert_eq!(json["version"], "v1");
        assert_eq!(json["iswc"], "T1234567890");
        assert_eq!(json["title"], "My Work Title");
        assert_eq!(json["creation_year"], 2024);
        assert_eq!(json["instrumental"], false);
        assert_eq!(json["language"], "en");
        assert_eq!(json["bpm"], 120);
        assert_eq!(json["key"]["pitch"], "C");
        assert_eq!(json["key"]["mode"], "Major");
        // Unit variant of WorkType is a bare string.
        assert_eq!(json["work_type"], "Original");
        // Creators is an array of objects with the canonical role + id shape.
        assert_eq!(json["creators"][0]["role"], "Composer");
        assert_eq!(json["creators"][0]["id"]["Ipi"], "123456789");
        assert!(json["classical_info"].is_null());
        assert!(json["offchain_extension"].is_null());
    }

    #[test]
    fn roundtrip_minimal() {
        let original = MusicalWork::V1(minimal_v1());
        let raw = serde_json::to_string(&original).unwrap();
        let restored: MusicalWork = serde_json::from_str(&raw).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn shape_full() {
        let mut v = minimal_v1();
        v.work_type = WorkType::Medley(
            BoundedVec::try_from(vec![bv::<11>(b"T0345246801"), bv::<11>(b"T9876543210")]).unwrap(),
        );
        v.classical_info = Some(ClassicalInfo {
            opus: Some(bv(b"Op. 27 No. 2")),
            catalog_number: Some(bv(b"K. 545")),
            number_of_voices: Some(4),
        });
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));
        v.creators = BoundedVec::try_from(vec![
            Creator {
                role: CreatorRole::Author,
                id: CreatorId::Isni(bv(b"0000000121032683")),
            },
            Creator {
                role: CreatorRole::Composer,
                id: CreatorId::Ipi(bv(b"123456789")),
            },
        ])
        .unwrap();

        let json = serde_json::to_value(MusicalWork::V1(v)).unwrap();
        // Tuple variants of WorkType use external tagging; ascii_vec yields strings.
        assert_eq!(
            json["work_type"]["Medley"],
            serde_json::json!(["T0345246801", "T9876543210"])
        );
        // Nested ASCII strings round-trip through ascii_opt.
        assert_eq!(json["classical_info"]["opus"], "Op. 27 No. 2");
        assert_eq!(json["classical_info"]["catalog_number"], "K. 545");
        assert_eq!(json["classical_info"]["number_of_voices"], 4);
        assert_eq!(json["offchain_extension"], "bafkreigh2akiscaildc");
        assert_eq!(json["creators"][0]["id"]["Isni"], "0000000121032683");
        assert_eq!(json["creators"][1]["id"]["Ipi"], "123456789");
    }

    #[test]
    fn roundtrip_full() {
        let mut v = minimal_v1();
        v.work_type = WorkType::Adaptation(bv(b"T0345246801"));
        v.classical_info = Some(ClassicalInfo {
            opus: Some(bv(b"Op. 27 No. 2")),
            catalog_number: Some(bv(b"K. 545")),
            number_of_voices: Some(4),
        });
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));

        let original = MusicalWork::V1(v);
        let raw = serde_json::to_string(&original).unwrap();
        let restored: MusicalWork = serde_json::from_str(&raw).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn rejects_oversized_iswc_at_decode() {
        // 14 ASCII chars cannot fit in MiddsString<11>; deserialize must fail
        // before any further validation runs.
        let bad = r#"{
            "version":"v1",
            "iswc":"T1234567890123",
            "title":"x",
            "creation_year":2024,
            "instrumental":false,
            "work_type":"Original",
            "creators":[{"role":"Composer","id":{"Ipi":"123456789"}}]
        }"#;
        let r: Result<MusicalWork, _> = serde_json::from_str(bad);
        assert!(r.is_err(), "expected length-bound rejection");
    }

    #[test]
    fn rejects_unknown_version_tag() {
        let bad = r#"{
            "version":"v2",
            "iswc":"T1234567890",
            "title":"x",
            "creation_year":2024,
            "instrumental":false,
            "work_type":"Original",
            "creators":[{"role":"Composer","id":{"Ipi":"123456789"}}]
        }"#;
        let r: Result<MusicalWork, _> = serde_json::from_str(bad);
        assert!(r.is_err(), "unknown version must be rejected");
    }
}
