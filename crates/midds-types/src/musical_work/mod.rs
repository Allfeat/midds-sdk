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
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MusicalWork {
    V1(MusicalWorkV1),
}

impl Midds for MusicalWork {
    type Identifier = Iswc;

    fn identifier(&self) -> Iswc {
        match self {
            Self::V1(v) => v.iswc.clone(),
        }
    }

    fn validate_format(&self) -> Result<(), MiddsFormatError> {
        match self {
            Self::V1(v) => v.validate_format(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Language;
    use frame_support::BoundedVec;

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
        assert_eq!(w.identifier(), bv::<11>(b"T1234567890"));
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
        v.work_type =
            WorkType::Mashup(BoundedVec::try_from(vec![bv::<11>(b"T0345246801")]).unwrap());
        assert!(MusicalWork::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_pass_adaptation_with_ref() {
        let mut v = sample_v1();
        v.work_type = WorkType::Adaptation(bv::<11>(b"T0345246801"));
        assert!(MusicalWork::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_fails_empty_medley_refs() {
        let mut v = sample_v1();
        v.work_type = WorkType::Medley(BoundedVec::default());
        assert_eq!(
            MusicalWork::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_invalid_medley_ref() {
        let mut v = sample_v1();
        v.work_type =
            WorkType::Medley(BoundedVec::try_from(vec![bv::<11>(b"X1234567890")]).unwrap());
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
