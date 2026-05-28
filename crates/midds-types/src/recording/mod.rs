pub mod v1;

pub use v1::{
    CONTRIBUTORS_MAX, Contributors, GENRES_MAX, Genre, Genres, PERFORMERS_MAX, PLACE_MAX_LEN,
    PRODUCERS_MAX, Performers, Place, Producers, ProductionPlaces, RecordingV1, RecordingVersion,
    TITLE_ALIASES_MAX, TitleAliases,
};

use midds_traits::{Isrc, Midds, MiddsFormatError};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Top-level versioned `Recording`. New on-chain versions are added as new
/// enum variants and migrated explicitly via `OnRuntimeUpgrade`.
///
/// JSON shape: internally tagged on a `"version"` field. `V1` serialises as
/// `{"version": "v1", "isrc": "…", "title": "…", …}` (flat). When `V2` is
/// added, existing consumers can dispatch on the tag without re-parsing.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "version", rename_all = "lowercase"))]
pub enum Recording {
    V1(RecordingV1),
}

impl Midds for Recording {
    const KIND: &'static str = "Recording";

    type Identifier = Isrc;

    fn identifier(&self) -> &Isrc {
        match self {
            Self::V1(v) => &v.isrc,
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
    use crate::shared::{Mode, MusicalKey, PartyId, PerformerId, PitchClass, WorkRef};
    use bounded_collections::BoundedVec;

    fn bv<const N: u32>(s: &[u8]) -> midds_traits::MiddsString<N> {
        BoundedVec::try_from(s.to_vec()).expect("bounded vec build")
    }

    fn sample_v1() -> RecordingV1 {
        RecordingV1 {
            isrc: bv(b"FRABC2412345"),
            title: bv(b"My Recording Title"),
            title_aliases: BoundedVec::default(),
            artist: PartyId::Ipi(bv::<11>(b"123456789")),
            work: WorkRef::Iswc(bv::<11>(b"T1234567890")),
            genres: BoundedVec::default(),
            record_year: Some(2024),
            version_type: Some(RecordingVersion::Original),
            performers: BoundedVec::default(),
            producers: BoundedVec::default(),
            duration: Some(213),
            bpm: Some(120),
            key: Some(MusicalKey {
                pitch: PitchClass::C,
                mode: Mode::Major,
            }),
            places: None,
            contributors: BoundedVec::default(),
            offchain_extension: None,
        }
    }

    #[test]
    fn roundtrip_encode_decode() {
        let original = Recording::V1(sample_v1());
        let encoded = original.encode();
        let decoded = Recording::decode(&mut &encoded[..]).expect("decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn identifier_returns_isrc() {
        let r = Recording::V1(sample_v1());
        assert_eq!(r.identifier(), &bv::<12>(b"FRABC2412345"));
    }

    #[test]
    fn kind_is_recording() {
        assert_eq!(Recording::KIND, "Recording");
    }

    #[test]
    fn validate_pass_minimal() {
        assert!(Recording::V1(sample_v1()).validate_format().is_ok());
    }

    #[test]
    fn validate_pass_full() {
        let mut v = sample_v1();
        v.title_aliases = BoundedVec::try_from(vec![bv(b"Alt Title"), bv(b"Titre FR")]).unwrap();
        v.artist = PartyId::Isni(bv::<16>(b"0000000121032683"));
        v.work = WorkRef::Midds(42);
        v.genres = BoundedVec::try_from(vec![Genre::Pop, Genre::Electronic]).unwrap();
        v.version_type = Some(RecordingVersion::RadioEdit);
        v.performers =
            BoundedVec::try_from(vec![PerformerId::Ipn(bv::<11>(b"12345678901"))]).unwrap();
        v.producers = BoundedVec::try_from(vec![bv::<16>(b"000000012103268X")]).unwrap();
        v.contributors =
            BoundedVec::try_from(vec![PartyId::Isni(bv::<16>(b"0000000121032683"))]).unwrap();
        v.places = Some(ProductionPlaces {
            recording: Some(bv(b"Abbey Road Studios")),
            mixing: Some(bv(b"Studio B")),
            mastering: Some(bv(b"Sterling Sound")),
        });
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));
        assert!(Recording::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_fails_invalid_isrc() {
        let mut v = sample_v1();
        v.isrc = bv(b"frABC2412345");
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_isrc_wrong_length() {
        let mut v = sample_v1();
        v.isrc = bv(b"FRABC241234");
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::OutOfBounds),
        );
    }

    #[test]
    fn validate_fails_empty_title() {
        let mut v = sample_v1();
        v.title = bv(b"");
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_empty_title_alias() {
        let mut v = sample_v1();
        v.title_aliases = BoundedVec::try_from(vec![bv(b"")]).unwrap();
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_invalid_artist_ipi() {
        let mut v = sample_v1();
        v.artist = PartyId::Ipi(bv::<11>(b"12345A789"));
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_invalid_work_iswc() {
        let mut v = sample_v1();
        v.work = WorkRef::Iswc(bv::<11>(b"X1234567890"));
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidIdentifierStructure),
        );
    }

    #[test]
    fn validate_pass_work_midds_ref() {
        let mut v = sample_v1();
        v.work = WorkRef::Midds(7);
        assert!(Recording::V1(v).validate_format().is_ok());
    }

    #[test]
    fn validate_fails_invalid_producer_isni() {
        let mut v = sample_v1();
        v.producers = BoundedVec::try_from(vec![bv::<16>(b"00000001A1032683")]).unwrap();
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_invalid_contributor() {
        let mut v = sample_v1();
        v.contributors =
            BoundedVec::try_from(vec![PartyId::Isni(bv::<16>(b"00000001A1032683"))]).unwrap();
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn validate_fails_empty_place() {
        let mut v = sample_v1();
        v.places = Some(ProductionPlaces {
            recording: Some(bv(b"")),
            mixing: None,
            mastering: None,
        });
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn validate_fails_empty_offchain_extension() {
        let mut v = sample_v1();
        v.offchain_extension = Some(bv(b""));
        assert_eq!(
            Recording::V1(v).validate_format(),
            Err(MiddsFormatError::EmptyMandatoryField),
        );
    }

    #[test]
    fn max_encoded_len_is_finite() {
        let max = <Recording as MaxEncodedLen>::max_encoded_len();
        assert!(max > 0);
        assert!(max < 8192, "Recording max_encoded_len = {max}");
    }
}

#[cfg(all(test, feature = "serde"))]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod json_tests {
    //! Snapshot tests pinning the `Recording` JSON wire format — the
    //! cross-service schema contract. A field rename or shape change must
    //! fail one of these on purpose, forcing a deliberate version bump.

    use super::*;
    use crate::shared::{Mode, MusicalKey, PartyId, PerformerId, PitchClass, WorkRef};
    use bounded_collections::BoundedVec;

    fn bv<const N: u32>(s: &[u8]) -> midds_traits::MiddsString<N> {
        BoundedVec::try_from(s.to_vec()).expect("bounded vec build")
    }

    fn minimal_v1() -> RecordingV1 {
        RecordingV1 {
            isrc: bv(b"FRABC2412345"),
            title: bv(b"My Recording Title"),
            title_aliases: BoundedVec::default(),
            artist: PartyId::Ipi(bv::<11>(b"123456789")),
            work: WorkRef::Iswc(bv::<11>(b"T1234567890")),
            genres: BoundedVec::default(),
            record_year: Some(2024),
            version_type: Some(RecordingVersion::Original),
            performers: BoundedVec::default(),
            producers: BoundedVec::default(),
            duration: Some(213),
            bpm: Some(120),
            key: Some(MusicalKey {
                pitch: PitchClass::C,
                mode: Mode::Major,
            }),
            places: None,
            contributors: BoundedVec::default(),
            offchain_extension: None,
        }
    }

    #[test]
    fn shape_minimal() {
        let json = serde_json::to_value(Recording::V1(minimal_v1())).unwrap();

        assert_eq!(json["version"], "v1");
        assert_eq!(json["isrc"], "FRABC2412345");
        assert_eq!(json["title"], "My Recording Title");
        assert_eq!(json["title_aliases"], serde_json::json!([]));
        assert_eq!(json["artist"]["Ipi"], "123456789");
        assert_eq!(json["work"]["Iswc"], "T1234567890");
        assert_eq!(json["genres"], serde_json::json!([]));
        assert_eq!(json["record_year"], 2024);
        assert_eq!(json["version_type"], "Original");
        assert_eq!(json["performers"], serde_json::json!([]));
        assert_eq!(json["producers"], serde_json::json!([]));
        assert_eq!(json["duration"], 213);
        assert_eq!(json["bpm"], 120);
        assert_eq!(json["key"]["pitch"], "C");
        assert_eq!(json["key"]["mode"], "Major");
        assert!(json["places"].is_null());
        assert_eq!(json["contributors"], serde_json::json!([]));
        assert!(json["offchain_extension"].is_null());
    }

    #[test]
    fn roundtrip_minimal() {
        let original = Recording::V1(minimal_v1());
        let raw = serde_json::to_string(&original).unwrap();
        let restored: Recording = serde_json::from_str(&raw).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn shape_full() {
        let mut v = minimal_v1();
        v.title_aliases = BoundedVec::try_from(vec![bv(b"Alt"), bv(b"Titre")]).unwrap();
        v.work = WorkRef::Midds(42);
        v.genres = BoundedVec::try_from(vec![Genre::HipHop, Genre::RnB]).unwrap();
        v.version_type = Some(RecordingVersion::ACapella);
        v.performers =
            BoundedVec::try_from(vec![PerformerId::Isni(bv::<16>(b"0000000121032683"))]).unwrap();
        v.producers = BoundedVec::try_from(vec![bv::<16>(b"000000012103268X")]).unwrap();
        v.contributors = BoundedVec::try_from(vec![PartyId::Ipi(bv::<11>(b"1234567890"))]).unwrap();
        v.places = Some(ProductionPlaces {
            recording: Some(bv(b"Abbey Road")),
            mixing: None,
            mastering: Some(bv(b"Sterling Sound")),
        });
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));

        let json = serde_json::to_value(Recording::V1(v)).unwrap();
        assert_eq!(json["title_aliases"], serde_json::json!(["Alt", "Titre"]));
        assert_eq!(json["work"]["Midds"], 42);
        assert_eq!(json["genres"], serde_json::json!(["HipHop", "RnB"]));
        assert_eq!(json["version_type"], "ACapella");
        assert_eq!(json["performers"][0]["Isni"], "0000000121032683");
        assert_eq!(json["producers"], serde_json::json!(["000000012103268X"]));
        assert_eq!(json["contributors"][0]["Ipi"], "1234567890");
        assert_eq!(json["places"]["recording"], "Abbey Road");
        assert!(json["places"]["mixing"].is_null());
        assert_eq!(json["places"]["mastering"], "Sterling Sound");
        assert_eq!(json["offchain_extension"], "bafkreigh2akiscaildc");
    }

    #[test]
    fn roundtrip_full() {
        let mut v = minimal_v1();
        v.work = WorkRef::Midds(99);
        v.genres = BoundedVec::try_from(vec![Genre::Jazz]).unwrap();
        v.places = Some(ProductionPlaces {
            recording: Some(bv(b"Studio One")),
            mixing: Some(bv(b"Studio Two")),
            mastering: None,
        });
        v.offchain_extension = Some(bv(b"bafkreigh2akiscaildc"));

        let original = Recording::V1(v);
        let raw = serde_json::to_string(&original).unwrap();
        let restored: Recording = serde_json::from_str(&raw).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn rejects_oversized_isrc_at_decode() {
        let bad = r#"{
            "version":"v1",
            "isrc":"FRABC2412345678",
            "title":"x",
            "title_aliases":[],
            "artist":{"Ipi":"123456789"},
            "work":{"Iswc":"T1234567890"},
            "genres":[],
            "record_year":null,
            "version_type":null,
            "performers":[],
            "producers":[],
            "duration":null,
            "bpm":null,
            "key":null,
            "places":null,
            "contributors":[],
            "offchain_extension":null
        }"#;
        let r: Result<Recording, _> = serde_json::from_str(bad);
        assert!(r.is_err(), "expected length-bound rejection");
    }

    #[test]
    fn rejects_unknown_version_tag() {
        let bad = r#"{
            "version":"v2",
            "isrc":"FRABC2412345",
            "title":"x",
            "title_aliases":[],
            "artist":{"Ipi":"123456789"},
            "work":{"Iswc":"T1234567890"},
            "genres":[],
            "record_year":null,
            "version_type":null,
            "performers":[],
            "producers":[],
            "duration":null,
            "bpm":null,
            "key":null,
            "places":null,
            "contributors":[],
            "offchain_extension":null
        }"#;
        let r: Result<Recording, _> = serde_json::from_str(bad);
        assert!(r.is_err(), "unknown version must be rejected");
    }
}
