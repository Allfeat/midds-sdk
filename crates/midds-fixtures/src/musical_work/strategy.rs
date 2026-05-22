//! `proptest::Strategy` implementations for `MusicalWork` and its parts.
//!
//! Strategies producing valid payloads do so by construction — they always
//! emit checksum-correct identifiers and respect every on-chain bound. This
//! avoids `prop_assume` discards in property tests and keeps shrinking
//! tractable.
//!
//! For invalid payloads, [`arb_musical_work_invalid`] returns a `(payload,
//! expected_error)` tuple so callers can assert that the on-chain validator
//! reports the same diagnostic.

use frame_support::BoundedVec;
use midds_traits::{Isrc, Iswc, MiddsFormatError, OffchainHash};
use midds_types::shared::{BPM_MAX, YEAR_MAX, YEAR_MIN};
use midds_types::{
    CATALOG_NUMBER_MAX_LEN, CREATOR_ROLES_MAX, CREATORS_MAX, ClassicalInfo, Creator, CreatorRole,
    CreatorRoles, Creators, Language, Mode, MusicalKey, MusicalWork, MusicalWorkV1, OPUS_MAX_LEN,
    PartyId, PitchClass, TITLE_MAX_LEN, Title, WORK_REFERENCES_MAX, WorkReferences, WorkType,
};
use proptest::prelude::*;

use crate::identifiers::{ipi_from_stem, isni_from_body, isrc_for_index, iswc_from_work_code};
use crate::recording::strategy::arb_party_id;

/// Strategy producing checksum-correct ISWCs.
pub fn arb_iswc() -> impl Strategy<Value = Iswc> {
    (0u32..1_000_000_000).prop_map(iswc_from_work_code)
}

/// Strategy producing checksum-correct IPIs of length 9..=11.
pub fn arb_ipi() -> impl Strategy<Value = midds_traits::Ipi> {
    (any::<u64>(), 9usize..=11).prop_map(|(stem, len)| ipi_from_stem(stem, len))
}

/// Strategy producing checksum-correct ISNIs.
pub fn arb_isni() -> impl Strategy<Value = midds_traits::Isni> {
    any::<[u8; 15]>().prop_map(isni_from_body)
}

/// Strategy producing structurally valid ISRCs (ISO 3901 has no check
/// digit, so "valid" here means well-formed shape only).
pub fn arb_isrc_valid() -> impl Strategy<Value = Isrc> {
    any::<u32>().prop_map(isrc_for_index)
}

/// Strategy producing payloads that fail [`midds_traits::validate_isrc_format`].
/// Targets each branch of the validator: wrong length, bad country code,
/// bad registrant, bad year, bad designation. Useful for negative-path
/// `MusicalWork`-extension tests once Recording lands.
pub fn arb_isrc_invalid() -> impl Strategy<Value = (Isrc, MiddsFormatError)> {
    prop_oneof![
        Just((
            BoundedVec::try_from(b"12RC17607839".to_vec()).expect("12 bytes"),
            MiddsFormatError::InvalidCharset,
        )),
        Just((
            BoundedVec::try_from(b"USrc17607839".to_vec()).expect("12 bytes"),
            MiddsFormatError::InvalidCharset,
        )),
        Just((
            BoundedVec::try_from(b"USRC1A607839".to_vec()).expect("12 bytes"),
            MiddsFormatError::InvalidCharset,
        )),
        Just((
            BoundedVec::try_from(b"USRC1760783A".to_vec()).expect("12 bytes"),
            MiddsFormatError::InvalidCharset,
        )),
        Just((
            BoundedVec::try_from(b"USRC".to_vec()).expect("≤ 12 bytes"),
            MiddsFormatError::OutOfBounds,
        )),
    ]
}

/// Strategy producing non-empty bounded titles.
pub fn arb_title() -> impl Strategy<Value = Title> {
    proptest::collection::vec(printable_ascii(), 1..=(TITLE_MAX_LEN as usize))
        .prop_map(|bytes| BoundedVec::try_from(bytes).expect("title within bound"))
}

/// Strategy producing non-empty off-chain extension hashes.
pub fn arb_offchain_hash() -> impl Strategy<Value = OffchainHash> {
    proptest::collection::vec(printable_ascii(), 1..=64usize)
        .prop_map(|bytes| BoundedVec::try_from(bytes).expect("offchain hash within bound"))
}

fn printable_ascii() -> impl Strategy<Value = u8> {
    32u8..=126u8
}

/// Strategy over the full ISO 639-1 enum.
pub fn arb_language() -> impl Strategy<Value = Language> {
    prop_oneof![
        Just(Language::En),
        Just(Language::Fr),
        Just(Language::Es),
        Just(Language::De),
        Just(Language::Ja),
        Just(Language::Zh),
        Just(Language::Ar),
        Just(Language::Ru),
        Just(Language::Pt),
        Just(Language::It),
    ]
}

pub fn arb_pitch_class() -> impl Strategy<Value = PitchClass> {
    prop_oneof![
        Just(PitchClass::C),
        Just(PitchClass::CSharp),
        Just(PitchClass::D),
        Just(PitchClass::DSharp),
        Just(PitchClass::E),
        Just(PitchClass::F),
        Just(PitchClass::FSharp),
        Just(PitchClass::G),
        Just(PitchClass::GSharp),
        Just(PitchClass::A),
        Just(PitchClass::ASharp),
        Just(PitchClass::B),
    ]
}

pub fn arb_mode() -> impl Strategy<Value = Mode> {
    prop_oneof![Just(Mode::Major), Just(Mode::Minor)]
}

pub fn arb_musical_key() -> impl Strategy<Value = MusicalKey> {
    (arb_pitch_class(), arb_mode()).prop_map(|(pitch, mode)| MusicalKey { pitch, mode })
}

pub fn arb_creator_role() -> impl Strategy<Value = CreatorRole> {
    prop_oneof![
        Just(CreatorRole::Author),
        Just(CreatorRole::Composer),
        Just(CreatorRole::Arranger),
        Just(CreatorRole::Adapter),
        Just(CreatorRole::Publisher),
    ]
}

/// Strategy producing a non-empty [`CreatorRoles`] set (1..=`CREATOR_ROLES_MAX`
/// distinct roles). The bounded BTreeSet rejects duplicates by construction,
/// so the input vector is deduplicated via the set rather than via a
/// `prop_filter` (which would slow shrinking).
pub fn arb_creator_roles() -> impl Strategy<Value = CreatorRoles> {
    proptest::collection::vec(arb_creator_role(), 1..=(CREATOR_ROLES_MAX as usize)).prop_map(
        |roles| {
            let mut set = CreatorRoles::new();
            for r in roles {
                // Ignore failure: a duplicate insert returns `Ok(false)`,
                // a full-set insert returns `Err(value)`. Either way the
                // resulting set has between 1 and CREATOR_ROLES_MAX entries.
                let _ = set.try_insert(r);
            }
            // Vec was non-empty, all branches above retain at least one role.
            debug_assert!(!set.is_empty());
            set
        },
    )
}

pub fn arb_creator() -> impl Strategy<Value = Creator> {
    (arb_creator_roles(), arb_party_id()).prop_map(|(roles, party)| Creator { roles, party })
}

/// Strategy producing 1..=`CREATORS_MAX` creators (always non-empty so
/// `validate_format` accepts the result).
pub fn arb_creators() -> impl Strategy<Value = Creators> {
    proptest::collection::vec(arb_creator(), 1..=(CREATORS_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("creators within bound"))
}

/// `WorkType` strategy spanning every variant. Source-ref lists for derived
/// variants carry >= 2 refs so the result validates (`validate_format`
/// requires Medley / Mashup to reference at least two source works).
pub fn arb_work_type() -> impl Strategy<Value = WorkType> {
    prop_oneof![
        Just(WorkType::Original),
        arb_work_references().prop_map(WorkType::Medley),
        arb_work_references().prop_map(WorkType::Mashup),
        arb_iswc().prop_map(WorkType::Adaptation),
    ]
}

fn arb_work_references() -> impl Strategy<Value = WorkReferences> {
    proptest::collection::vec(arb_iswc(), 2..=(WORK_REFERENCES_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("work refs within bound"))
}

pub fn arb_classical_info() -> impl Strategy<Value = ClassicalInfo> {
    (
        proptest::option::of(
            proptest::collection::vec(printable_ascii(), 1..=(OPUS_MAX_LEN as usize))
                .prop_map(|v| BoundedVec::try_from(v).expect("opus within bound")),
        ),
        proptest::option::of(
            proptest::collection::vec(printable_ascii(), 1..=(CATALOG_NUMBER_MAX_LEN as usize))
                .prop_map(|v| BoundedVec::try_from(v).expect("catalog within bound")),
        ),
        proptest::option::of(1u16..=u16::MAX),
    )
        .prop_map(|(opus, catalog_number, number_of_voices)| ClassicalInfo {
            opus,
            catalog_number,
            number_of_voices,
        })
}

/// Strategy producing valid `MusicalWorkV1` payloads. Honours the
/// "instrumental ⇒ no language" convention only by construction; if you
/// override `instrumental`, call `prop_filter`/`prop_map` to drop the language.
pub fn arb_musical_work_v1() -> impl Strategy<Value = MusicalWorkV1> {
    (
        arb_iswc(),
        arb_title(),
        proptest::option::of(YEAR_MIN..=YEAR_MAX),
        any::<bool>(),
        proptest::option::of(arb_language()),
        proptest::option::of(40u16..=240u16),
        proptest::option::of(arb_musical_key()),
        arb_work_type(),
        arb_creators(),
        proptest::option::of(arb_classical_info()),
        proptest::option::of(arb_offchain_hash()),
    )
        .prop_map(
            |(
                iswc,
                title,
                creation_year,
                instrumental,
                language,
                bpm,
                key,
                work_type,
                creators,
                classical_info,
                offchain_extension,
            )| MusicalWorkV1 {
                iswc,
                title,
                creation_year,
                instrumental,
                language,
                bpm,
                key,
                work_type,
                creators,
                classical_info,
                offchain_extension,
            },
        )
}

/// Strategy producing valid `MusicalWork` payloads that pass
/// `Midds::validate_format`.
pub fn arb_musical_work() -> impl Strategy<Value = MusicalWork> {
    arb_musical_work_v1().prop_map(MusicalWork::V1)
}

/// Strategy producing payloads saturated to `MaxEncodedLen`.
///
/// Every bounded field is filled to its bound: `title` to `TITLE_MAX_LEN`;
/// `creators` to `CREATORS_MAX` entries, each carrying the full
/// `CREATOR_ROLES_MAX` (= 5) distinct roles and the `PartyId::Both` variant
/// (the largest of the three — full 11-byte IPI + 16-byte ISNI ≈ 30 bytes
/// vs ≤ 18 for the single-id variants); `classical_info` populated with
/// max-size `opus` and `catalog_number`; off-chain hash at 64 bytes; work
/// type set to Medley with `WORK_REFERENCES_MAX` refs (the largest variant).
/// The byte content is randomised so shrinking still has work to do.
pub fn arb_musical_work_max_size() -> impl Strategy<Value = MusicalWork> {
    (
        arb_iswc(),
        proptest::collection::vec(printable_ascii(), TITLE_MAX_LEN as usize),
        proptest::collection::vec(any::<[u8; 15]>(), CREATORS_MAX as usize),
        proptest::collection::vec(any::<u64>(), CREATORS_MAX as usize),
        proptest::collection::vec(printable_ascii(), OPUS_MAX_LEN as usize),
        proptest::collection::vec(printable_ascii(), CATALOG_NUMBER_MAX_LEN as usize),
        proptest::collection::vec(printable_ascii(), 64usize),
        proptest::collection::vec(0u32..1_000_000_000u32, WORK_REFERENCES_MAX as usize),
    )
        .prop_map(
            |(iswc, title, isni_bodies, ipi_stems, opus, catalog, hash, work_codes)| {
                let title = BoundedVec::try_from(title).expect("title at bound");
                let all_roles = {
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
                };
                let creators: Vec<Creator> = isni_bodies
                    .into_iter()
                    .zip(ipi_stems)
                    .map(|(body, stem)| Creator {
                        roles: all_roles.clone(),
                        party: PartyId::Both {
                            ipi: ipi_from_stem(stem, 11),
                            isni: isni_from_body(body),
                        },
                    })
                    .collect();
                let creators = BoundedVec::try_from(creators).expect("creators at bound");
                let classical = ClassicalInfo {
                    opus: Some(BoundedVec::try_from(opus).expect("opus at bound")),
                    catalog_number: Some(BoundedVec::try_from(catalog).expect("catalog at bound")),
                    number_of_voices: Some(u16::MAX),
                };
                let offchain = BoundedVec::try_from(hash).expect("offchain at bound");
                let refs: Vec<Iswc> = work_codes.into_iter().map(iswc_from_work_code).collect();
                let work_type =
                    WorkType::Medley(BoundedVec::try_from(refs).expect("work refs at bound"));
                let v1 = MusicalWorkV1 {
                    iswc,
                    title,
                    creation_year: Some(YEAR_MAX),
                    instrumental: false,
                    language: Some(Language::En),
                    bpm: Some(BPM_MAX),
                    key: Some(MusicalKey {
                        pitch: PitchClass::C,
                        mode: Mode::Major,
                    }),
                    work_type,
                    creators,
                    classical_info: Some(classical),
                    offchain_extension: Some(offchain),
                };
                MusicalWork::V1(v1)
            },
        )
}

/// Strategy producing payloads that systematically fail
/// `Midds::validate_format`, paired with the expected diagnostic so callers
/// can assert exact-error matches.
pub fn arb_musical_work_invalid() -> impl Strategy<Value = (MusicalWork, MiddsFormatError)> {
    prop_oneof![
        arb_musical_work_v1().prop_map(|mut v1| {
            let mut bad = v1.iswc.to_vec();
            bad[5] = b'A';
            v1.iswc = BoundedVec::try_from(bad).expect("11 bytes");
            (MusicalWork::V1(v1), MiddsFormatError::InvalidCharset)
        }),
        arb_musical_work_v1().prop_map(|mut v1| {
            let mut bad = v1.iswc.to_vec();
            bad[0] = b'X';
            v1.iswc = BoundedVec::try_from(bad).expect("11 bytes");
            (
                MusicalWork::V1(v1),
                MiddsFormatError::InvalidIdentifierStructure,
            )
        }),
        arb_musical_work_v1().prop_map(|mut v1| {
            v1.title = BoundedVec::default();
            (MusicalWork::V1(v1), MiddsFormatError::EmptyMandatoryField)
        }),
        arb_musical_work_v1().prop_map(|mut v1| {
            v1.creators = BoundedVec::default();
            (MusicalWork::V1(v1), MiddsFormatError::EmptyMandatoryField)
        }),
        arb_musical_work_v1().prop_map(|mut v1| {
            v1.work_type = WorkType::Medley(BoundedVec::default());
            (MusicalWork::V1(v1), MiddsFormatError::OutOfBounds)
        }),
        arb_musical_work_v1().prop_map(|mut v1| {
            v1.offchain_extension = Some(BoundedVec::default());
            (MusicalWork::V1(v1), MiddsFormatError::EmptyMandatoryField)
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
        fn arb_musical_work_always_validates(work in arb_musical_work()) {
            work.validate_format().expect("strategy emits valid payloads");
        }

        #[test]
        fn arb_musical_work_max_size_validates(work in arb_musical_work_max_size()) {
            work.validate_format().expect("max-size payload validates");
        }

        #[test]
        fn arb_musical_work_max_size_saturates_bound(work in arb_musical_work_max_size()) {
            let max = <MusicalWork as MaxEncodedLen>::max_encoded_len();
            let actual = work.encoded_size();
            prop_assert_eq!(actual, max);
        }

        #[test]
        fn arb_musical_work_invalid_fails_with_expected_error(
            (work, expected) in arb_musical_work_invalid(),
        ) {
            let err = work.validate_format().expect_err("invalid payload");
            prop_assert_eq!(err, expected);
        }
    }
}
