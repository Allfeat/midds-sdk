//! Borderline payloads called out by `docs/testing.md` §12.
//!
//! Each function returns a *deterministic* payload — no RNG — because these
//! are the worst-cases tests want to pin down byte-for-byte. The "invalid"
//! constructors return values whose `validate_format` is expected to fail;
//! tests should assert the matching `MiddsFormatError`.

use frame_support::BoundedVec;
use midds_traits::{Isni, Iswc, MiddsFormatError};
use midds_types::release as rel;
use midds_types::shared::{BPM_MAX, YEAR_MAX};
use midds_types::{
    CATALOG_NUMBER_MAX_LEN, CONTRIBUTORS_MAX, CREATORS_MAX, ClassicalInfo, Country, Creator,
    CreatorRole, CreatorRoles, GENRES_MAX, Genre, Language, Mode, MusicalKey, MusicalWork,
    MusicalWorkV1, OPUS_MAX_LEN, PERFORMERS_MAX, PLACE_MAX_LEN, PRODUCERS_MAX, PartyId,
    PerformerId, PitchClass, Producer, ProductionPlaces, Recording, RecordingRef, RecordingV1,
    RecordingVersion, Release, ReleaseDate, ReleaseFormat, ReleasePackaging, ReleaseStatus,
    ReleaseType, ReleaseV1, SAMPLES_MAX, TITLE_ALIASES_MAX, TITLE_MAX_LEN, WORK_REFERENCES_MAX,
    WorkRef, WorkType,
};

use crate::identifiers::{
    ipi_from_stem, isni_from_body, isrc_for_index, iswc_for_index, iswc_from_work_code,
    upc_for_index,
};

/// Helper: a single-role `CreatorRoles` for the *min-size* path. One role +
/// the compact length prefix gives the smallest non-empty BoundedBTreeSet
/// encoding.
fn single_role(role: CreatorRole) -> CreatorRoles {
    let mut set = CreatorRoles::new();
    set.try_insert(role).expect("1 role within bound");
    set
}

/// Helper: a `CreatorRoles` containing every variant — the worst case for
/// SCALE encoding (CREATOR_ROLES_MAX entries + length prefix). Used by the
/// max-size payload constructors below.
fn all_roles() -> CreatorRoles {
    let mut set = CreatorRoles::new();
    for r in [
        CreatorRole::Author,
        CreatorRole::Composer,
        CreatorRole::Arranger,
        CreatorRole::Adapter,
        CreatorRole::Publisher,
    ] {
        set.try_insert(r).expect("role within CREATOR_ROLES_MAX");
    }
    set
}

/// Minimum-size valid `MusicalWork`: smallest non-empty title, single
/// 9-digit-IPI creator with a single role, no optional fields, `Original`
/// work type.
pub fn min_size_musical_work() -> MusicalWork {
    let v1 = MusicalWorkV1 {
        iswc: iswc_for_index(0),
        title: BoundedVec::try_from(b"x".to_vec()).expect("1 byte"),
        creation_year: None,
        instrumental: true,
        language: None,
        explicit_lyrics: false,
        bpm: None,
        key: None,
        work_type: WorkType::Original,
        samples: BoundedVec::default(),
        creators: BoundedVec::try_from(vec![Creator {
            roles: single_role(CreatorRole::Composer),
            party: PartyId::Ipi(ipi_from_stem(0, 9)),
        }])
        .expect("1 creator"),
        classical_info: None,
        offchain_extension: None,
    };
    MusicalWork::V1(v1)
}

/// Maximum-size valid `MusicalWork`: every bounded field at capacity, every
/// optional field present, every creator carrying `CREATOR_ROLES_MAX` roles
/// and the `PartyId::Both` variant (the largest of the three — full 11-byte
/// IPI + 16-byte ISNI), and a `Medley` work type with the maximum number of
/// source refs. Stable worst-case baseline for fee benchmarks and
/// SCALE-encoding tests.
pub fn max_size_musical_work() -> MusicalWork {
    let title = BoundedVec::try_from(vec![b'x'; TITLE_MAX_LEN as usize]).expect("title at bound");
    let creators: Vec<Creator> = (0..CREATORS_MAX)
        .map(|i| {
            let body: [u8; 15] = core::array::from_fn(|j| ((i + j as u32 + 1) % 10) as u8);
            Creator {
                roles: all_roles(),
                party: PartyId::Both {
                    ipi: ipi_from_stem(u64::from(i) * 1_000_000_007, 11),
                    isni: isni_from_body(body),
                },
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
    // `SAMPLES_MAX` distinct `WorkRef::Iswc` entries (the larger variant — 13
    // bytes vs 9 for the MIDDS-id one) saturate the samples bound. Codes
    // `1..=SAMPLES_MAX` keep every sample distinct and different from the
    // work's own ISWC (code 0).
    let samples: Vec<WorkRef> = (0..SAMPLES_MAX)
        .map(|i| WorkRef::Iswc(iswc_from_work_code(i + 1)))
        .collect();
    let samples = BoundedVec::try_from(samples).expect("samples at bound");
    let offchain = BoundedVec::try_from(vec![b'h'; 64]).expect("offchain at 64-byte bound");
    let v1 = MusicalWorkV1 {
        iswc: iswc_from_work_code(0),
        title,
        creation_year: Some(YEAR_MAX),
        instrumental: false,
        language: Some(Language::En),
        explicit_lyrics: true,
        bpm: Some(BPM_MAX),
        key: Some(MusicalKey {
            pitch: PitchClass::C,
            mode: Mode::Major,
        }),
        work_type,
        samples,
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

/// `MusicalWork` with `WorkType::Medley` carrying fewer than 2 source refs
/// (here: empty). Triggers `OutOfBounds` (Medley / Mashup require >= 2 refs).
pub fn invalid_empty_medley_refs() -> (MusicalWork, MiddsFormatError) {
    let MusicalWork::V1(mut v1) = min_size_musical_work();
    v1.work_type = WorkType::Medley(BoundedVec::default());
    (MusicalWork::V1(v1), MiddsFormatError::OutOfBounds)
}

/// Minimum-size valid `Recording`: smallest non-empty title, an IPI artist,
/// the work referenced by the cheapest `WorkRef::Midds` variant, every
/// collection empty and every optional field absent.
pub fn min_size_recording() -> Recording {
    let v1 = RecordingV1 {
        isrc: isrc_for_index(0),
        title: BoundedVec::try_from(b"x".to_vec()).expect("1 byte"),
        title_aliases: BoundedVec::default(),
        artist: PartyId::Ipi(ipi_from_stem(0, 9)),
        work: WorkRef::Midds(0),
        genres: BoundedVec::default(),
        record_year: None,
        version_type: None,
        performers: BoundedVec::default(),
        producers: BoundedVec::default(),
        duration: None,
        bpm: None,
        key: None,
        places: None,
        contributors: BoundedVec::default(),
        offchain_extension: None,
    };
    Recording::V1(v1)
}

/// Maximum-size valid `Recording`: every bounded field at capacity, every
/// optional field present, `PartyId::Both` for `artist`/`contributors` (the
/// larger identity variant now that IPI+ISNI can be carried together — 30
/// bytes vs ≤ 18 for single-id), `PerformerId::Isni` for `performers` (the
/// larger variant — 17 bytes vs 12 for `Ipn`/`Ipi`), and `WorkRef::Iswc`
/// (larger than the MIDDS id). Stable worst-case baseline for fee benchmarks
/// and SCALE-encoding tests.
pub fn max_size_recording() -> Recording {
    let title = BoundedVec::try_from(vec![b'x'; TITLE_MAX_LEN as usize]).expect("title at bound");
    let aliases: Vec<_> = (0..TITLE_ALIASES_MAX)
        .map(|_| BoundedVec::try_from(vec![b'a'; TITLE_MAX_LEN as usize]).expect("alias at bound"))
        .collect();
    let isni_at = |i: u32| {
        let body: [u8; 15] = core::array::from_fn(|j| ((i + j as u32 + 1) % 10) as u8);
        isni_from_body(body)
    };
    let party_both_at = |i: u32| PartyId::Both {
        ipi: ipi_from_stem(u64::from(i) * 1_000_000_007 + 1, 11),
        isni: isni_at(i),
    };
    let performers: Vec<PerformerId> = (0..PERFORMERS_MAX)
        .map(|i| PerformerId::Isni(isni_at(i)))
        .collect();
    let producers: Vec<Isni> = (0..PRODUCERS_MAX).map(isni_at).collect();
    let contributors: Vec<PartyId> = (0..CONTRIBUTORS_MAX).map(party_both_at).collect();
    let place = BoundedVec::try_from(vec![b'p'; PLACE_MAX_LEN as usize]).expect("place at bound");
    let offchain = BoundedVec::try_from(vec![b'h'; 64]).expect("offchain at 64-byte bound");
    let v1 = RecordingV1 {
        isrc: isrc_for_index(0),
        title,
        title_aliases: BoundedVec::try_from(aliases).expect("aliases at bound"),
        artist: party_both_at(99),
        work: WorkRef::Iswc(iswc_from_work_code(1)),
        genres: BoundedVec::try_from(vec![Genre::Other; GENRES_MAX as usize])
            .expect("genres at bound"),
        record_year: Some(YEAR_MAX),
        version_type: Some(RecordingVersion::Original),
        performers: BoundedVec::try_from(performers).expect("performers at bound"),
        producers: BoundedVec::try_from(producers).expect("producers at bound"),
        duration: Some(u32::MAX),
        bpm: Some(BPM_MAX),
        key: Some(MusicalKey {
            pitch: PitchClass::C,
            mode: Mode::Major,
        }),
        places: Some(ProductionPlaces {
            recording: Some(place.clone()),
            mixing: Some(place.clone()),
            mastering: Some(place),
        }),
        contributors: BoundedVec::try_from(contributors).expect("contributors at bound"),
        offchain_extension: Some(offchain),
    };
    Recording::V1(v1)
}

/// ISRC with a lowercase country code. Triggers `InvalidCharset`.
pub fn invalid_recording_isrc_bad_charset() -> (Recording, MiddsFormatError) {
    let Recording::V1(mut v1) = min_size_recording();
    let mut bytes = isrc_for_index(0).to_vec();
    bytes[0] = b'f';
    v1.isrc = BoundedVec::try_from(bytes).expect("12 bytes");
    (Recording::V1(v1), MiddsFormatError::InvalidCharset)
}

/// ISRC shorter than 12 bytes. Triggers `OutOfBounds`.
pub fn invalid_recording_isrc_wrong_length() -> (Recording, MiddsFormatError) {
    let Recording::V1(mut v1) = min_size_recording();
    v1.isrc = BoundedVec::try_from(b"USRC1760783".to_vec()).expect("11 bytes ≤ 12");
    (Recording::V1(v1), MiddsFormatError::OutOfBounds)
}

/// `Recording` with an empty title. Triggers `EmptyMandatoryField`.
pub fn invalid_recording_empty_title() -> (Recording, MiddsFormatError) {
    let Recording::V1(mut v1) = min_size_recording();
    v1.title = BoundedVec::default();
    (Recording::V1(v1), MiddsFormatError::EmptyMandatoryField)
}

/// `Recording` whose `WorkRef::Iswc` is missing the `T` prefix. Triggers
/// `InvalidIdentifierStructure`.
pub fn invalid_recording_work_iswc_prefix() -> (Recording, MiddsFormatError) {
    let Recording::V1(mut v1) = min_size_recording();
    v1.work = WorkRef::Iswc(BoundedVec::try_from(b"X1234567890".to_vec()).expect("11 bytes"));
    (
        Recording::V1(v1),
        MiddsFormatError::InvalidIdentifierStructure,
    )
}

/// Minimum-size valid `Release`: a 12-digit UPC-A, smallest non-empty title,
/// an IPI artist, a single cheapest `RecordingRef::Midds` track, every
/// collection empty and every optional field absent.
pub fn min_size_release() -> Release {
    let v1 = ReleaseV1 {
        upc: BoundedVec::try_from(b"000000000000".to_vec()).expect("12-byte UPC-A"),
        title: BoundedVec::try_from(b"x".to_vec()).expect("1 byte"),
        title_aliases: BoundedVec::default(),
        artist: PartyId::Ipi(ipi_from_stem(0, 9)),
        tracks: BoundedVec::try_from(vec![RecordingRef::Midds(0)]).expect("one track"),
        producers: BoundedVec::default(),
        status: ReleaseStatus::Official,
        release_date: ReleaseDate {
            year: 0,
            month: 1,
            day: 1,
        },
        country: Country::Us,
        distributor_name: BoundedVec::try_from(b"x".to_vec()).expect("1 byte"),
        release_type: ReleaseType::Album,
        format: ReleaseFormat::Cd,
        packaging: ReleasePackaging::None,
        cover_contributors: BoundedVec::default(),
        offchain_extension: None,
    };
    Release::V1(v1)
}

/// Maximum-size valid `Release`: every bounded field at capacity, every
/// optional field present, `PartyId::Both` for the artist (the larger
/// identity variant now that IPI+ISNI can be carried together) and
/// `RecordingRef::Isrc` everywhere for tracks (the larger variant). Stable
/// worst-case baseline for fee benchmarks and SCALE-encoding tests.
pub fn max_size_release() -> Release {
    let title = BoundedVec::try_from(vec![b'x'; TITLE_MAX_LEN as usize]).expect("title at bound");
    let aliases: Vec<_> = (0..rel::TITLE_ALIASES_MAX)
        .map(|_| BoundedVec::try_from(vec![b'a'; TITLE_MAX_LEN as usize]).expect("alias at bound"))
        .collect();
    let isni_at = |i: u32| {
        let body: [u8; 15] = core::array::from_fn(|j| ((i + j as u32 + 1) % 10) as u8);
        isni_from_body(body)
    };
    let tracks: Vec<RecordingRef> = (0..rel::TRACKS_MAX)
        .map(|i| RecordingRef::Isrc(isrc_for_index(i)))
        .collect();
    let producers: Vec<Producer> = (0..rel::PRODUCERS_MAX)
        .map(|i| Producer {
            isni: isni_at(i),
            catalog_number: BoundedVec::try_from(vec![b'c'; rel::CATALOG_NUMBER_MAX_LEN as usize])
                .expect("catalog at bound"),
        })
        .collect();
    let cover_contributors: Vec<_> = (0..rel::COVER_CONTRIBUTORS_MAX)
        .map(|_| {
            BoundedVec::try_from(vec![b'n'; rel::COVER_CONTRIBUTOR_NAME_MAX_LEN as usize])
                .expect("cover name at bound")
        })
        .collect();
    let offchain = BoundedVec::try_from(vec![b'h'; 64]).expect("offchain at 64-byte bound");
    let v1 = ReleaseV1 {
        upc: upc_for_index(0),
        title,
        title_aliases: BoundedVec::try_from(aliases).expect("aliases at bound"),
        artist: PartyId::Both {
            ipi: ipi_from_stem(99 * 1_000_000_007, 11),
            isni: isni_at(99),
        },
        tracks: BoundedVec::try_from(tracks).expect("tracks at bound"),
        producers: BoundedVec::try_from(producers).expect("producers at bound"),
        status: ReleaseStatus::Official,
        release_date: ReleaseDate {
            year: u16::MAX,
            month: 12,
            day: 31,
        },
        country: Country::Us,
        distributor_name: BoundedVec::try_from(vec![b'd'; rel::DISTRIBUTOR_NAME_MAX_LEN as usize])
            .expect("distributor at bound"),
        release_type: ReleaseType::Album,
        format: ReleaseFormat::Cd,
        packaging: ReleasePackaging::None,
        cover_contributors: BoundedVec::try_from(cover_contributors)
            .expect("cover contributors at bound"),
        offchain_extension: Some(offchain),
    };
    Release::V1(v1)
}

/// UPC with a non-digit byte. Triggers `InvalidCharset`.
pub fn invalid_release_upc_bad_charset() -> (Release, MiddsFormatError) {
    let Release::V1(mut v1) = min_size_release();
    v1.upc = BoundedVec::try_from(b"00000000000X".to_vec()).expect("12 bytes");
    (Release::V1(v1), MiddsFormatError::InvalidCharset)
}

/// UPC of an unsupported length. Triggers `OutOfBounds`.
pub fn invalid_release_upc_wrong_length() -> (Release, MiddsFormatError) {
    let Release::V1(mut v1) = min_size_release();
    v1.upc = BoundedVec::try_from(b"0000000".to_vec()).expect("7 bytes ≤ 13");
    (Release::V1(v1), MiddsFormatError::OutOfBounds)
}

/// `Release` with an empty title. Triggers `EmptyMandatoryField`.
pub fn invalid_release_empty_title() -> (Release, MiddsFormatError) {
    let Release::V1(mut v1) = min_size_release();
    v1.title = BoundedVec::default();
    (Release::V1(v1), MiddsFormatError::EmptyMandatoryField)
}

/// `Release` with an empty mandatory tracklist. Triggers `EmptyMandatoryField`.
pub fn invalid_release_empty_tracklist() -> (Release, MiddsFormatError) {
    let Release::V1(mut v1) = min_size_release();
    v1.tracks = BoundedVec::default();
    (Release::V1(v1), MiddsFormatError::EmptyMandatoryField)
}

/// `Release` with an out-of-range release-date month. Triggers `OutOfBounds`.
pub fn invalid_release_bad_date() -> (Release, MiddsFormatError) {
    let Release::V1(mut v1) = min_size_release();
    v1.release_date = ReleaseDate {
        year: 2024,
        month: 13,
        day: 1,
    };
    (Release::V1(v1), MiddsFormatError::OutOfBounds)
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

    #[test]
    fn recording_min_size_validates() {
        min_size_recording()
            .validate_format()
            .expect("min size validates");
    }

    #[test]
    fn recording_max_size_validates_and_saturates_bound() {
        let recording = max_size_recording();
        recording.validate_format().expect("max size validates");
        let max = <Recording as MaxEncodedLen>::max_encoded_len();
        assert_eq!(
            parity_scale_codec::Encode::encoded_size(&recording),
            max,
            "max-size Recording must hit MaxEncodedLen exactly"
        );
    }

    #[test]
    fn recording_min_smaller_than_max_in_encoded_bytes() {
        use parity_scale_codec::Encode;
        let min = min_size_recording().encoded_size();
        let max = max_size_recording().encoded_size();
        assert!(min < max);
    }

    #[test]
    fn recording_invalid_constructors_match_expected_error() {
        for ctor in [
            invalid_recording_isrc_bad_charset as fn() -> _,
            invalid_recording_isrc_wrong_length,
            invalid_recording_empty_title,
            invalid_recording_work_iswc_prefix,
        ] {
            let (recording, expected) = ctor();
            let err = recording.validate_format().expect_err("payload must fail");
            assert_eq!(err, expected);
        }
    }

    #[test]
    fn release_min_size_validates() {
        min_size_release()
            .validate_format()
            .expect("min size validates");
    }

    #[test]
    fn release_max_size_validates_and_saturates_bound() {
        let release = max_size_release();
        release.validate_format().expect("max size validates");
        let max = <Release as MaxEncodedLen>::max_encoded_len();
        assert_eq!(
            parity_scale_codec::Encode::encoded_size(&release),
            max,
            "max-size Release must hit MaxEncodedLen exactly"
        );
    }

    #[test]
    fn release_min_smaller_than_max_in_encoded_bytes() {
        use parity_scale_codec::Encode;
        let min = min_size_release().encoded_size();
        let max = max_size_release().encoded_size();
        assert!(min < max);
    }

    #[test]
    fn release_invalid_constructors_match_expected_error() {
        for ctor in [
            invalid_release_upc_bad_charset as fn() -> _,
            invalid_release_upc_wrong_length,
            invalid_release_empty_title,
            invalid_release_empty_tracklist,
            invalid_release_bad_date,
        ] {
            let (release, expected) = ctor();
            let err = release.validate_format().expect_err("payload must fail");
            assert_eq!(err, expected);
        }
    }
}
