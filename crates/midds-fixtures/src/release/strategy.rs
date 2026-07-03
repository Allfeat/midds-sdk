//! `proptest::Strategy` implementations for `Release` and its parts.
//!
//! Strategies producing valid payloads do so by construction — they always
//! emit well-formed identifiers and respect every on-chain bound, so property
//! tests don't need `prop_assume` discards. Shared part-strategies (ISNI,
//! title, party id, ISRC) are reused from
//! [`crate::musical_work::strategy`] / [`crate::recording::strategy`] so the
//! cross-type wire shape stays identical and a fix lands once.
//!
//! For invalid payloads, [`arb_release_invalid`] returns a `(payload,
//! expected_error)` tuple so callers can assert that the on-chain validator
//! reports the same diagnostic.

use bounded_collections::BoundedVec;
use midds_traits::{MiddsFormatError, Upc};
use midds_types::release::{
    CATALOG_NUMBER_MAX_LEN, CatalogNumber, DISTRIBUTOR_NAME_MAX_LEN, FEATURING_MAX, Featuring,
    PRODUCERS_MAX, Producers, TITLE_ALIASES_MAX, TitleAliases, Track, Tracks,
};
use midds_types::{
    COVER_CONTRIBUTORS_MAX, Country, CoverContributors, DistributorName, PartyId, Producer,
    RecordingRef, Release, ReleaseDate, ReleaseFormat, ReleasePackaging, ReleaseStatus,
    ReleaseType, ReleaseV1, TRACKS_MAX,
};
use proptest::prelude::*;

use crate::common::printable_ascii;
use crate::identifiers::{ipi_from_stem, isni_from_body, isrc_for_index};
use crate::musical_work::strategy::{arb_isni, arb_title};
use crate::recording::strategy::arb_party_id;

const TITLE_MAX_LEN: u32 = 256;
const COVER_CONTRIBUTOR_NAME_MAX_LEN: u32 = 128;

fn ascii_digit() -> impl Strategy<Value = u8> {
    b'0'..=b'9'
}

/// UPC-A (12 digits) or EAN-13 (13 digits) — structurally valid by
/// construction (the check digit is not enforced on-chain, mirroring
/// `arb_isrc_valid`).
pub fn arb_upc() -> impl Strategy<Value = Upc> {
    prop_oneof![
        proptest::collection::vec(ascii_digit(), 12),
        proptest::collection::vec(ascii_digit(), 13),
    ]
    .prop_map(|bytes| BoundedVec::try_from(bytes).expect("upc within bound"))
}

/// Tracklist entry: an on-chain MIDDS id or an external ISRC.
pub fn arb_recording_ref() -> impl Strategy<Value = RecordingRef> {
    prop_oneof![
        any::<midds_traits::MiddsId>().prop_map(RecordingRef::Midds),
        crate::musical_work::strategy::arb_isrc_valid().prop_map(RecordingRef::Isrc),
    ]
}

fn arb_catalog_number() -> impl Strategy<Value = CatalogNumber> {
    proptest::collection::vec(printable_ascii(), 1..=(CATALOG_NUMBER_MAX_LEN as usize))
        .prop_map(|bytes| BoundedVec::try_from(bytes).expect("catalog number within bound"))
}

/// Producer pair (ISNI + non-empty catalog number), well-formed by
/// construction.
pub fn arb_producer() -> impl Strategy<Value = Producer> {
    (arb_isni(), arb_catalog_number()).prop_map(|(isni, catalog_number)| Producer {
        isni,
        catalog_number,
    })
}

pub fn arb_release_status() -> impl Strategy<Value = ReleaseStatus> {
    prop_oneof![
        Just(ReleaseStatus::Official),
        Just(ReleaseStatus::Promotional),
        Just(ReleaseStatus::Bootleg),
        Just(ReleaseStatus::PseudoRelease),
        Just(ReleaseStatus::Withdrawn),
        Just(ReleaseStatus::Cancelled),
        Just(ReleaseStatus::Other),
    ]
}

pub fn arb_release_type() -> impl Strategy<Value = ReleaseType> {
    prop_oneof![
        Just(ReleaseType::Album),
        Just(ReleaseType::Single),
        Just(ReleaseType::Ep),
        Just(ReleaseType::Broadcast),
        Just(ReleaseType::Compilation),
        Just(ReleaseType::Soundtrack),
        Just(ReleaseType::Live),
        Just(ReleaseType::Remix),
        Just(ReleaseType::Mixtape),
        Just(ReleaseType::Demo),
        Just(ReleaseType::Other),
    ]
}

pub fn arb_release_format() -> impl Strategy<Value = ReleaseFormat> {
    prop_oneof![
        Just(ReleaseFormat::Cd),
        Just(ReleaseFormat::Vinyl),
        Just(ReleaseFormat::Cassette),
        Just(ReleaseFormat::DigitalDownload),
        Just(ReleaseFormat::Streaming),
        Just(ReleaseFormat::Dvd),
        Just(ReleaseFormat::BluRay),
        Just(ReleaseFormat::Sacd),
        Just(ReleaseFormat::MiniDisc),
        Just(ReleaseFormat::ReelToReel),
        Just(ReleaseFormat::Other),
    ]
}

pub fn arb_release_packaging() -> impl Strategy<Value = ReleasePackaging> {
    prop_oneof![
        Just(ReleasePackaging::None),
        Just(ReleasePackaging::JewelCase),
        Just(ReleasePackaging::SlimJewelCase),
        Just(ReleasePackaging::Digipak),
        Just(ReleasePackaging::CardboardSleeve),
        Just(ReleasePackaging::Gatefold),
        Just(ReleasePackaging::KeepCase),
        Just(ReleasePackaging::Box),
        Just(ReleasePackaging::Other),
    ]
}

/// One of a fixed pool of common ISO 3166-1 countries. `Country` always
/// encodes to a single byte regardless of variant, so the pool only affects
/// value variety, never the encoded size.
pub fn arb_country() -> impl Strategy<Value = Country> {
    prop_oneof![
        Just(Country::Us),
        Just(Country::Fr),
        Just(Country::Gb),
        Just(Country::De),
        Just(Country::Jp),
        Just(Country::Br),
        Just(Country::Au),
        Just(Country::Za),
    ]
}

/// Range-valid calendar date (month `1..=12`, day `1..=31`).
pub fn arb_release_date() -> impl Strategy<Value = ReleaseDate> {
    (any::<u16>(), 1u8..=12u8, 1u8..=31u8).prop_map(|(year, month, day)| ReleaseDate {
        year,
        month,
        day,
    })
}

fn arb_title_aliases() -> impl Strategy<Value = TitleAliases> {
    proptest::collection::vec(arb_title(), 0..=(TITLE_ALIASES_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("title aliases within bound"))
}

/// Featured artists (0..=`FEATURING_MAX`), each a well-formed [`PartyId`].
fn arb_featuring() -> impl Strategy<Value = Featuring> {
    proptest::collection::vec(arb_party_id(), 0..=(FEATURING_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("featuring within bound"))
}

fn arb_tracks() -> impl Strategy<Value = Tracks> {
    proptest::collection::vec(arb_recording_ref(), 1..=(TRACKS_MAX as usize)).prop_map(|v| {
        // Dedup recordings, then number the survivors 1..=N — the strict
        // contiguous 1-based sequence required by `docs/validation.md` §6.
        let mut unique: Vec<RecordingRef> = Vec::with_capacity(v.len());
        for r in v {
            if !unique.contains(&r) {
                unique.push(r);
            }
        }
        let tracks: Vec<Track> = unique
            .into_iter()
            .enumerate()
            .map(|(i, recording)| Track {
                number: (i + 1) as u16,
                recording,
            })
            .collect();
        BoundedVec::try_from(tracks).expect("tracks within bound")
    })
}

fn arb_producers() -> impl Strategy<Value = Producers> {
    proptest::collection::vec(arb_producer(), 0..=(PRODUCERS_MAX as usize))
        .prop_map(|v| BoundedVec::try_from(v).expect("producers within bound"))
}

fn arb_distributor_name() -> impl Strategy<Value = DistributorName> {
    proptest::collection::vec(printable_ascii(), 1..=(DISTRIBUTOR_NAME_MAX_LEN as usize))
        .prop_map(|bytes| BoundedVec::try_from(bytes).expect("distributor name within bound"))
}

fn arb_cover_contributors() -> impl Strategy<Value = CoverContributors> {
    proptest::collection::vec(
        proptest::collection::vec(
            printable_ascii(),
            1..=(COVER_CONTRIBUTOR_NAME_MAX_LEN as usize),
        )
        .prop_map(|b| BoundedVec::try_from(b).expect("cover name within bound")),
        0..=(COVER_CONTRIBUTORS_MAX as usize),
    )
    .prop_map(|v| BoundedVec::try_from(v).expect("cover contributors within bound"))
}

/// Strategy producing valid `ReleaseV1` payloads.
///
/// `ReleaseV1` has 15 fields; proptest only implements `Strategy` for tuples
/// up to arity 12, so the fields are split into an 8-tuple and a 7-tuple and
/// flattened in the `prop_map`.
pub fn arb_release_v1() -> impl Strategy<Value = ReleaseV1> {
    let head = (
        arb_upc(),
        arb_title(),
        arb_title_aliases(),
        arb_party_id(),
        arb_tracks(),
        arb_producers(),
        arb_release_status(),
        arb_release_date(),
    );
    let tail = (
        arb_featuring(),
        arb_country(),
        arb_distributor_name(),
        arb_release_type(),
        arb_release_format(),
        arb_release_packaging(),
        arb_cover_contributors(),
    );
    (head, tail).prop_map(
        |(
            (upc, title, title_aliases, artist, tracks, producers, status, release_date),
            (
                featuring,
                country,
                distributor_name,
                release_type,
                format,
                packaging,
                cover_contributors,
            ),
        )| ReleaseV1 {
            upc,
            title,
            title_aliases,
            artist,
            featuring,
            tracks,
            producers,
            status,
            release_date,
            country,
            distributor_name,
            release_type,
            format,
            packaging,
            cover_contributors,
        },
    )
}

/// Strategy producing valid `Release` payloads that pass
/// `Midds::validate_format`.
pub fn arb_release() -> impl Strategy<Value = Release> {
    arb_release_v1().prop_map(Release::V1)
}

/// Strategy producing payloads saturated to `MaxEncodedLen`.
///
/// Every bounded field is filled to its bound and every `Option` is `Some`,
/// using the larger enum variant wherever a choice exists: `artist` and every
/// `featuring` entry as `PartyId::Both` (30 bytes vs ≤ 18 for single-id), every
/// track as `RecordingRef::Isrc` (the larger reference variant). The byte
/// content is randomised so shrinking still has work to do.
pub fn arb_release_max_size() -> impl Strategy<Value = Release> {
    (
        proptest::collection::vec(ascii_digit(), 13),
        proptest::collection::vec(printable_ascii(), TITLE_MAX_LEN as usize),
        proptest::collection::vec(
            proptest::collection::vec(printable_ascii(), TITLE_MAX_LEN as usize),
            TITLE_ALIASES_MAX as usize,
        ),
        any::<[u8; 15]>(),
        any::<u64>(),
        proptest::collection::vec(any::<u32>(), TRACKS_MAX as usize),
        proptest::collection::vec(
            (
                any::<[u8; 15]>(),
                proptest::collection::vec(printable_ascii(), CATALOG_NUMBER_MAX_LEN as usize),
            ),
            PRODUCERS_MAX as usize,
        ),
        proptest::collection::vec(printable_ascii(), DISTRIBUTOR_NAME_MAX_LEN as usize),
        proptest::collection::vec(
            proptest::collection::vec(printable_ascii(), COVER_CONTRIBUTOR_NAME_MAX_LEN as usize),
            COVER_CONTRIBUTORS_MAX as usize,
        ),
        proptest::collection::vec((any::<u64>(), any::<[u8; 15]>()), FEATURING_MAX as usize),
    )
        .prop_map(
            |(
                upc,
                title,
                aliases,
                artist_body,
                artist_stem,
                track_idxs,
                producer_bodies,
                distributor,
                cover_bodies,
                feat_pairs,
            )| {
                let title = BoundedVec::try_from(title).expect("title at bound");
                let title_aliases: Vec<_> = aliases
                    .into_iter()
                    .map(|a| BoundedVec::try_from(a).expect("alias at bound"))
                    .collect();
                let tracks: Vec<Track> = track_idxs
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| Track {
                        number: (idx + 1) as u16,
                        recording: RecordingRef::Isrc(isrc_for_index(idx as u32)),
                    })
                    .collect();
                let featuring: Vec<PartyId> = feat_pairs
                    .into_iter()
                    .map(|(stem, body)| PartyId::Both {
                        ipi: ipi_from_stem(stem, 11),
                        isni: isni_from_body(body),
                    })
                    .collect();
                let producers: Vec<Producer> = producer_bodies
                    .into_iter()
                    .map(|(isni_body, catalog)| Producer {
                        isni: isni_from_body(isni_body),
                        catalog_number: BoundedVec::try_from(catalog).expect("catalog at bound"),
                    })
                    .collect();
                let cover_contributors: Vec<_> = cover_bodies
                    .into_iter()
                    .map(|c| BoundedVec::try_from(c).expect("cover name at bound"))
                    .collect();
                let v1 = ReleaseV1 {
                    upc: BoundedVec::try_from(upc).expect("13-byte upc at bound"),
                    title,
                    title_aliases: BoundedVec::try_from(title_aliases).expect("aliases at bound"),
                    artist: PartyId::Both {
                        ipi: ipi_from_stem(artist_stem, 11),
                        isni: isni_from_body(artist_body),
                    },
                    featuring: BoundedVec::try_from(featuring).expect("featuring at bound"),
                    tracks: BoundedVec::try_from(tracks).expect("tracks at bound"),
                    producers: BoundedVec::try_from(producers).expect("producers at bound"),
                    status: ReleaseStatus::Official,
                    release_date: ReleaseDate {
                        year: u16::MAX,
                        month: 12,
                        day: 31,
                    },
                    country: Country::Us,
                    distributor_name: BoundedVec::try_from(distributor)
                        .expect("distributor at bound"),
                    release_type: ReleaseType::Album,
                    format: ReleaseFormat::Cd,
                    packaging: ReleasePackaging::None,
                    cover_contributors: BoundedVec::try_from(cover_contributors)
                        .expect("cover contributors at bound"),
                };
                Release::V1(v1)
            },
        )
}

/// Strategy producing payloads that systematically fail
/// `Midds::validate_format`, paired with the expected diagnostic so callers
/// can assert exact-error matches.
///
/// A single valid base is generated and then corrupted one of nine ways,
/// selected by `pick`. Mutating one shared base (rather than wrapping
/// [`arb_release_v1`] in a wide `prop_oneof!`) keeps the strategy tree shallow
/// — the many-armed form builds nine independent copies of the now-larger
/// release strategy and overflows the test stack in debug builds.
pub fn arb_release_invalid() -> impl Strategy<Value = (Release, MiddsFormatError)> {
    (arb_release_v1(), 0u8..9).prop_map(|(mut v1, pick)| {
        let err = match pick {
            0 => {
                v1.upc = BoundedVec::try_from(b"12345678".to_vec()).expect("8 bytes ≤ 13");
                MiddsFormatError::OutOfBounds
            }
            1 => {
                v1.upc = BoundedVec::try_from(b"40063813339X1".to_vec()).expect("13 bytes");
                MiddsFormatError::InvalidCharset
            }
            2 => {
                v1.title = BoundedVec::default();
                MiddsFormatError::EmptyMandatoryField
            }
            3 => {
                v1.tracks = BoundedVec::default();
                MiddsFormatError::EmptyMandatoryField
            }
            4 => {
                v1.release_date = ReleaseDate {
                    year: 2024,
                    month: 0,
                    day: 1,
                };
                MiddsFormatError::OutOfBounds
            }
            5 => {
                v1.distributor_name = BoundedVec::default();
                MiddsFormatError::EmptyMandatoryField
            }
            6 => {
                // Same recording under two different numbers → recording-uniqueness.
                let rec = v1.tracks[0].recording.clone();
                v1.tracks = BoundedVec::try_from(vec![
                    Track {
                        number: 1,
                        recording: rec.clone(),
                    },
                    Track {
                        number: 2,
                        recording: rec,
                    },
                ])
                .expect("2 tracks ≤ TRACKS_MAX");
                MiddsFormatError::CrossFieldInconsistency
            }
            7 => {
                // Non-contiguous numbering (gap: 1 then 3, no track 2) on two
                // distinct recordings → CrossFieldInconsistency.
                v1.tracks = BoundedVec::try_from(vec![
                    Track {
                        number: 1,
                        recording: RecordingRef::Midds(1),
                    },
                    Track {
                        number: 3,
                        recording: RecordingRef::Midds(2),
                    },
                ])
                .expect("2 tracks ≤ TRACKS_MAX");
                MiddsFormatError::CrossFieldInconsistency
            }
            _ => {
                // Reserved track number 0 → OutOfBounds.
                let rec = v1.tracks[0].recording.clone();
                v1.tracks = BoundedVec::try_from(vec![Track {
                    number: 0,
                    recording: rec,
                }])
                .expect("1 track");
                MiddsFormatError::OutOfBounds
            }
        };
        (Release::V1(v1), err)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use midds_traits::Midds as _;
    use parity_scale_codec::{Encode, MaxEncodedLen};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn arb_release_always_validates(r in arb_release()) {
            r.validate_format().expect("strategy emits valid payloads");
        }

        #[test]
        fn arb_release_max_size_validates(r in arb_release_max_size()) {
            r.validate_format().expect("max-size payload validates");
        }

        #[test]
        fn arb_release_max_size_saturates_bound(r in arb_release_max_size()) {
            let max = <Release as MaxEncodedLen>::max_encoded_len();
            prop_assert_eq!(r.encoded_size(), max);
        }

        #[test]
        fn arb_release_invalid_fails_with_expected_error(
            (r, expected) in arb_release_invalid(),
        ) {
            let err = r.validate_format().expect_err("invalid payload");
            prop_assert_eq!(err, expected);
        }
    }
}
