pub mod v1;

pub use v1::{
    CATALOG_NUMBER_MAX_LEN, COVER_CONTRIBUTOR_NAME_MAX_LEN, COVER_CONTRIBUTORS_MAX, CatalogNumber,
    CoverContributorName, CoverContributors, DISTRIBUTOR_NAME_MAX_LEN, DistributorName,
    PRODUCERS_MAX, Producer, Producers, ReleaseDate, ReleaseFormat, ReleasePackaging,
    ReleaseStatus, ReleaseType, ReleaseV1, TITLE_ALIASES_MAX, TRACKS_MAX, TitleAliases, Tracks,
};

use midds_traits::{Midds, MiddsFormatError, Upc};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Top-level versioned `Release`. New on-chain versions are added as new
/// enum variants and migrated explicitly via `OnRuntimeUpgrade`.
///
/// JSON shape: internally tagged on a `"version"` field. `V1` serialises as
/// `{"version": "v1", "upc": "…", "title": "…", …}` (flat). When `V2` is
/// added, existing consumers can dispatch on the tag without re-parsing.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "version", rename_all = "lowercase"))]
pub enum Release {
    V1(ReleaseV1),
}

impl Midds for Release {
    const KIND: &'static str = "Release";

    type Identifier = Upc;

    fn identifier(&self) -> &Upc {
        match self {
            Self::V1(v) => &v.upc,
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
    use crate::Country;
    use crate::shared::{PartyId, RecordingRef};
    use bounded_collections::BoundedVec;

    fn bv<const N: u32>(s: &[u8]) -> midds_traits::MiddsString<N> {
        BoundedVec::try_from(s.to_vec()).expect("bounded vec build")
    }

    fn sample_v1() -> ReleaseV1 {
        ReleaseV1 {
            upc: bv(b"4006381333931"),
            title: bv(b"My Release Title"),
            title_aliases: BoundedVec::default(),
            artist: PartyId::Ipi(bv::<11>(b"123456789")),
            tracks: BoundedVec::try_from(vec![RecordingRef::Isrc(bv::<12>(b"FRABC2412345"))])
                .expect("one track"),
            producers: BoundedVec::default(),
            status: ReleaseStatus::Official,
            release_date: ReleaseDate {
                year: 2024,
                month: 3,
                day: 15,
            },
            country: Country::Fr,
            distributor_name: bv(b"Believe"),
            release_type: ReleaseType::Album,
            format: ReleaseFormat::Cd,
            packaging: ReleasePackaging::JewelCase,
            cover_contributors: BoundedVec::default(),
            offchain_extension: None,
        }
    }

    #[test]
    fn roundtrip_encode_decode() {
        let original = Release::V1(sample_v1());
        let encoded = original.encode();
        let decoded = Release::decode(&mut &encoded[..]).expect("decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn identifier_returns_upc() {
        let r = Release::V1(sample_v1());
        assert_eq!(r.identifier(), &bv::<13>(b"4006381333931"));
    }

    #[test]
    fn kind_is_release() {
        assert_eq!(Release::KIND, "Release");
    }

    #[test]
    fn validate_pass_minimal() {
        assert!(Release::V1(sample_v1()).validate_format().is_ok());
    }

    #[test]
    fn validate_pass_full() {
        let mut v = sample_v1();
        v.upc = bv(b"036000291452"); // 12-digit UPC-A
        v.title_aliases = BoundedVec::try_from(vec![bv(b"Alt Title"), bv(b"Titre FR")]).unwrap();
        v.artist = PartyId::Isni(bv::<16>(b"0000000121032683"));
        v.tracks = BoundedVec::try_from(vec![
            RecordingRef::Isrc(bv::<12>(b"FRABC2412345")),
            RecordingRef::Midds(42),
        ])
        .unwrap();
        v.producers = BoundedVec::try_from(vec![Producer {
            isni: bv::<16>(b"000000012103268X"),
            catalog_number: bv(b"88985456971"),
        }])
        .unwrap();
        v.status = ReleaseStatus::Promotional;
        v.country = Country::Us;
        v.release_type = ReleaseType::Compilation;
        v.format = ReleaseFormat::Vinyl;
        v.packaging = ReleasePackaging::Gatefold;
        v.cover_contributors =
            BoundedVec::try_from(vec![bv(b"Jane Photographer"), bv(b"John Designer")]).unwrap();
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));
        assert!(Release::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_fails_invalid_upc() {
        let mut v = sample_v1();
        v.upc = bv(b"40063813339X1");
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_upc_wrong_length() {
        let mut v = sample_v1();
        v.upc = bv(b"40063813");
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::OutOfBounds),
        );
    }

    #[test]
    fn validate_fails_empty_title() {
        let mut v = sample_v1();
        v.title = bv(b"");
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_empty_title_alias() {
        let mut v = sample_v1();
        v.title_aliases = BoundedVec::try_from(vec![bv(b"")]).unwrap();
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_invalid_artist_ipi() {
        let mut v = sample_v1();
        v.artist = PartyId::Ipi(bv::<11>(b"12345A789"));
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_empty_tracklist() {
        let mut v = sample_v1();
        v.tracks = BoundedVec::default();
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_invalid_track_isrc() {
        let mut v = sample_v1();
        v.tracks =
            BoundedVec::try_from(vec![RecordingRef::Isrc(bv::<12>(b"frABC2412345"))]).unwrap();
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_pass_track_midds_ref() {
        let mut v = sample_v1();
        v.tracks = BoundedVec::try_from(vec![RecordingRef::Midds(7)]).unwrap();
        assert!(Release::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_fails_invalid_producer_isni() {
        let mut v = sample_v1();
        v.producers = BoundedVec::try_from(vec![Producer {
            isni: bv::<16>(b"00000001A1032683"),
            catalog_number: bv(b"CAT-1"),
        }])
        .unwrap();
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_empty_producer_catalog_number() {
        let mut v = sample_v1();
        v.producers = BoundedVec::try_from(vec![Producer {
            isni: bv::<16>(b"0000000121032683"),
            catalog_number: bv(b""),
        }])
        .unwrap();
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_bad_release_date() {
        let mut v = sample_v1();
        v.release_date = ReleaseDate {
            year: 2024,
            month: 13,
            day: 1,
        };
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::OutOfBounds),
        );
    }

    #[test]
    fn validate_fails_empty_distributor() {
        let mut v = sample_v1();
        v.distributor_name = bv(b"");
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_empty_cover_contributor() {
        let mut v = sample_v1();
        v.cover_contributors = BoundedVec::try_from(vec![bv(b"")]).unwrap();
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_empty_offchain_extension() {
        let mut v = sample_v1();
        v.offchain_extension = Some(bv(b""));
        assert_eq!(
            Release::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn max_encoded_len_is_finite() {
        let max = <Release as MaxEncodedLen>::max_encoded_len();
        assert!(max > 0);
        // A `Release` aggregates a full tracklist (up to `TRACKS_MAX`
        // `RecordingRef`s) plus aliases / producers / cover credits, so its
        // bound is legitimately larger than a single-entity `Recording`.
        assert!(max < 16384, "Release max_encoded_len = {max}");
    }
}

#[cfg(all(test, feature = "serde"))]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod json_tests {
    //! Snapshot tests pinning the `Release` JSON wire format — the
    //! cross-service schema contract. A field rename or shape change must
    //! fail one of these on purpose, forcing a deliberate version bump.

    use super::*;
    use crate::Country;
    use crate::shared::{PartyId, RecordingRef};
    use bounded_collections::BoundedVec;

    fn bv<const N: u32>(s: &[u8]) -> midds_traits::MiddsString<N> {
        BoundedVec::try_from(s.to_vec()).expect("bounded vec build")
    }

    fn minimal_v1() -> ReleaseV1 {
        ReleaseV1 {
            upc: bv(b"4006381333931"),
            title: bv(b"My Release Title"),
            title_aliases: BoundedVec::default(),
            artist: PartyId::Ipi(bv::<11>(b"123456789")),
            tracks: BoundedVec::try_from(vec![RecordingRef::Isrc(bv::<12>(b"FRABC2412345"))])
                .expect("one track"),
            producers: BoundedVec::default(),
            status: ReleaseStatus::Official,
            release_date: ReleaseDate {
                year: 2024,
                month: 3,
                day: 15,
            },
            country: Country::Fr,
            distributor_name: bv(b"Believe"),
            release_type: ReleaseType::Album,
            format: ReleaseFormat::Cd,
            packaging: ReleasePackaging::JewelCase,
            cover_contributors: BoundedVec::default(),
            offchain_extension: None,
        }
    }

    #[test]
    fn shape_minimal() {
        let json = serde_json::to_value(Release::V1(minimal_v1())).unwrap();

        assert_eq!(json["version"], "v1");
        assert_eq!(json["upc"], "4006381333931");
        assert_eq!(json["title"], "My Release Title");
        assert_eq!(json["title_aliases"], serde_json::json!([]));
        assert_eq!(json["artist"]["Ipi"], "123456789");
        assert_eq!(json["tracks"][0]["Isrc"], "FRABC2412345");
        assert_eq!(json["producers"], serde_json::json!([]));
        assert_eq!(json["status"], "Official");
        assert_eq!(json["release_date"]["year"], 2024);
        assert_eq!(json["release_date"]["month"], 3);
        assert_eq!(json["release_date"]["day"], 15);
        assert_eq!(json["country"], "FR");
        assert_eq!(json["distributor_name"], "Believe");
        assert_eq!(json["release_type"], "Album");
        assert_eq!(json["format"], "Cd");
        assert_eq!(json["packaging"], "JewelCase");
        assert_eq!(json["cover_contributors"], serde_json::json!([]));
        assert!(json["offchain_extension"].is_null());
    }

    #[test]
    fn roundtrip_minimal() {
        let original = Release::V1(minimal_v1());
        let raw = serde_json::to_string(&original).unwrap();
        let restored: Release = serde_json::from_str(&raw).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn shape_full() {
        let mut v = minimal_v1();
        v.title_aliases = BoundedVec::try_from(vec![bv(b"Alt"), bv(b"Titre")]).unwrap();
        v.tracks = BoundedVec::try_from(vec![
            RecordingRef::Midds(42),
            RecordingRef::Isrc(bv::<12>(b"USRC17607839")),
        ])
        .unwrap();
        v.producers = BoundedVec::try_from(vec![Producer {
            isni: bv::<16>(b"000000012103268X"),
            catalog_number: bv(b"88985456971"),
        }])
        .unwrap();
        v.status = ReleaseStatus::Bootleg;
        v.country = Country::Us;
        v.release_type = ReleaseType::Live;
        v.format = ReleaseFormat::Vinyl;
        v.packaging = ReleasePackaging::Gatefold;
        v.cover_contributors = BoundedVec::try_from(vec![bv(b"Jane Photographer")]).unwrap();
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));

        let json = serde_json::to_value(Release::V1(v)).unwrap();
        assert_eq!(json["title_aliases"], serde_json::json!(["Alt", "Titre"]));
        assert_eq!(json["tracks"][0]["Midds"], 42);
        assert_eq!(json["tracks"][1]["Isrc"], "USRC17607839");
        assert_eq!(json["producers"][0]["isni"], "000000012103268X");
        assert_eq!(json["producers"][0]["catalog_number"], "88985456971");
        assert_eq!(json["status"], "Bootleg");
        assert_eq!(json["country"], "US");
        assert_eq!(json["release_type"], "Live");
        assert_eq!(json["format"], "Vinyl");
        assert_eq!(json["packaging"], "Gatefold");
        assert_eq!(
            json["cover_contributors"],
            serde_json::json!(["Jane Photographer"])
        );
        assert_eq!(json["offchain_extension"], "bafkreigh2akiscaildc");
    }

    #[test]
    fn roundtrip_full() {
        let mut v = minimal_v1();
        v.tracks = BoundedVec::try_from(vec![RecordingRef::Midds(99)]).unwrap();
        v.producers = BoundedVec::try_from(vec![Producer {
            isni: bv::<16>(b"0000000121032683"),
            catalog_number: bv(b"CAT-001"),
        }])
        .unwrap();
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));

        let original = Release::V1(v);
        let raw = serde_json::to_string(&original).unwrap();
        let restored: Release = serde_json::from_str(&raw).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn rejects_oversized_upc_at_decode() {
        let bad = r#"{
            "version":"v1",
            "upc":"40063813339311",
            "title":"x",
            "title_aliases":[],
            "artist":{"Ipi":"123456789"},
            "tracks":[{"Isrc":"FRABC2412345"}],
            "producers":[],
            "status":"Official",
            "release_date":{"year":2024,"month":3,"day":15},
            "country":"FR",
            "distributor_name":"Believe",
            "release_type":"Album",
            "format":"Cd",
            "packaging":"JewelCase",
            "cover_contributors":[],
            "offchain_extension":null
        }"#;
        let r: Result<Release, _> = serde_json::from_str(bad);
        assert!(r.is_err(), "expected length-bound rejection");
    }

    #[test]
    fn rejects_unknown_version_tag() {
        let bad = r#"{
            "version":"v2",
            "upc":"4006381333931",
            "title":"x",
            "title_aliases":[],
            "artist":{"Ipi":"123456789"},
            "tracks":[{"Isrc":"FRABC2412345"}],
            "producers":[],
            "status":"Official",
            "release_date":{"year":2024,"month":3,"day":15},
            "country":"FR",
            "distributor_name":"Believe",
            "release_type":"Album",
            "format":"Cd",
            "packaging":"JewelCase",
            "cover_contributors":[],
            "offchain_extension":null
        }"#;
        let r: Result<Release, _> = serde_json::from_str(bad);
        assert!(r.is_err(), "unknown version must be rejected");
    }
}
