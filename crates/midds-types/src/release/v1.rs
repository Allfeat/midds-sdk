use bounded_collections::{BoundedVec, ConstU32};
use midds_traits::{
    Isni, MiddsFormatError, MiddsString, OffchainHash, Upc, validate_isni_format,
    validate_offchain_hash, validate_upc_format,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::Country;
use crate::shared::{PartyId, RecordingRef, Title};

/// Maximum number of alternative / localized titles on a release.
pub const TITLE_ALIASES_MAX: u32 = 8;
/// Maximum number of recordings (tracks) referenced by a release. Sized for
/// large box sets; the per-track [`RecordingRef`] is at most 14 SCALE bytes.
pub const TRACKS_MAX: u32 = 256;
/// Maximum number of producers / labels credited on a release.
pub const PRODUCERS_MAX: u32 = 16;
/// Maximum number of cover-artwork contributors credited.
pub const COVER_CONTRIBUTORS_MAX: u32 = 16;
/// Maximum byte length of a label / release catalog number (e.g. "88985456971").
pub const CATALOG_NUMBER_MAX_LEN: u32 = 32;
/// Maximum byte length of the distributor name.
pub const DISTRIBUTOR_NAME_MAX_LEN: u32 = 128;
/// Maximum byte length of one cover-contributor name.
pub const COVER_CONTRIBUTOR_NAME_MAX_LEN: u32 = 128;

/// Alternative / localized titles for the release.
pub type TitleAliases = BoundedVec<Title, ConstU32<TITLE_ALIASES_MAX>>;
/// Ordered tracklist: each entry references a recording on-chain (MIDDS id)
/// or externally (ISRC). Position in the vector is the track order.
pub type Tracks = BoundedVec<RecordingRef, ConstU32<TRACKS_MAX>>;
/// A label / release catalog number (free-text within the bound).
pub type CatalogNumber = MiddsString<CATALOG_NUMBER_MAX_LEN>;
/// Producers credited on the release, each with their own catalog number.
pub type Producers = BoundedVec<Producer, ConstU32<PRODUCERS_MAX>>;
/// Free-text distributor name.
pub type DistributorName = MiddsString<DISTRIBUTOR_NAME_MAX_LEN>;
/// One cover-artwork contributor's name (free text).
pub type CoverContributorName = MiddsString<COVER_CONTRIBUTOR_NAME_MAX_LEN>;
/// Cover-artwork contributor names (photographers, designers, …).
pub type CoverContributors = BoundedVec<CoverContributorName, ConstU32<COVER_CONTRIBUTORS_MAX>>;

/// A producer / label credited on the release. Producers are legal persons
/// identified by ISNI in industry metadata (not IPI), each carrying the
/// catalog number under which they issued the release — co-editions across
/// multiple labels each keep their own number.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Producer {
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub isni: Isni,
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub catalog_number: CatalogNumber,
}

impl Producer {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        validate_isni_format(&self.isni)?;
        if self.catalog_number.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        Ok(())
    }
}

/// Editorial status of a release, aligned with the MusicBrainz / DDEX
/// release-status conventions used across the industry.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReleaseStatus {
    /// Officially sanctioned commercial or non-commercial release.
    Official,
    /// Promotional copy (radio, press, …) not sold to the public.
    Promotional,
    /// Unofficial / unsanctioned release.
    Bootleg,
    /// Placeholder for a release known to exist but not yet detailed.
    PseudoRelease,
    /// Officially released then pulled from distribution.
    Withdrawn,
    /// Announced then cancelled before release.
    Cancelled,
    /// Any status not captured above.
    Other,
}

/// Primary editorial type of a release. Flattened from the MusicBrainz
/// release-group primary + secondary taxonomy — finer granularity is left
/// to a future payload version rather than a type tree.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReleaseType {
    Album,
    Single,
    Ep,
    Broadcast,
    Compilation,
    Soundtrack,
    Live,
    Remix,
    Mixtape,
    Demo,
    Other,
}

/// Physical or digital format the release is issued on. Aligned with the
/// MusicBrainz medium-format list, kept to the commercially common values.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReleaseFormat {
    Cd,
    Vinyl,
    Cassette,
    DigitalDownload,
    Streaming,
    Dvd,
    BluRay,
    Sacd,
    MiniDisc,
    ReelToReel,
    Other,
}

/// Physical packaging of the release, aligned with the MusicBrainz
/// packaging list.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReleasePackaging {
    /// No packaging (typically digital releases).
    None,
    JewelCase,
    SlimJewelCase,
    Digipak,
    CardboardSleeve,
    Gatefold,
    KeepCase,
    Box,
    Other,
}

/// Calendar release date. Strict by product decision: year, month and day
/// are all mandatory and range-checked on-chain (month `1..=12`, day
/// `1..=31`). Per the format-only validation philosophy this is a structural
/// range check, not a full calendar check (Feb-30 is accepted on-chain;
/// `midds-validate` is the place for stricter rules).
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    TypeInfo,
    MaxEncodedLen,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReleaseDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl ReleaseDate {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        if !(1..=12).contains(&self.month) || !(1..=31).contains(&self.day) {
            return Err(MiddsFormatError::OutOfBounds);
        }
        Ok(())
    }
}

/// First on-chain version of a `Release`.
///
/// Mandatory fields: `upc`, `title`, `artist`, `tracks` (non-empty),
/// `status`, `release_date`, `country`, `distributor_name`, `release_type`,
/// `format`, `packaging`. The rest are optional / collection-empty-by-default.
/// Identity (artist) reuses the shared [`PartyId`]; the tracklist reuses the
/// shared [`RecordingRef`]; producers are ISNI-keyed pairs.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReleaseV1 {
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub upc: Upc,
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub title: Title,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_vec")
    )]
    pub title_aliases: TitleAliases,
    pub artist: PartyId,
    pub tracks: Tracks,
    pub producers: Producers,
    pub status: ReleaseStatus,
    pub release_date: ReleaseDate,
    pub country: Country,
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub distributor_name: DistributorName,
    pub release_type: ReleaseType,
    pub format: ReleaseFormat,
    pub packaging: ReleasePackaging,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_vec")
    )]
    pub cover_contributors: CoverContributors,
    /// Off-chain extension hash (opaque on-chain; CIDv1 by client
    /// convention). The standard MIDDS extensibility hook — present on every
    /// payload type even though it is not a domain field.
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub offchain_extension: Option<OffchainHash>,
}

impl ReleaseV1 {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        validate_upc_format(&self.upc)?;
        if self.title.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        for alias in &self.title_aliases {
            if alias.is_empty() {
                return Err(MiddsFormatError::EmptyMandatoryField);
            }
        }
        self.artist.validate_format()?;
        if self.tracks.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        for t in &self.tracks {
            t.validate_format()?;
        }
        for (i, a) in self.tracks.iter().enumerate() {
            if self.tracks.iter().skip(i + 1).any(|b| b == a) {
                return Err(MiddsFormatError::CrossFieldInconsistency);
            }
        }
        for p in &self.producers {
            p.validate_format()?;
        }
        self.release_date.validate_format()?;
        if self.distributor_name.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        for c in &self.cover_contributors {
            if c.is_empty() {
                return Err(MiddsFormatError::EmptyMandatoryField);
            }
        }
        if let Some(h) = &self.offchain_extension {
            validate_offchain_hash(h)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Boundary tests for the tracklist-uniqueness rule stabilised per
    //! `docs/validation.md` §6/§8. Identifier-structure and empty-mandatory
    //! paths are covered by `midds-fixtures` (`pathological`, proptest).
    use super::*;

    /// Minimal payload that passes `validate_format` — each test mutates
    /// `tracks` only.
    fn base() -> ReleaseV1 {
        ReleaseV1 {
            upc: BoundedVec::try_from(b"000000000000".to_vec()).expect("12-byte UPC-A"),
            title: BoundedVec::try_from(b"x".to_vec()).expect("1-byte title"),
            title_aliases: BoundedVec::default(),
            artist: PartyId::Ipi(BoundedVec::try_from(b"123456789".to_vec()).expect("9-byte IPI")),
            tracks: BoundedVec::try_from(vec![RecordingRef::Midds(1)]).expect("1 track"),
            producers: BoundedVec::default(),
            status: ReleaseStatus::Official,
            release_date: ReleaseDate {
                year: 2000,
                month: 1,
                day: 1,
            },
            country: Country::Us,
            distributor_name: BoundedVec::try_from(b"x".to_vec()).expect("1-byte distributor"),
            release_type: ReleaseType::Album,
            format: ReleaseFormat::Cd,
            packaging: ReleasePackaging::None,
            cover_contributors: BoundedVec::default(),
            offchain_extension: None,
        }
    }

    fn isrc(s: &[u8]) -> RecordingRef {
        RecordingRef::Isrc(BoundedVec::try_from(s.to_vec()).expect("12-byte ISRC"))
    }

    #[test]
    fn base_is_valid() {
        base().validate_format().expect("base payload validates");
    }

    #[test]
    fn distinct_tracks_validate() {
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![
            RecordingRef::Midds(1),
            RecordingRef::Midds(2),
            isrc(b"USRC17607839"),
            isrc(b"GBAYE0601477"),
        ])
        .expect("4 distinct tracks");
        r.validate_format().expect("distinct tracklist validates");
    }

    #[test]
    fn duplicate_midds_track_rejected() {
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![RecordingRef::Midds(7), RecordingRef::Midds(7)])
            .expect("2 tracks");
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency),
        );
    }

    #[test]
    fn duplicate_isrc_track_rejected() {
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![
            RecordingRef::Midds(1),
            isrc(b"USRC17607839"),
            isrc(b"USRC17607839"),
        ])
        .expect("3 tracks");
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency),
        );
    }
}
