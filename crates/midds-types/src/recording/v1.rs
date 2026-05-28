use bounded_collections::{BoundedVec, ConstU32};
use midds_traits::{
    Isni, Isrc, MiddsFormatError, MiddsString, OffchainHash, validate_isni_format,
    validate_isrc_format, validate_offchain_hash,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::shared::{
    BPM_MAX, BPM_MIN, MusicalKey, PartyId, PerformerId, Title, WorkRef, YEAR_MAX, YEAR_MIN,
};

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
/// Performers credited on the recording. Each carries an [`PerformerId`] —
/// preferably an IPN (issued by a performer CMO), falling back to IPI or ISNI
/// when the performer is not declared at such a CMO. Distinct from the
/// [`PartyId`] used for artist / creators / contributors: a non-performer
/// party cannot hold an IPN.
pub type Performers = BoundedVec<PerformerId, ConstU32<PERFORMERS_MAX>>;
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
/// optional / collection-empty-by-default. `artist` and `contributors` use
/// the shared [`PartyId`] (IPI / ISNI / both); `performers` use the
/// performer-specific [`PerformerId`] (IPN / IPI / ISNI); producers are
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
        if let Some(year) = self.record_year
            && !(YEAR_MIN..=YEAR_MAX).contains(&year)
        {
            return Err(MiddsFormatError::OutOfBounds);
        }
        if let Some(bpm) = self.bpm
            && !(BPM_MIN..=BPM_MAX).contains(&bpm)
        {
            return Err(MiddsFormatError::OutOfBounds);
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

#[cfg(test)]
mod tests {
    //! Boundary tests for the range rules stabilised per
    //! `docs/validation.md` §5. Identifier-structure and empty-mandatory
    //! paths are covered by `midds-fixtures` (`pathological`, proptest).
    use super::*;

    /// Minimal payload that passes `validate_format` — each test mutates one
    /// field to probe a single rule.
    fn base() -> RecordingV1 {
        RecordingV1 {
            isrc: BoundedVec::try_from(b"USRC17607839".to_vec()).expect("12-byte ISRC"),
            title: BoundedVec::try_from(b"x".to_vec()).expect("1-byte title"),
            title_aliases: BoundedVec::default(),
            artist: PartyId::Ipi(BoundedVec::try_from(b"123456789".to_vec()).expect("9-byte IPI")),
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
        }
    }

    #[test]
    fn base_is_valid() {
        base().validate_format().expect("base payload validates");
    }

    #[test]
    fn record_year_bounds_when_present() {
        let mut r = base();
        r.record_year = None;
        r.validate_format().expect("absent record_year validates");
        for (year, ok) in [(0u16, false), (1, true), (2999, true), (3000, false)] {
            let mut r = base();
            r.record_year = Some(year);
            assert_eq!(
                r.validate_format().is_ok(),
                ok,
                "record_year {year} expected ok={ok}"
            );
            if !ok {
                assert_eq!(r.validate_format(), Err(MiddsFormatError::OutOfBounds));
            }
        }
    }

    #[test]
    fn bpm_bounds_when_present() {
        for (bpm, ok) in [(19u16, false), (20, true), (300, true), (301, false)] {
            let mut r = base();
            r.bpm = Some(bpm);
            assert_eq!(
                r.validate_format().is_ok(),
                ok,
                "bpm {bpm} expected ok={ok}"
            );
        }
        let mut r = base();
        r.bpm = None;
        r.validate_format().expect("None bpm validates");
    }
}
