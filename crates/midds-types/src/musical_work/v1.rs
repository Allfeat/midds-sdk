use bounded_collections::{BoundedVec, ConstU32};
use midds_traits::{
    Iswc, MiddsFormatError, MiddsString, OffchainHash, validate_iswc_format, validate_offchain_hash,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::language::Language;
use crate::shared::{BPM_MAX, BPM_MIN, YEAR_MAX, YEAR_MIN};

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
        // `number_of_voices` is optional, but a present value of 0 is
        // nonsensical (legacy front enforced a `>= 1` minimum).
        if let Some(n) = self.number_of_voices
            && n == 0
        {
            return Err(MiddsFormatError::OutOfBounds);
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
        if !(YEAR_MIN..=YEAR_MAX).contains(&self.creation_year) {
            return Err(MiddsFormatError::OutOfBounds);
        }
        if let Some(bpm) = self.bpm
            && !(BPM_MIN..=BPM_MAX).contains(&bpm)
        {
            return Err(MiddsFormatError::OutOfBounds);
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
                // A medley / mashup that references fewer than two source
                // works is not one. Min-cardinality ⇒ `OutOfBounds` (the
                // legacy front required >= 2; previously on-chain only
                // rejected the empty case).
                if refs.len() < 2 {
                    return Err(MiddsFormatError::OutOfBounds);
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

#[cfg(test)]
mod tests {
    //! Boundary tests for the range / cardinality rules stabilised per
    //! `docs/validation.md` §4. Identifier-structure and empty-mandatory
    //! paths are covered by `midds-fixtures` (`pathological`, proptest).
    use super::*;

    fn iswc() -> Iswc {
        BoundedVec::try_from(b"T0000000000".to_vec()).expect("11-byte ISWC")
    }

    /// Minimal payload that passes `validate_format` — each test mutates one
    /// field to probe a single rule.
    fn base() -> MusicalWorkV1 {
        MusicalWorkV1 {
            iswc: iswc(),
            title: BoundedVec::try_from(b"x".to_vec()).expect("1-byte title"),
            creation_year: 2000,
            instrumental: false,
            language: None,
            bpm: None,
            key: None,
            work_type: WorkType::Original,
            creators: BoundedVec::try_from(vec![Creator {
                role: CreatorRole::Composer,
                id: CreatorId::Ipi(
                    BoundedVec::try_from(b"123456789".to_vec()).expect("9-byte IPI"),
                ),
            }])
            .expect("1 creator"),
            classical_info: None,
            offchain_extension: None,
        }
    }

    #[test]
    fn base_is_valid() {
        base().validate_format().expect("base payload validates");
    }

    #[test]
    fn creation_year_bounds() {
        for (year, ok) in [
            (0u16, false),
            (1, true),
            (2000, true),
            (2999, true),
            (3000, false),
        ] {
            let mut w = base();
            w.creation_year = year;
            assert_eq!(
                w.validate_format().is_ok(),
                ok,
                "creation_year {year} expected ok={ok}"
            );
            if !ok {
                assert_eq!(w.validate_format(), Err(MiddsFormatError::OutOfBounds));
            }
        }
    }

    #[test]
    fn bpm_bounds() {
        for (bpm, ok) in [(19u16, false), (20, true), (300, true), (301, false)] {
            let mut w = base();
            w.bpm = Some(bpm);
            assert_eq!(
                w.validate_format().is_ok(),
                ok,
                "bpm {bpm} expected ok={ok}"
            );
        }
        // Absent BPM is always fine.
        let mut w = base();
        w.bpm = None;
        w.validate_format().expect("None bpm validates");
    }

    #[test]
    fn number_of_voices_must_be_positive_when_present() {
        let mut w = base();
        w.classical_info = Some(ClassicalInfo {
            opus: None,
            catalog_number: None,
            number_of_voices: Some(0),
        });
        assert_eq!(w.validate_format(), Err(MiddsFormatError::OutOfBounds));
        if let Some(ci) = w.classical_info.as_mut() {
            ci.number_of_voices = Some(1);
        }
        w.validate_format().expect("1 voice validates");
        if let Some(ci) = w.classical_info.as_mut() {
            ci.number_of_voices = None;
        }
        w.validate_format().expect("absent voice count validates");
    }

    #[test]
    fn medley_and_mashup_need_at_least_two_refs() {
        let refs = |n: usize| {
            WorkReferences::try_from(vec![iswc(); n]).expect("refs within WORK_REFERENCES_MAX")
        };
        for make in [
            WorkType::Medley as fn(WorkReferences) -> WorkType,
            WorkType::Mashup,
        ] {
            let mut w = base();
            w.work_type = make(refs(0));
            assert_eq!(w.validate_format(), Err(MiddsFormatError::OutOfBounds));
            w.work_type = make(refs(1));
            assert_eq!(w.validate_format(), Err(MiddsFormatError::OutOfBounds));
            w.work_type = make(refs(2));
            w.validate_format().expect(">= 2 refs validates");
        }
    }
}
