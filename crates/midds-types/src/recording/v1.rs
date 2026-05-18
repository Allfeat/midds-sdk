use bounded_collections::{BoundedVec, ConstU32};
use midds_traits::{
    Isni, Isrc, MiddsFormatError, MiddsString, OffchainHash, validate_isni_format,
    validate_isrc_format, validate_offchain_hash,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::shared::{MusicalKey, PartyId, Title, WorkRef};

/// Maximum number of alternative titles attached to a recording.
pub const TITLE_ALIASES_MAX: u32 = 8;
/// Maximum number of genres tagging a recording.
pub const GENRES_MAX: u32 = 8;
/// Maximum number of performers (band members, orchestra…) credited.
pub const PERFORMERS_MAX: u32 = 64;
/// Maximum number of producers credited.
pub const PRODUCERS_MAX: u32 = 8;
/// Maximum number of other contributors credited.
pub const CONTRIBUTORS_MAX: u32 = 32;
/// Maximum byte length of a production place name (studio, city…).
pub const PLACE_MAX_LEN: u32 = 128;

/// Alternative / localized titles for the recording.
pub type TitleAliases = BoundedVec<Title, ConstU32<TITLE_ALIASES_MAX>>;
/// Genres tagging the recording.
pub type Genres = BoundedVec<Genre, ConstU32<GENRES_MAX>>;
/// Performers credited on the recording (IPI or ISNI each).
pub type Performers = BoundedVec<PartyId, ConstU32<PERFORMERS_MAX>>;
/// Producers credited on the recording. ISNI only — producers are legal /
/// natural persons identified by ISNI in industry metadata, not IPI.
pub type Producers = BoundedVec<Isni, ConstU32<PRODUCERS_MAX>>;
/// Other contributors credited on the recording (IPI or ISNI each).
pub type Contributors = BoundedVec<PartyId, ConstU32<CONTRIBUTORS_MAX>>;
/// A free-text production place (studio name, city…).
pub type Place = MiddsString<PLACE_MAX_LEN>;

/// Top-level musical genre. Closed enum: stored as a single SCALE tag byte
/// on-chain (cheaper and indexable, vs an unbounded free-text string).
/// Flat industry-aligned taxonomy; finer granularity is deliberately left
/// to a future payload version rather than a sub-genre tree (which would
/// cost a byte and force a version bump on every taxonomy tweak).
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
pub enum Genre {
    Pop,
    Rock,
    HipHop,
    RnB,
    Electronic,
    Dance,
    Jazz,
    Blues,
    Classical,
    Country,
    Folk,
    Metal,
    Punk,
    Reggae,
    Latin,
    World,
    Soul,
    Funk,
    Gospel,
    Soundtrack,
    Ambient,
    Experimental,
    Children,
    SpokenWord,
    Other,
}

/// Editorial version of a recording, aligned with the DDEX
/// `EditionType` / version conventions used across the industry.
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
pub enum RecordingVersion {
    /// The original studio version.
    Original,
    /// Shortened cut for broadcast.
    RadioEdit,
    /// Lengthened cut.
    Extended,
    /// Reworked by a remixer.
    Remix,
    /// Recorded in front of an audience.
    Live,
    /// Unplugged / acoustic rendition.
    Acoustic,
    /// Vocals removed.
    Instrumental,
    /// Instruments removed.
    ACapella,
    /// Backing track for sing-along.
    Karaoke,
    /// Pre-release / working version.
    Demo,
    /// Re-recording of an earlier release.
    ReRecorded,
    /// Otherwise edited (clean, censored…).
    Edited,
    /// Cover of another artist's recording.
    Cover,
}

/// Where the recording was recorded, mixed and mastered. Every sub-field is
/// optional; the wrapping `Option` on `RecordingV1` elides the whole block
/// when nothing is known.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProductionPlaces {
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub recording: Option<Place>,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub mixing: Option<Place>,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub mastering: Option<Place>,
}

impl ProductionPlaces {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        if let Some(p) = &self.recording
            && p.is_empty()
        {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        if let Some(p) = &self.mixing
            && p.is_empty()
        {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        if let Some(p) = &self.mastering
            && p.is_empty()
        {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        Ok(())
    }
}

/// First on-chain version of a `Recording`.
///
/// Mandatory fields: `isrc`, `title`, `artist`, `work`. Everything else is
/// optional / collection-empty-by-default. Identity references (artist,
/// performers, contributors) reuse the shared [`PartyId`]; producers are
/// ISNI-only; the recorded work is referenced by on-chain id or ISWC via
/// the shared [`WorkRef`].
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RecordingV1 {
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub isrc: Isrc,
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub title: Title,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_vec")
    )]
    pub title_aliases: TitleAliases,
    pub artist: PartyId,
    pub work: WorkRef,
    pub genres: Genres,
    pub record_year: Option<u16>,
    pub version_type: Option<RecordingVersion>,
    pub performers: Performers,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_vec")
    )]
    pub producers: Producers,
    /// Recording length in whole seconds. `u32` covers any conceivable
    /// single recording (≈136 years) while staying 4 bytes.
    pub duration: Option<u32>,
    pub bpm: Option<u16>,
    pub key: Option<MusicalKey>,
    pub places: Option<ProductionPlaces>,
    pub contributors: Contributors,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub offchain_extension: Option<OffchainHash>,
}

impl RecordingV1 {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        validate_isrc_format(&self.isrc)?;
        if self.title.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        for alias in &self.title_aliases {
            if alias.is_empty() {
                return Err(MiddsFormatError::EmptyMandatoryField);
            }
        }
        self.artist.validate_format()?;
        self.work.validate_format()?;
        for p in &self.performers {
            p.validate_format()?;
        }
        for prod in &self.producers {
            validate_isni_format(prod)?;
        }
        for c in &self.contributors {
            c.validate_format()?;
        }
        if let Some(places) = &self.places {
            places.validate_format()?;
        }
        if let Some(h) = &self.offchain_extension {
            validate_offchain_hash(h)?;
        }
        Ok(())
    }
}
