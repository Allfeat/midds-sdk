//! `proptest::Strategy` implementations for `Recording` and its parts.
//!
//! Strategies producing valid payloads do so by construction — they always
//! emit well-formed identifiers and respect every on-chain bound, so property
//! tests don't need `prop_assume` discards. Shared part-strategies (ISRC,
//! ISWC, ISNI, IPI, key, title, off-chain hash) are reused from
//! [`crate::musical_work::strategy`] so the cross-type wire shape stays
//! identical and a fix lands once.
//!
//! For invalid payloads, [`arb_recording_invalid`] returns a `(payload,
//! expected_error)` tuple so callers can assert that the on-chain validator
//! reports the same diagnostic.

use frame_support::BoundedVec;
use midds_traits::{Isni, MiddsFormatError};
use midds_types::{
    CONTRIBUTORS_MAX, Contributors, GENRES_MAX, Genre, Genres, PERFORMERS_MAX, PLACE_MAX_LEN,
    PRODUCERS_MAX, PartyId, Performers, Place, Producers, ProductionPlaces, Recording, RecordingV1,
    RecordingVersion, TITLE_ALIASES_MAX, TITLE_MAX_LEN, TitleAliases, WorkRef,
};
use proptest::prelude::*;

use crate::identifiers::{isni_from_body, isrc_for_index, iswc_from_work_code};
use crate::musical_work::strategy::{
    arb_ipi, arb_isni, arb_isrc_valid, arb_iswc, arb_musical_key, arb_offchain_hash, arb_title,
};

fn printable_ascii() -> impl Strategy<Value = u8> {
    32u8..=126u8
}

/// Strategy over the full genre taxonomy.
pub fn arb_genre() -> impl Strategy<Value = Genre> {
    prop_oneof![
        Just(Genre::Pop),
        Just(Genre::Rock),
        Just(Genre::HipHop),
        Just(Genre::RnB),
        Just(Genre::Electronic),
        Just(Genre::Dance),
        Just(Genre::Jazz),
        Just(Genre::Blues),
        Just(Genre::Classical),
        Just(Genre::Country),
        Just(Genre::Folk),
        Just(Genre::Metal),
        Just(Genre::Punk),
        Just(Genre::Reggae),
        Just(Genre::Latin),
        Just(Genre::World),
        Just(Genre::Soul),
        Just(Genre::Funk),
        Just(Genre::Gospel),
        Just(Genre::Soundtrack),
        Just(Genre::Ambient),
        Just(Genre::Experimental),
        Just(Genre::Children),
        Just(Genre::SpokenWord),
        Just(Genre::Other),
    ]
}

/// Strategy over every editorial recording version.
pub fn arb_recording_version() -> impl Strategy<Value = RecordingVersion> {
    prop_oneof![
        Just(RecordingVersion::Original),
        Just(RecordingVersion::RadioEdit),
        Just(RecordingVersion::Extended),
        Just(RecordingVersion::Remix),
        Just(RecordingVersion::Live),
        Just(RecordingVersion::Acoustic),
        Just(RecordingVersion::Instrumental),
        Just(RecordingVersion::ACapella),
        Just(RecordingVersion::Karaoke),
        Just(RecordingVersion::Demo),
        Just(RecordingVersion::ReRecorded),
        Just(RecordingVersion::Edited),
        Just(RecordingVersion::Cover),
    ]
}

/// Party identifier (artist / performer / contributor): IPI or ISNI, each
/// well-formed by construction.
pub fn arb_party_id() -> impl Strategy<Value = PartyId> {
    prop_oneof![
        arb_ipi().prop_map(PartyId::Ipi),
        arb_isni().prop_map(PartyId::Isni),
    ]
}

/// Reference to the recorded work: an on-chain MIDDS id or an external ISWC.
pub fn arb_work_ref() -> impl Strategy<Value = WorkRef> {
    prop_oneof![
        any::<midds_traits::MiddsId>().prop_map(WorkRef::Midds),
        arb_iswc().prop_map(WorkRef::Iswc),
    ]
}

/// Non-empty production-place name within `PLACE_MAX_LEN`.
fn arb_place() -> impl Strategy<Value = Place> {
    proptest::collection::vec(printable_ascii(), 1..=(PLACE_MAX_LEN as usize))
        .prop_map(|bytes| BoundedVec::try_from(bytes).expect("place within bound"))
}

/// Production places block — every sub-field independently optional, and
/// non-empty when present so `validate_format` accepts it.
pub fn arb_production_places() -> impl Strategy<Value = ProductionPlaces> {
    (
        proptest::option::of(arb_place()),
        proptest::option::of(arb_place()),
        proptest::option::of(arb_place()),
    )
        .prop_map(|(recording, mixing, mastering)| ProductionPlaces {
            recording,
            mixing,
            mastering,
        })
}

fn arb_title_aliases() -> impl Strategy<Value = TitleAliases> {
    proptest::collection::vec(arb_title(), 0..=(TITLE_ALIASES_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("title aliases within bound"))
}

fn arb_genres() -> impl Strategy<Value = Genres> {
    proptest::collection::vec(arb_genre(), 0..=(GENRES_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("genres within bound"))
}

fn arb_performers() -> impl Strategy<Value = Performers> {
    proptest::collection::vec(arb_party_id(), 0..=(PERFORMERS_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("performers within bound"))
}

fn arb_producers() -> impl Strategy<Value = Producers> {
    proptest::collection::vec(arb_isni(), 0..=(PRODUCERS_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("producers within bound"))
}

fn arb_contributors() -> impl Strategy<Value = Contributors> {
    proptest::collection::vec(arb_party_id(), 0..=(CONTRIBUTORS_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("contributors within bound"))
}

/// Strategy producing valid `RecordingV1` payloads.
///
/// `RecordingV1` has 16 fields; proptest only implements `Strategy` for
/// tuples up to arity 12, so the fields are split into two 8-tuples and
/// flattened in the `prop_map`.
pub fn arb_recording_v1() -> impl Strategy<Value = RecordingV1> {
    let head = (
        arb_isrc_valid(),
        arb_title(),
        arb_title_aliases(),
        arb_party_id(),
        arb_work_ref(),
        arb_genres(),
        proptest::option::of(1900u16..=2025u16),
        proptest::option::of(arb_recording_version()),
    );
    let tail = (
        arb_performers(),
        arb_producers(),
        proptest::option::of(any::<u32>()),
        proptest::option::of(40u16..=240u16),
        proptest::option::of(arb_musical_key()),
        proptest::option::of(arb_production_places()),
        arb_contributors(),
        proptest::option::of(arb_offchain_hash()),
    );
    (head, tail).prop_map(
        |(
            (isrc, title, title_aliases, artist, work, genres, record_year, version_type),
            (performers, producers, duration, bpm, key, places, contributors, offchain_extension),
        )| RecordingV1 {
            isrc,
            title,
            title_aliases,
            artist,
            work,
            genres,
            record_year,
            version_type,
            performers,
            producers,
            duration,
            bpm,
            key,
            places,
            contributors,
            offchain_extension,
        },
    )
}

/// Strategy producing valid `Recording` payloads that pass
/// `Midds::validate_format`.
pub fn arb_recording() -> impl Strategy<Value = Recording> {
    arb_recording_v1().prop_map(Recording::V1)
}

/// Strategy producing payloads saturated to `MaxEncodedLen`.
///
/// Every bounded field is filled to its bound and every `Option` is `Some`,
/// using the larger enum variant wherever a choice exists: `artist` /
/// `performers` / `contributors` as `PartyId::Isni` (17 bytes vs 12 for IPI),
/// `work` as `WorkRef::Iswc` (12 bytes vs 8 for the MIDDS id), all three
/// production places present at `PLACE_MAX_LEN`. The byte content is
/// randomised so shrinking still has work to do.
pub fn arb_recording_max_size() -> impl Strategy<Value = Recording> {
    (
        any::<u32>(),
        proptest::collection::vec(printable_ascii(), TITLE_MAX_LEN as usize),
        proptest::collection::vec(
            proptest::collection::vec(printable_ascii(), TITLE_MAX_LEN as usize),
            TITLE_ALIASES_MAX as usize,
        ),
        proptest::collection::vec(any::<[u8; 15]>(), PERFORMERS_MAX as usize),
        proptest::collection::vec(any::<[u8; 15]>(), PRODUCERS_MAX as usize),
        proptest::collection::vec(any::<[u8; 15]>(), CONTRIBUTORS_MAX as usize),
        any::<[u8; 15]>(),
        any::<u32>(),
        proptest::collection::vec(printable_ascii(), PLACE_MAX_LEN as usize),
        proptest::collection::vec(printable_ascii(), 64usize),
    )
        .prop_map(
            |(
                isrc_idx,
                title,
                aliases,
                performer_bodies,
                producer_bodies,
                contributor_bodies,
                artist_body,
                work_code,
                place,
                hash,
            )| {
                let title = BoundedVec::try_from(title).expect("title at bound");
                let title_aliases: Vec<_> = aliases
                    .into_iter()
                    .map(|a| BoundedVec::try_from(a).expect("alias at bound"))
                    .collect();
                let to_isni = |body: [u8; 15]| PartyId::Isni(isni_from_body(body));
                let performers: Vec<PartyId> = performer_bodies.into_iter().map(to_isni).collect();
                let producers: Vec<Isni> =
                    producer_bodies.into_iter().map(isni_from_body).collect();
                let contributors: Vec<PartyId> =
                    contributor_bodies.into_iter().map(to_isni).collect();
                let place = BoundedVec::try_from(place).expect("place at bound");
                let offchain = BoundedVec::try_from(hash).expect("offchain at bound");
                let v1 = RecordingV1 {
                    isrc: isrc_for_index(isrc_idx),
                    title,
                    title_aliases: BoundedVec::try_from(title_aliases).expect("aliases at bound"),
                    artist: PartyId::Isni(isni_from_body(artist_body)),
                    work: WorkRef::Iswc(iswc_from_work_code(work_code)),
                    genres: BoundedVec::try_from(vec![Genre::Other; GENRES_MAX as usize])
                        .expect("genres at bound"),
                    record_year: Some(u16::MAX),
                    version_type: Some(RecordingVersion::Original),
                    performers: BoundedVec::try_from(performers).expect("performers at bound"),
                    producers: BoundedVec::try_from(producers).expect("producers at bound"),
                    duration: Some(u32::MAX),
                    bpm: Some(u16::MAX),
                    // `MusicalKey` carries no length-bearing field, so any
                    // concrete value encodes to the same size — pin one.
                    key: Some(midds_types::MusicalKey {
                        pitch: midds_types::PitchClass::C,
                        mode: midds_types::Mode::Major,
                    }),
                    places: Some(ProductionPlaces {
                        recording: Some(place.clone()),
                        mixing: Some(place.clone()),
                        mastering: Some(place),
                    }),
                    contributors: BoundedVec::try_from(contributors)
                        .expect("contributors at bound"),
                    offchain_extension: Some(offchain),
                };
                Recording::V1(v1)
            },
        )
}

/// Strategy producing payloads that systematically fail
/// `Midds::validate_format`, paired with the expected diagnostic so callers
/// can assert exact-error matches.
pub fn arb_recording_invalid() -> impl Strategy<Value = (Recording, MiddsFormatError)> {
    prop_oneof![
        // Bad ISRC charset: lowercase country code.
        arb_recording_v1().prop_map(|mut v1| {
            let mut bad = v1.isrc.to_vec();
            bad[0] = b'f';
            v1.isrc = BoundedVec::try_from(bad).expect("12 bytes");
            (Recording::V1(v1), MiddsFormatError::InvalidCharset)
        }),
        // ISRC too short ⇒ length check fires before charset.
        arb_recording_v1().prop_map(|mut v1| {
            let mut bad = v1.isrc.to_vec();
            bad.truncate(11);
            v1.isrc = BoundedVec::try_from(bad).expect("11 bytes ≤ 12");
            (Recording::V1(v1), MiddsFormatError::OutOfBounds)
        }),
        // Empty title.
        arb_recording_v1().prop_map(|mut v1| {
            v1.title = BoundedVec::default();
            (Recording::V1(v1), MiddsFormatError::EmptyMandatoryField)
        }),
        // Empty title alias.
        arb_recording_v1().prop_map(|mut v1| {
            v1.title_aliases = BoundedVec::try_from(vec![BoundedVec::default()]).expect("1 alias");
            (Recording::V1(v1), MiddsFormatError::EmptyMandatoryField)
        }),
        // Bad work ISWC structural prefix: replace `T` with `X`.
        arb_recording_v1().prop_map(|mut v1| {
            v1.work =
                WorkRef::Iswc(BoundedVec::try_from(b"X1234567890".to_vec()).expect("11 bytes"));
            (
                Recording::V1(v1),
                MiddsFormatError::InvalidIdentifierStructure,
            )
        }),
        // Empty offchain extension.
        arb_recording_v1().prop_map(|mut v1| {
            v1.offchain_extension = Some(BoundedVec::default());
            (Recording::V1(v1), MiddsFormatError::EmptyMandatoryField)
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use midds_traits::Midds as _;
    use parity_scale_codec::{Encode, MaxEncodedLen};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn arb_recording_always_validates(r in arb_recording()) {
            r.validate_format().expect("strategy emits valid payloads");
        }

        #[test]
        fn arb_recording_max_size_validates(r in arb_recording_max_size()) {
            r.validate_format().expect("max-size payload validates");
        }

        #[test]
        fn arb_recording_max_size_saturates_bound(r in arb_recording_max_size()) {
            let max = <Recording as MaxEncodedLen>::max_encoded_len();
            // Every bounded field is filled, every Option is Some, and the
            // worst-case PartyId / WorkRef variants are used — so the encoded
            // size lands exactly on the bound.
            prop_assert_eq!(r.encoded_size(), max);
        }

        #[test]
        fn arb_recording_invalid_fails_with_expected_error(
            (r, expected) in arb_recording_invalid(),
        ) {
            let err = r.validate_format().expect_err("invalid payload");
            prop_assert_eq!(err, expected);
        }
    }
}
