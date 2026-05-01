use frame_support::{BoundedVec, traits::ConstU32};
use midds_traits::{
    Ipi, Isni, Iswc, MiddsFormatError, MiddsString, OffchainHash, validate_ipi_format,
    validate_isni_format, validate_iswc_format, validate_offchain_hash,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::language::Language;

/// Maximum byte length of a work title.
pub const TITLE_MAX_LEN: u32 = 256;
/// Maximum number of creators attached to a work.
pub const CREATORS_MAX: u32 = 32;
/// Maximum byte length of an opus designation (e.g. "Op. 27 No. 2").
pub const OPUS_MAX_LEN: u32 = 32;
/// Maximum byte length of a thematic catalogue number (e.g. "BWV 565", "K. 545").
pub const CATALOG_NUMBER_MAX_LEN: u32 = 32;
/// Maximum number of source-work references carried by Medley / Mashup.
pub const WORK_REFERENCES_MAX: u32 = 32;

pub type Title = MiddsString<TITLE_MAX_LEN>;
pub type Creators = BoundedVec<Creator, ConstU32<CREATORS_MAX>>;
pub type Opus = MiddsString<OPUS_MAX_LEN>;
pub type CatalogNumber = MiddsString<CATALOG_NUMBER_MAX_LEN>;
pub type WorkReferences = BoundedVec<Iswc, ConstU32<WORK_REFERENCES_MAX>>;

/// One of the 12 chromatic pitch classes.
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
pub enum PitchClass {
    C,
    CSharp,
    D,
    DSharp,
    E,
    F,
    FSharp,
    G,
    GSharp,
    A,
    ASharp,
    B,
}

/// Diatonic mode. V1 keeps it minimal; modal music (Dorian, Phrygian, …) can
/// be modelled by extending later through a fresh `MusicalWorkV2`.
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
pub enum Mode {
    Major,
    Minor,
}

/// Diatonic key of a work: pitch class plus mode.
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
pub struct MusicalKey {
    pub pitch: PitchClass,
    pub mode: Mode,
}

/// Top-level classification of a musical work, with the source ISWCs the
/// derived variants reference.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WorkType {
    /// Standalone work — no source reference.
    Original,
    /// Several source works performed back-to-back.
    Medley(WorkReferences),
    /// Several source works combined into a single new work.
    Mashup(WorkReferences),
    /// Derivative work based on exactly one source.
    Adaptation(Iswc),
}

/// Role attributed to a creator within a work.
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
pub enum CreatorRole {
    Author,
    Composer,
    Arranger,
    Adapter,
    Publisher,
}

/// External identifier of a creator: either an IPI or an ISNI code.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CreatorId {
    Ipi(Ipi),
    Isni(Isni),
}

impl CreatorId {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        match self {
            Self::Ipi(v) => validate_ipi_format(v),
            Self::Isni(v) => validate_isni_format(v),
        }
    }
}

/// A creator attached to a work, with their role and external identifier.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Creator {
    pub role: CreatorRole,
    pub id: CreatorId,
}

impl Creator {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        self.id.validate_format()
    }
}

/// Classical-music-specific metadata. Every sub-field is optional (works are
/// rarely fully catalogued); the wrapping `Option` on `MusicalWorkV1` further
/// elides this block entirely for non-classical works.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClassicalInfo {
    pub opus: Option<Opus>,
    pub catalog_number: Option<CatalogNumber>,
    pub number_of_voices: Option<u16>,
}

impl ClassicalInfo {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        if let Some(o) = &self.opus
            && o.is_empty()
        {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        if let Some(c) = &self.catalog_number
            && c.is_empty()
        {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        Ok(())
    }
}

/// First on-chain version of a `MusicalWork`.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MusicalWorkV1 {
    pub iswc: Iswc,
    pub title: Title,
    pub creation_year: u16,
    pub instrumental: bool,
    pub language: Option<Language>,
    pub bpm: Option<u16>,
    pub key: Option<MusicalKey>,
    pub work_type: WorkType,
    pub creators: Creators,
    pub classical_info: Option<ClassicalInfo>,
    pub offchain_extension: Option<OffchainHash>,
}

impl MusicalWorkV1 {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        validate_iswc_format(&self.iswc)?;
        if self.title.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        if self.creators.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        for c in &self.creators {
            c.validate_format()?;
        }
        match &self.work_type {
            WorkType::Original => {}
            WorkType::Medley(refs) | WorkType::Mashup(refs) => {
                if refs.is_empty() {
                    return Err(MiddsFormatError::EmptyMandatoryField);
                }
                for r in refs {
                    validate_iswc_format(r)?;
                }
            }
            WorkType::Adaptation(iswc) => validate_iswc_format(iswc)?,
        }
        if let Some(ci) = &self.classical_info {
            ci.validate_format()?;
        }
        if let Some(h) = &self.offchain_extension {
            validate_offchain_hash(h)?;
        }
        Ok(())
    }
}
