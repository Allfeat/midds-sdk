use bounded_collections::{BoundedVec, ConstU32};
use midds_traits::{
    Iswc, MiddsFormatError, MiddsString, OffchainHash, validate_iswc_format, validate_offchain_hash,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::language::Language;

// Shared types live in `crate::shared` so `MusicalWork` and `Recording`
// encode them identically. Re-exported here (and through `mod.rs` →
// `lib.rs`) so `midds_types::{Title, MusicalKey, Mode, PitchClass,
// TITLE_MAX_LEN, CreatorId}` stay valid for existing consumers. `CreatorId`
// is now an alias of the generalised `shared::PartyId`.
pub use crate::shared::PartyId as CreatorId;
pub use crate::shared::{Mode, MusicalKey, PitchClass, TITLE_MAX_LEN, Title};

/// Maximum number of creators attached to a work.
pub const CREATORS_MAX: u32 = 32;
/// Maximum byte length of an opus designation (e.g. "Op. 27 No. 2").
pub const OPUS_MAX_LEN: u32 = 32;
/// Maximum byte length of a thematic catalogue number (e.g. "BWV 565", "K. 545").
pub const CATALOG_NUMBER_MAX_LEN: u32 = 32;
/// Maximum number of source-work references carried by Medley / Mashup.
pub const WORK_REFERENCES_MAX: u32 = 32;

pub type Creators = BoundedVec<Creator, ConstU32<CREATORS_MAX>>;
pub type Opus = MiddsString<OPUS_MAX_LEN>;
pub type CatalogNumber = MiddsString<CATALOG_NUMBER_MAX_LEN>;
pub type WorkReferences = BoundedVec<Iswc, ConstU32<WORK_REFERENCES_MAX>>;

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
    Medley(
        #[cfg_attr(
            feature = "serde",
            serde(with = "midds_traits::serde_helpers::ascii_vec")
        )]
        WorkReferences,
    ),
    /// Several source works combined into a single new work.
    Mashup(
        #[cfg_attr(
            feature = "serde",
            serde(with = "midds_traits::serde_helpers::ascii_vec")
        )]
        WorkReferences,
    ),
    /// Derivative work based on exactly one source.
    Adaptation(
        #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))] Iswc,
    ),
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
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub opus: Option<Opus>,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
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
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub iswc: Iswc,
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub title: Title,
    pub creation_year: u16,
    pub instrumental: bool,
    pub language: Option<Language>,
    pub bpm: Option<u16>,
    pub key: Option<MusicalKey>,
    pub work_type: WorkType,
    pub creators: Creators,
    pub classical_info: Option<ClassicalInfo>,
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
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
