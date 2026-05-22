use bounded_collections::{BoundedBTreeSet, BoundedVec, ConstU32};
use midds_traits::{
    Iswc, MiddsFormatError, MiddsString, OffchainHash, validate_iswc_format, validate_offchain_hash,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::language::Language;
use crate::shared::{BPM_MAX, BPM_MIN, PartyId, YEAR_MAX, YEAR_MIN};

pub use crate::shared::{Mode, MusicalKey, PitchClass, TITLE_MAX_LEN, Title};

/// Maximum number of creators attached to a work.
pub const CREATORS_MAX: u32 = 32;
/// Maximum number of distinct roles attributable to a single creator. Equal
/// to the cardinality of [`CreatorRole`] — a creator cannot hold the same
/// role twice, and there are five role variants.
pub const CREATOR_ROLES_MAX: u32 = 5;
/// Maximum byte length of an opus designation (e.g. "Op. 27 No. 2").
pub const OPUS_MAX_LEN: u32 = 32;
/// Maximum byte length of a thematic catalogue number (e.g. "BWV 565", "K. 545").
pub const CATALOG_NUMBER_MAX_LEN: u32 = 32;
/// Maximum number of source-work references carried by Medley / Mashup.
pub const WORK_REFERENCES_MAX: u32 = 32;

pub type Creators = BoundedVec<Creator, ConstU32<CREATORS_MAX>>;
pub type CreatorRoles = BoundedBTreeSet<CreatorRole, ConstU32<CREATOR_ROLES_MAX>>;
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

/// Role attributed to a creator within a work. Derives `Ord` on the
/// declaration order so [`CreatorRoles`] (the bounded BTreeSet wrapping
/// these) iterates and SCALE-serialises in a canonical order.
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
    PartialOrd,
    Ord,
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

/// A creator attached to a work: the party (one or both of IPI / ISNI)
/// and the set of roles that party holds for this work.
///
/// `roles` is a [`BoundedBTreeSet`] rather than a plain list — a same
/// creator cannot legitimately hold the same role twice, and the set
/// shape removes the cross-validation we would otherwise need to forbid
/// duplicates. The set is also iterated in canonical order (the `Ord`
/// derivation on `CreatorRole`), so two semantically-equal `Creator`s
/// always encode identically.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Creator {
    pub roles: CreatorRoles,
    pub party: PartyId,
}

impl Creator {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        if self.roles.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        self.party.validate_format()
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
    pub creation_year: Option<u16>,
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
        if let Some(year) = self.creation_year
            && !(YEAR_MIN..=YEAR_MAX).contains(&year)
        {
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

    /// Builds a [`CreatorRoles`] from a static set of roles. Tests stay
    /// readable with `roles_set([CreatorRole::Composer])` instead of the
    /// raw `BoundedBTreeSet::try_insert` plumbing.
    fn roles_set<const N: usize>(roles: [CreatorRole; N]) -> CreatorRoles {
        let mut set = CreatorRoles::new();
        for r in roles {
            set.try_insert(r).expect("role within bound");
        }
        set
    }

    /// Minimal payload that passes `validate_format` — each test mutates one
    /// field to probe a single rule.
    fn base() -> MusicalWorkV1 {
        MusicalWorkV1 {
            iswc: iswc(),
            title: BoundedVec::try_from(b"x".to_vec()).expect("1-byte title"),
            creation_year: Some(2000),
            instrumental: false,
            language: None,
            bpm: None,
            key: None,
            work_type: WorkType::Original,
            creators: BoundedVec::try_from(vec![Creator {
                roles: roles_set([CreatorRole::Composer]),
                party: PartyId::Ipi(
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
    fn creation_year_bounds_when_present() {
        for (year, ok) in [
            (0u16, false),
            (1, true),
            (2000, true),
            (2999, true),
            (3000, false),
        ] {
            let mut w = base();
            w.creation_year = Some(year);
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
    fn creation_year_is_optional() {
        let mut w = base();
        w.creation_year = None;
        w.validate_format()
            .expect("None creation_year validates — field is optional");
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
