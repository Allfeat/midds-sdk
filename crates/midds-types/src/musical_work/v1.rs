//! First on-chain version of the `MusicalWork` payload and its parts.

use bounded_collections::{BoundedBTreeSet, BoundedVec, ConstU32};
use midds_traits::{
    Iswc, MiddsFormatError, MiddsString, OffchainHash, validate_iswc_format, validate_offchain_hash,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::language::Language;
use crate::shared::{
    PartyId, WorkRef, require_bpm_in_bounds, require_non_empty, require_non_empty_opt,
    require_year_in_bounds,
};

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
/// Maximum number of sampled-work references a work may declare (the works
/// *this* work sampled). Generous cap — heavily-sampled productions stay
/// representable on-chain; collage works beyond it can fall back to the
/// off-chain extension.
pub const SAMPLES_MAX: u32 = 64;

/// Creators attached to a work (at least one is mandatory).
pub type Creators = BoundedVec<Creator, ConstU32<CREATORS_MAX>>;
/// Set of roles one creator holds for a work — a set, so a role cannot
/// repeat, iterated in canonical [`CreatorRole`] order.
pub type CreatorRoles = BoundedBTreeSet<CreatorRole, ConstU32<CREATOR_ROLES_MAX>>;
/// Opus designation of a classical work (e.g. "Op. 27 No. 2").
pub type Opus = MiddsString<OPUS_MAX_LEN>;
/// Thematic catalogue number of a classical work (e.g. "BWV 565").
pub type CatalogNumber = MiddsString<CATALOG_NUMBER_MAX_LEN>;
/// Source-work ISWCs referenced by a Medley / Mashup.
pub type WorkReferences = BoundedVec<Iswc, ConstU32<WORK_REFERENCES_MAX>>;
/// List of sampled-work references. Each entry is a [`WorkRef`] — a sampled
/// work may be cited by its on-chain MIDDS id or by an external ISWC — so a
/// sample can be declared before the sampled work is itself registered.
pub type SampleReferences = BoundedVec<WorkRef, ConstU32<SAMPLES_MAX>>;

/// Top-level classification of a musical work, with the source ISWCs the
/// derived variants reference.
///
/// SCALE layout: variant tags are append-only (`Original` = 0 … `Adaptation`
/// = 3). `Rearrangement` is appended at tag 4, so payloads encoded before it
/// existed decode unchanged.
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
    /// Re-arrangement of exactly one source work (e.g. an orchestral version
    /// of a song, a piano reduction). Structurally identical to `Adaptation`
    /// — one source ISWC — but a distinct variant so the *kind* of derivation
    /// survives on the wire.
    Rearrangement(
        #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))] Iswc,
    ),
}

/// Role attributed to a creator within a work. Derives `Ord` on the
/// declaration order so [`CreatorRoles`] (the bounded `BTreeSet` wrapping
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
    /// Wrote the lyrics.
    Author,
    /// Wrote the music.
    Composer,
    /// Arranged the music.
    Arranger,
    /// Adapted the lyrics (e.g. a translation).
    Adapter,
    /// Publishing entity administering the work.
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
    /// Roles this party holds for the work (at least one is mandatory).
    pub roles: CreatorRoles,
    /// The party, identified by IPI, ISNI or both.
    pub party: PartyId,
}

impl Creator {
    /// Validates the credit: a non-empty role set and a well-formed party.
    /// Errors: [`MiddsFormatError::EmptyMandatoryField`] on an empty role set;
    /// otherwise propagates [`PartyId::validate_format`].
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
    /// Opus designation (e.g. "Op. 27 No. 2").
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub opus: Option<Opus>,
    /// Thematic catalogue number (e.g. "K. 545").
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub catalog_number: Option<CatalogNumber>,
    /// Number of voices / parts; must be positive when present.
    pub number_of_voices: Option<u16>,
}

impl ClassicalInfo {
    /// Validates the block: present sub-fields must be non-empty / positive.
    /// Errors: [`MiddsFormatError::EmptyMandatoryField`] on a present-but-empty text
    /// field, [`MiddsFormatError::OutOfBounds`] on a zero voice count.
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        require_non_empty_opt(self.opus.as_ref())?;
        require_non_empty_opt(self.catalog_number.as_ref())?;
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
    /// Canonical ISWC of the work — the uniqueness-index key.
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub iswc: Iswc,
    /// Primary title (mandatory, non-empty).
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub title: Title,
    /// Year the work was created, in `1..=2999` when present.
    pub creation_year: Option<u16>,
    /// Whether the work has no lyrics.
    pub instrumental: bool,
    /// Language of the lyrics, when known.
    pub language: Option<Language>,
    /// Whether the work's lyrics are explicit. Defaults to `false`; an
    /// instrumental work has no lyrics, so `explicit_lyrics` should stay
    /// `false` when `instrumental` is `true` (a builder-side convention,
    /// surfaced as a warning in `midds-validate`, not enforced on-chain).
    pub explicit_lyrics: bool,
    /// Tempo in beats per minute, in `20..=300` when present.
    pub bpm: Option<u16>,
    /// Diatonic key of the work, when known.
    pub key: Option<MusicalKey>,
    /// Original / derived classification, with source references.
    pub work_type: WorkType,
    /// Works sampled *by* this work, each referenced by MIDDS id or ISWC.
    /// Empty when the work samples nothing.
    pub samples: SampleReferences,
    /// Creators of the work (at least one is mandatory).
    pub creators: Creators,
    /// Classical-music metadata, absent for non-classical works.
    pub classical_info: Option<ClassicalInfo>,
    /// Off-chain extension hash (opaque on-chain; `CIDv1` by client
    /// convention). The standard MIDDS extensibility hook.
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub offchain_extension: Option<OffchainHash>,
}

impl MusicalWorkV1 {
    /// Validates every structural rule of the payload (charset, length,
    /// ranges, cardinality, cross-field consistency) per
    /// `docs/validation.md` §4.
    /// Errors: The [`MiddsFormatError`] variant matching the first violated rule.
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        validate_iswc_format(&self.iswc)?;
        require_non_empty(&self.title)?;
        require_year_in_bounds(self.creation_year)?;
        require_bpm_in_bounds(self.bpm)?;
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
                // A medley / mashup must reference at least two *distinct other*
                // works: no reference may be the work itself, and no reference
                // may repeat. Both are structurally meaningless otherwise. The
                // `BTreeSet` keeps the uniqueness scan O(n log n) over the
                // (<= WORK_REFERENCES_MAX = 32) entries, mirroring the Release
                // tracklist-uniqueness check.
                let mut seen = alloc::collections::BTreeSet::new();
                for r in refs {
                    if r == &self.iswc || !seen.insert(r) {
                        return Err(MiddsFormatError::CrossFieldInconsistency);
                    }
                }
            }
            WorkType::Adaptation(iswc) | WorkType::Rearrangement(iswc) => {
                validate_iswc_format(iswc)?;
                // A derivative work (adaptation or rearrangement) cannot
                // derive from itself.
                if iswc == &self.iswc {
                    return Err(MiddsFormatError::CrossFieldInconsistency);
                }
            }
        }
        // Sampled-work references: each must be structurally valid, the list
        // must hold no duplicate, and no sample may be the work itself. Self-
        // reference is only checkable for the ISWC variant — a `WorkRef::Midds`
        // points at a MIDDS id assigned at deposit time, unknown here. Mirrors
        // the Medley / Mashup uniqueness + non-self-reference rules (and the
        // Release tracklist check) with a `BTreeSet`, O(n log n) over the
        // (<= SAMPLES_MAX = 64) entries.
        let mut seen_samples = alloc::collections::BTreeSet::new();
        for s in &self.samples {
            s.validate_format()?;
            if let WorkRef::Iswc(i) = s
                && i == &self.iswc
            {
                return Err(MiddsFormatError::CrossFieldInconsistency);
            }
            if !seen_samples.insert(s) {
                return Err(MiddsFormatError::CrossFieldInconsistency);
            }
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

    /// A structurally-valid ISWC distinct from [`iswc`] (`T0000000000`) for
    /// `1 <= n <= 9` — lets the work-reference tests build distinct,
    /// non-self source lists.
    fn iswc_n(n: u8) -> Iswc {
        let mut bytes = b"T0000000000".to_vec();
        bytes[10] = b'0' + n;
        BoundedVec::try_from(bytes).expect("11-byte ISWC")
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
            explicit_lyrics: false,
            bpm: None,
            key: None,
            work_type: WorkType::Original,
            samples: BoundedVec::default(),
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

    /// Builds a [`SampleReferences`] list from raw [`WorkRef`]s, panicking past
    /// the bound — keeps the sample tests below readable.
    fn samples<const N: usize>(refs: [WorkRef; N]) -> SampleReferences {
        SampleReferences::try_from(refs.to_vec()).expect("samples within bound")
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
        // Distinct refs, none equal to `base().iswc` — isolates the cardinality
        // rule from the distinctness / self-reference rules below.
        let distinct = [iswc_n(1), iswc_n(2)];
        let refs =
            |n: usize| WorkReferences::try_from(distinct[..n].to_vec()).expect("refs within bound");
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
            w.validate_format()
                .expect(">= 2 distinct non-self refs validates");
        }
    }

    #[test]
    fn medley_and_mashup_reject_duplicate_refs() {
        for make in [
            WorkType::Medley as fn(WorkReferences) -> WorkType,
            WorkType::Mashup,
        ] {
            let mut w = base();
            // Two structurally-valid but identical refs.
            w.work_type = make(
                WorkReferences::try_from(vec![iswc_n(1), iswc_n(1)]).expect("refs within bound"),
            );
            assert_eq!(
                w.validate_format(),
                Err(MiddsFormatError::CrossFieldInconsistency)
            );
        }
    }

    #[test]
    fn medley_and_mashup_reject_self_reference() {
        for make in [
            WorkType::Medley as fn(WorkReferences) -> WorkType,
            WorkType::Mashup,
        ] {
            let mut w = base();
            let self_iswc = w.iswc.clone();
            // One distinct source plus the work's own ISWC.
            w.work_type = make(
                WorkReferences::try_from(vec![iswc_n(1), self_iswc]).expect("refs within bound"),
            );
            assert_eq!(
                w.validate_format(),
                Err(MiddsFormatError::CrossFieldInconsistency)
            );
        }
    }

    #[test]
    fn adaptation_rejects_self_reference() {
        let mut w = base();
        w.work_type = WorkType::Adaptation(w.iswc.clone());
        assert_eq!(
            w.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency)
        );
        // An adaptation of a distinct source validates.
        w.work_type = WorkType::Adaptation(iswc_n(1));
        w.validate_format()
            .expect("adaptation on a distinct source validates");
    }

    #[test]
    fn rearrangement_rejects_self_reference() {
        let mut w = base();
        w.work_type = WorkType::Rearrangement(w.iswc.clone());
        assert_eq!(
            w.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency)
        );
        // A rearrangement of a distinct source validates.
        w.work_type = WorkType::Rearrangement(iswc_n(1));
        w.validate_format()
            .expect("rearrangement on a distinct source validates");
    }

    #[test]
    fn samples_accepts_distinct_mixed_refs() {
        let mut w = base();
        // A MIDDS-id sample and an ISWC sample, distinct and non-self.
        w.samples = samples([WorkRef::Midds(7), WorkRef::Iswc(iswc_n(1))]);
        w.validate_format()
            .expect("distinct MIDDS + ISWC samples validate");
    }

    #[test]
    fn samples_reject_invalid_iswc() {
        let mut w = base();
        let bad = BoundedVec::try_from(b"X0000000000".to_vec()).expect("11 bytes");
        w.samples = samples([WorkRef::Iswc(bad)]);
        assert_eq!(
            w.validate_format(),
            Err(MiddsFormatError::InvalidIdentifierStructure)
        );
    }

    #[test]
    fn samples_reject_duplicate() {
        let mut w = base();
        // Same ISWC sampled twice.
        w.samples = samples([WorkRef::Iswc(iswc_n(1)), WorkRef::Iswc(iswc_n(1))]);
        assert_eq!(
            w.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency)
        );
        // The MIDDS-id variant is deduplicated on exact equality too.
        w.samples = samples([WorkRef::Midds(3), WorkRef::Midds(3)]);
        assert_eq!(
            w.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency)
        );
    }

    #[test]
    fn samples_reject_self_iswc() {
        let mut w = base();
        // Sampling the work's own ISWC is meaningless.
        w.samples = samples([WorkRef::Iswc(w.iswc.clone())]);
        assert_eq!(
            w.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency)
        );
    }

    #[test]
    fn empty_samples_validate() {
        let mut w = base();
        w.samples = BoundedVec::default();
        w.validate_format()
            .expect("a work that samples nothing validates");
    }
}
