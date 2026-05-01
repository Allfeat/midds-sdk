//! Borderline payloads called out by `docs/testing.md` §12.
//!
//! Each function returns a *deterministic* payload — no RNG — because these
//! are the worst-cases tests want to pin down byte-for-byte. The "invalid"
//! constructors return values whose `validate_format` is expected to fail;
//! tests should assert the matching `MiddsFormatError`.

use frame_support::BoundedVec;
use midds_traits::{Iswc, MiddsFormatError};
use midds_types::{
    CATALOG_NUMBER_MAX_LEN, CREATORS_MAX, ClassicalInfo, Creator, CreatorId, CreatorRole, Language,
    Mode, MusicalKey, MusicalWork, MusicalWorkV1, OPUS_MAX_LEN, PitchClass, TITLE_MAX_LEN,
    WORK_REFERENCES_MAX, WorkType,
};

use crate::identifiers::{ipi_from_stem, isni_from_body, iswc_for_index, iswc_from_work_code};

// -----------------------------------------------------------------------------
// Size extremes
// -----------------------------------------------------------------------------

/// Minimum-size valid `MusicalWork`: smallest non-empty title, single
/// 9-digit-IPI creator, no optional fields, `Original` work type.
pub fn min_size_musical_work() -> MusicalWork {
    let v1 = MusicalWorkV1 {
        iswc: iswc_for_index(0),
        title: BoundedVec::try_from(b"x".to_vec()).expect("1 byte"),
        creation_year: 1900,
        instrumental: true,
        language: None,
        bpm: None,
        key: None,
        work_type: WorkType::Original,
        creators: BoundedVec::try_from(vec![Creator {
            role: CreatorRole::Composer,
            id: CreatorId::Ipi(ipi_from_stem(0, 9)),
        }])
        .expect("1 creator"),
        classical_info: None,
        offchain_extension: None,
    };
    MusicalWork::V1(v1)
}

/// Maximum-size valid `MusicalWork`: every bounded field at capacity, every
/// optional field present, ISNI creators (the larger `CreatorId` variant) and
/// `Medley` work type (the only one that carries refs). Useful as a stable
/// worst-case baseline for fee benchmarks and SCALE-encoding tests.
pub fn max_size_musical_work() -> MusicalWork {
    let title = BoundedVec::try_from(vec![b'x'; TITLE_MAX_LEN as usize]).expect("title at bound");
    let creators: Vec<Creator> = (0..CREATORS_MAX)
        .map(|i| {
            // Distinct ISNI bodies per creator so the SCALE encoding doesn't
            // accidentally compress identical entries.
            let body: [u8; 15] = core::array::from_fn(|j| ((i + j as u32 + 1) % 10) as u8);
            Creator {
                role: CreatorRole::Composer,
                id: CreatorId::Isni(isni_from_body(body)),
            }
        })
        .collect();
    let creators = BoundedVec::try_from(creators).expect("creators at bound");
    let opus = BoundedVec::try_from(vec![b'o'; OPUS_MAX_LEN as usize]).expect("opus at bound");
    let catalog = BoundedVec::try_from(vec![b'c'; CATALOG_NUMBER_MAX_LEN as usize])
        .expect("catalog at bound");
    let refs: Vec<Iswc> = (0..WORK_REFERENCES_MAX)
        .map(|i| iswc_from_work_code(i + 1))
        .collect();
    let work_type = WorkType::Medley(BoundedVec::try_from(refs).expect("refs at bound"));
    let offchain = BoundedVec::try_from(vec![b'h'; 64]).expect("offchain at 64-byte bound");
    let v1 = MusicalWorkV1 {
        iswc: iswc_from_work_code(0),
        title,
        creation_year: u16::MAX,
        instrumental: false,
        language: Some(Language::En),
        bpm: Some(u16::MAX),
        key: Some(MusicalKey {
            pitch: PitchClass::C,
            mode: Mode::Major,
        }),
        work_type,
        creators,
        classical_info: Some(ClassicalInfo {
            opus: Some(opus),
            catalog_number: Some(catalog),
            number_of_voices: Some(u16::MAX),
        }),
        offchain_extension: Some(offchain),
    };
    MusicalWork::V1(v1)
}

// -----------------------------------------------------------------------------
// Invalid payloads — paired with the expected error
// -----------------------------------------------------------------------------

/// ISWC with `T` replaced by `X`. Triggers `InvalidIdentifierStructure`.
pub fn invalid_iswc_wrong_prefix() -> (MusicalWork, MiddsFormatError) {
    let mut bytes = iswc_for_index(0).to_vec();
    bytes[0] = b'X';
    let MusicalWork::V1(mut v1) = min_size_musical_work();
    v1.iswc = BoundedVec::try_from(bytes).expect("11 bytes");
    (
        MusicalWork::V1(v1),
        MiddsFormatError::InvalidIdentifierStructure,
    )
}

/// ISWC with one digit replaced by `A`. Triggers `InvalidCharset`.
pub fn invalid_iswc_bad_charset() -> (MusicalWork, MiddsFormatError) {
    let mut bytes = iswc_for_index(0).to_vec();
    bytes[5] = b'A';
    let MusicalWork::V1(mut v1) = min_size_musical_work();
    v1.iswc = BoundedVec::try_from(bytes).expect("11 bytes");
    (MusicalWork::V1(v1), MiddsFormatError::InvalidCharset)
}

/// `MusicalWork` with empty title. Triggers `EmptyMandatoryField`.
pub fn invalid_empty_title() -> (MusicalWork, MiddsFormatError) {
    let MusicalWork::V1(mut v1) = min_size_musical_work();
    v1.title = BoundedVec::default();
    (MusicalWork::V1(v1), MiddsFormatError::EmptyMandatoryField)
}

/// `MusicalWork` with empty creators list. Triggers `EmptyMandatoryField`.
pub fn invalid_empty_creators() -> (MusicalWork, MiddsFormatError) {
    let MusicalWork::V1(mut v1) = min_size_musical_work();
    v1.creators = BoundedVec::default();
    (MusicalWork::V1(v1), MiddsFormatError::EmptyMandatoryField)
}

/// `MusicalWork` with `WorkType::Medley(empty refs)`. Triggers `EmptyMandatoryField`.
pub fn invalid_empty_medley_refs() -> (MusicalWork, MiddsFormatError) {
    let MusicalWork::V1(mut v1) = min_size_musical_work();
    v1.work_type = WorkType::Medley(BoundedVec::default());
    (MusicalWork::V1(v1), MiddsFormatError::EmptyMandatoryField)
}

#[cfg(test)]
mod tests {
    use super::*;
    use midds_traits::Midds as _;
    use parity_scale_codec::MaxEncodedLen;

    #[test]
    fn min_size_validates() {
        min_size_musical_work()
            .validate_format()
            .expect("min size validates");
    }

    #[test]
    fn max_size_validates_and_saturates_bound() {
        let work = max_size_musical_work();
        work.validate_format().expect("max size validates");
        let max = <MusicalWork as MaxEncodedLen>::max_encoded_len();
        assert_eq!(parity_scale_codec::Encode::encoded_size(&work), max);
    }

    #[test]
    fn min_smaller_than_max_in_encoded_bytes() {
        use parity_scale_codec::Encode;
        let min = min_size_musical_work().encoded_size();
        let max = max_size_musical_work().encoded_size();
        assert!(min < max);
    }

    #[test]
    fn invalid_constructors_match_expected_error() {
        for ctor in [
            invalid_iswc_wrong_prefix as fn() -> _,
            invalid_iswc_bad_charset,
            invalid_empty_title,
            invalid_empty_creators,
            invalid_empty_medley_refs,
        ] {
            let (work, expected) = ctor();
            let err = work.validate_format().expect_err("payload must fail");
            assert_eq!(err, expected);
        }
    }
}
