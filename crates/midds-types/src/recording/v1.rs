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
/// Maximum number of featured artists ("feat.") credited on the recording.
pub const FEATURING_MAX: u32 = 16;
/// Maximum number of performers (band members, orchestra…) credited.
pub const PERFORMERS_MAX: u32 = 64;
/// Maximum number of instruments credited to a single performer. A
/// multi-instrumentalist ("guitar, vocals") lists each; a single-instrument
/// credit is a one-element list, and an unknown instrument is the empty list
/// (no minimum cardinality).
pub const INSTRUMENTS_PER_PERFORMER_MAX: u32 = 8;
/// Maximum number of producers credited.
pub const PRODUCERS_MAX: u32 = 8;
/// Maximum number of other contributors credited.
pub const CONTRIBUTORS_MAX: u32 = 32;
/// Maximum byte length of a production place name (studio, city…).
pub const PLACE_MAX_LEN: u32 = 128;

/// Alternative / localized titles for the recording.
pub type TitleAliases = BoundedVec<Title, ConstU32<TITLE_ALIASES_MAX>>;
/// Featured artists ("feat.") credited on the recording. Each is a
/// [`PartyId`] (IPI / ISNI / both) exactly like the main `artist`: a featured
/// artist is credited in the same sense as the lead, not as a session
/// [`PerformerId`].
pub type Featuring = BoundedVec<PartyId, ConstU32<FEATURING_MAX>>;
/// Instruments a single [`Performer`] played on the recording.
pub type PerformerInstruments = BoundedVec<Instrument, ConstU32<INSTRUMENTS_PER_PERFORMER_MAX>>;
/// Performers credited on the recording. Each is a [`Performer`]: a
/// performer-specific [`PerformerId`] (IPN preferred, IPI / ISNI fallback)
/// paired with the instrument(s) they played.
pub type Performers = BoundedVec<Performer, ConstU32<PERFORMERS_MAX>>;
/// Producers credited on the recording. ISNI only — producers are legal /
/// natural persons identified by ISNI in industry metadata, not IPI.
pub type Producers = BoundedVec<Isni, ConstU32<PRODUCERS_MAX>>;
/// Other contributors credited on the recording (IPI or ISNI each).
pub type Contributors = BoundedVec<PartyId, ConstU32<CONTRIBUTORS_MAX>>;
/// A free-text production place (studio name, city…).
pub type Place = MiddsString<PLACE_MAX_LEN>;

/// Top-level musical genre. Closed enum: stored as a single SCALE tag byte
/// on-chain (cheaper and indexable, vs an unbounded free-text string).
/// Flat industry-aligned taxonomy: the same enum backs both the primary
/// `genre` and the optional `sub_genre` refinement. Finer *hierarchical*
/// granularity (a genre→sub-genre tree) is deliberately left to a future
/// payload version rather than baked into the variant set.
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
    /// Otherwise edited (censored, trimmed…). For the parental-advisory
    /// clean cut prefer the dedicated [`RecordingVersion::Clean`].
    Edited,
    /// Cover of another artist's recording.
    Cover,
    /// Explicit content removed or masked — the "clean" radio / retail edit
    /// (the parental-advisory counterpart to an explicit release). Appended
    /// after `Cover` so every pre-existing V1 tag byte is unchanged.
    Clean,
}

/// Instrument played by a [`Performer`] on a recording.
///
/// Closed enum stored as a single SCALE tag byte (like [`Genre`]): cheap and
/// indexable versus a free-text string. The taxonomy is broad and grouped by
/// instrument family, with generic catch-alls (`Vocals`, `Guitar`,
/// `Keyboards`, `Strings`, `Percussion`) for when only the family is known
/// and a final `Other` for anything outside the list. New instruments are
/// **appended** in a future payload version — never reordered or removed — so
/// existing tag bytes stay stable.
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
pub enum Instrument {
    // --- Vocals ---
    /// Vocals, family unspecified.
    Vocals,
    LeadVocals,
    BackingVocals,
    Choir,
    // --- Keyboards ---
    Piano,
    ElectricPiano,
    Organ,
    Harpsichord,
    Synthesizer,
    Accordion,
    Celesta,
    Melodica,
    /// Keyboards, type unspecified.
    Keyboards,
    // --- Plucked strings ---
    AcousticGuitar,
    ElectricGuitar,
    BassGuitar,
    ClassicalGuitar,
    /// Guitar, type unspecified.
    Guitar,
    Banjo,
    Mandolin,
    Ukulele,
    Harp,
    Sitar,
    Lute,
    Balalaika,
    Oud,
    // --- Bowed strings ---
    Violin,
    Viola,
    Cello,
    DoubleBass,
    /// Bowed-string section / unspecified.
    Strings,
    // --- Woodwinds ---
    Flute,
    Piccolo,
    Clarinet,
    Oboe,
    Bassoon,
    Saxophone,
    Recorder,
    EnglishHorn,
    Harmonica,
    Bagpipes,
    PanFlute,
    // --- Brass ---
    Trumpet,
    Cornet,
    Flugelhorn,
    Trombone,
    FrenchHorn,
    Tuba,
    Euphonium,
    // --- Pitched / mallet percussion ---
    Marimba,
    Xylophone,
    Vibraphone,
    Glockenspiel,
    Timpani,
    Steelpan,
    // --- Percussion & drums ---
    DrumKit,
    SnareDrum,
    BassDrum,
    Cymbals,
    Tambourine,
    Congas,
    Bongos,
    Djembe,
    Tabla,
    Cajon,
    Triangle,
    Castanets,
    Maracas,
    Timbales,
    Cowbell,
    HandClaps,
    /// Percussion, instrument unspecified.
    Percussion,
    // --- Electronic / production ---
    DrumMachine,
    Sampler,
    Sequencer,
    Turntables,
    Theremin,
    // --- Fallback ---
    /// Any instrument outside the list above.
    Other,
}

/// A performer credited on the recording: a performer-specific identifier
/// plus the instrument(s) they played.
///
/// `id` is a [`PerformerId`] (IPN preferred — the performer-CMO registry —
/// with IPI / ISNI as the fallback for performers not declared at such a CMO).
/// `instruments` may be empty (instrument unknown), carry one entry, or list
/// several for a multi-instrumentalist, up to [`INSTRUMENTS_PER_PERFORMER_MAX`].
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Performer {
    pub id: PerformerId,
    pub instruments: PerformerInstruments,
}

impl Performer {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        // The identifier is the only format-checkable part; instrument
        // membership is guaranteed structurally by the closed enum and the
        // empty list is a valid "instrument unknown".
        self.id.validate_format()
    }
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
/// optional / collection-empty-by-default. `artist`, `featuring` and
/// `contributors` use the shared [`PartyId`] (IPI / ISNI / both);
/// `performers` are [`Performer`]s pairing the performer-specific
/// [`PerformerId`] (IPN / IPI / ISNI) with the instrument(s) played;
/// producers are ISNI-only; the recorded work is referenced by on-chain id
/// or ISWC via the shared [`WorkRef`].
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
    pub featuring: Featuring,
    pub work: WorkRef,
    /// Primary genre of the recording. Optional — an unknown genre is `None`
    /// (use [`Genre::Other`] for "known but outside the taxonomy").
    pub genre: Option<Genre>,
    /// Optional secondary genre, drawn from the same [`Genre`] taxonomy as
    /// `genre` — a refinement of the primary tagging, not a separate scale.
    /// A sub-genre may only be present alongside a primary `genre`
    /// (enforced by [`RecordingV1::validate_format`]).
    pub sub_genre: Option<Genre>,
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
        // A sub-genre refines a primary genre; it cannot stand alone.
        if self.sub_genre.is_some() && self.genre.is_none() {
            return Err(MiddsFormatError::CrossFieldInconsistency);
        }
        self.artist.validate_format()?;
        for f in &self.featuring {
            f.validate_format()?;
        }
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
            featuring: BoundedVec::default(),
            work: WorkRef::Midds(0),
            genre: None,
            sub_genre: None,
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

    #[test]
    fn genre_and_sub_genre_consistency() {
        // genre absent + sub_genre absent → ok (the base case).
        base().validate_format().expect("no genre, no sub-genre");

        // genre present alone → ok.
        let mut r = base();
        r.genre = Some(Genre::Jazz);
        r.validate_format().expect("primary genre alone validates");

        // genre + sub_genre both present → ok.
        let mut r = base();
        r.genre = Some(Genre::Jazz);
        r.sub_genre = Some(Genre::Blues);
        r.validate_format().expect("genre + sub-genre validates");

        // sub_genre without a primary genre → rejected.
        let mut r = base();
        r.genre = None;
        r.sub_genre = Some(Genre::Blues);
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency),
            "a lone sub-genre is rejected"
        );
    }

    #[test]
    fn featuring_validates_each_party() {
        // A well-formed featured artist passes; a malformed one is rejected
        // through the same `PartyId` structure check as the main artist.
        let mut r = base();
        r.featuring = BoundedVec::try_from(vec![PartyId::Isni(
            BoundedVec::try_from(b"0000000121032683".to_vec()).expect("16-byte ISNI"),
        )])
        .expect("one featured artist");
        r.validate_format()
            .expect("valid featured artist validates");

        r.featuring = BoundedVec::try_from(vec![PartyId::Ipi(
            BoundedVec::try_from(b"12345A789".to_vec()).expect("9 bytes"),
        )])
        .expect("one featured artist");
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::InvalidCharset),
            "non-digit IPI in a featured artist is rejected"
        );
    }

    #[test]
    fn performer_instruments_validate() {
        // A performer carrying several instruments validates (instrument
        // membership is structural); the performer id is still format-checked.
        let mut r = base();
        r.performers = BoundedVec::try_from(vec![Performer {
            id: PerformerId::Ipn(
                BoundedVec::try_from(b"12345678".to_vec()).expect("8-byte IPN"),
            ),
            instruments: BoundedVec::try_from(vec![
                Instrument::ElectricGuitar,
                Instrument::LeadVocals,
            ])
            .expect("two instruments"),
        }])
        .expect("one performer");
        r.validate_format()
            .expect("performer with instruments validates");

        r.performers = BoundedVec::try_from(vec![Performer {
            id: PerformerId::Isni(
                BoundedVec::try_from(b"00000001A1032683".to_vec()).expect("16 bytes"),
            ),
            instruments: BoundedVec::default(),
        }])
        .expect("one performer");
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::InvalidCharset),
            "malformed performer ISNI is rejected even with no instruments"
        );
    }
}
