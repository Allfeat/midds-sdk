//! First on-chain version of the `Release` payload and its parts.

use bounded_collections::{BoundedVec, ConstU32};
use midds_traits::{
    Isni, MiddsFormatError, MiddsString, OffchainHash, Upc, validate_isni_format,
    validate_offchain_hash, validate_upc_format,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

use crate::Country;
use crate::shared::{PartyId, RecordingRef, Title, require_non_empty};

/// Maximum number of alternative / localized titles on a release.
pub const TITLE_ALIASES_MAX: u32 = 8;
/// Maximum number of featured artists ("feat.") credited on the release.
pub const FEATURING_MAX: u32 = 16;
/// Maximum number of recordings (tracks) referenced by a release. Sized for
/// large box sets; the per-track [`Track`] is at most 16 SCALE bytes.
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
/// Featured artists ("feat.") credited on the release. Each is a [`PartyId`]
/// (IPI / ISNI / both) exactly like the main `artist`, mirroring
/// `Recording.featuring`.
pub type Featuring = BoundedVec<PartyId, ConstU32<FEATURING_MAX>>;
/// Ordered tracklist: each [`Track`] pairs its 1-based track number with the
/// recording it references (on-chain MIDDS id or external ISRC).
pub type Tracks = BoundedVec<Track, ConstU32<TRACKS_MAX>>;
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

/// A numbered entry in a release tracklist: the track number paired with the
/// recording it references.
///
/// `number` is the human-facing 1-based track number (not the vector index).
/// Across the whole tracklist the numbers must form a **contiguous 1-based
/// sequence** — sorted, they are exactly `1, 2, …, N` for an `N`-track release:
/// they start at 1, increment by 1 and leave no gaps (which also makes them
/// unique and `>= 1`). The recording is referenced on-chain (MIDDS id) or
/// externally (ISRC) via [`RecordingRef`] and must be unique across the
/// tracklist (no recording — hence no ISRC — listed twice). Both invariants
/// are enforced by [`ReleaseV1::validate_format`].
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Track {
    /// 1-based track number; the numbers across a tracklist must run
    /// `1, 2, …, N` (contiguous, starting at 1).
    pub number: u16,
    /// The recording this track points to.
    pub recording: RecordingRef,
}

impl Track {
    /// Validates the entry: a positive track number and a well-formed
    /// recording reference.
    /// Errors: [`MiddsFormatError::OutOfBounds`] on a zero track number; otherwise
    /// propagates [`RecordingRef::validate_format`].
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        if self.number == 0 {
            return Err(MiddsFormatError::OutOfBounds);
        }
        self.recording.validate_format()
    }
}

/// A producer / label credited on the release. Producers are legal persons
/// identified by ISNI in industry metadata (not IPI), each carrying the
/// catalog number under which they issued the release — co-editions across
/// multiple labels each keep their own number.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Producer {
    /// ISNI of the producing label / person.
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub isni: Isni,
    /// Catalog number under which this producer issued the release.
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub catalog_number: CatalogNumber,
}

impl Producer {
    /// Validates the credit: a well-formed ISNI and a non-empty catalog
    /// number.
    /// Errors: Propagates the ISNI format error, then
    /// [`MiddsFormatError::EmptyMandatoryField`] on an empty catalog number.
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        validate_isni_format(&self.isni)?;
        require_non_empty(&self.catalog_number)
    }
}

/// Editorial status of a release, aligned with the `MusicBrainz` / DDEX
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

/// Primary editorial type of a release. Flattened from the `MusicBrainz`
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
// Variant names are the MusicBrainz release-group labels themselves.
#[allow(missing_docs)]
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
/// `MusicBrainz` medium-format list, kept to the commercially common values.
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
// Variant names are the MusicBrainz medium-format labels themselves.
#[allow(missing_docs)]
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

/// Physical packaging of the release, aligned with the `MusicBrainz`
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
// Variant names are the MusicBrainz packaging labels themselves.
#[allow(missing_docs)]
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
    /// Calendar year. Deliberately unconstrained so future-dated
    /// (announced) releases stay representable — see `docs/validation.md` §7.
    pub year: u16,
    /// Calendar month, `1..=12`.
    pub month: u8,
    /// Calendar day, `1..=31`.
    pub day: u8,
}

impl ReleaseDate {
    /// Validates the structural month / day ranges (not calendar validity).
    /// Errors: [`MiddsFormatError::OutOfBounds`] on a month outside `1..=12` or a
    /// day outside `1..=31`.
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
/// Identity (artist) and featured artists reuse the shared [`PartyId`]; each
/// tracklist entry is a [`Track`] pairing a number with a shared
/// [`RecordingRef`]; producers are ISNI-keyed pairs.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReleaseV1 {
    /// Canonical UPC / EAN barcode — the uniqueness-index key.
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub upc: Upc,
    /// Primary title (mandatory, non-empty).
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub title: Title,
    /// Alternative / localized titles; each must be non-empty.
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_vec")
    )]
    pub title_aliases: TitleAliases,
    /// Main credited artist.
    pub artist: PartyId,
    /// Featured artists ("feat.").
    pub featuring: Featuring,
    /// Ordered tracklist (non-empty; numbers must cover `1..=N`).
    pub tracks: Tracks,
    /// Producers / labels credited, each with their own catalog number.
    pub producers: Producers,
    /// Editorial status (official, promotional, …).
    pub status: ReleaseStatus,
    /// Calendar release date (strict: year, month and day all mandatory).
    pub release_date: ReleaseDate,
    /// Country the release was issued in (ISO 3166-1).
    pub country: Country,
    /// Distributor name (mandatory, non-empty).
    #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
    pub distributor_name: DistributorName,
    /// Primary editorial type (album, single, EP, …).
    pub release_type: ReleaseType,
    /// Physical or digital medium.
    pub format: ReleaseFormat,
    /// Physical packaging.
    pub packaging: ReleasePackaging,
    /// Cover-artwork contributor names; each must be non-empty.
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_vec")
    )]
    pub cover_contributors: CoverContributors,
    /// Off-chain extension hash (opaque on-chain; `CIDv1` by client
    /// convention). The standard MIDDS extensibility hook — present on every
    /// payload type even though it is not a domain field.
    #[cfg_attr(
        feature = "serde",
        serde(with = "midds_traits::serde_helpers::ascii_opt")
    )]
    pub offchain_extension: Option<OffchainHash>,
}

impl ReleaseV1 {
    /// Validates every structural rule of the payload (charset, length,
    /// ranges, cardinality, tracklist invariants) per
    /// `docs/validation.md` §6.
    /// Errors: The [`MiddsFormatError`] variant matching the first violated rule.
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        validate_upc_format(&self.upc)?;
        require_non_empty(&self.title)?;
        for alias in &self.title_aliases {
            require_non_empty(alias)?;
        }
        self.artist.validate_format()?;
        for f in &self.featuring {
            f.validate_format()?;
        }
        if self.tracks.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        // Per-track format (number `>= 1`, recording-ref structure) plus the
        // tracklist-wide recording-uniqueness invariant: no `RecordingRef` —
        // hence no ISRC — may appear twice. The `BTreeSet` keeps this O(n log n)
        // over the ≤ TRACKS_MAX = 256 entries rather than an O(n²) pairwise scan.
        let mut seen_recordings = alloc::collections::BTreeSet::new();
        for t in &self.tracks {
            t.validate_format()?;
            if !seen_recordings.insert(&t.recording) {
                return Err(MiddsFormatError::CrossFieldInconsistency);
            }
        }
        // Numbers are validated as a *set* covering exactly `1..=N` (order
        // free). Pigeonhole: N numbers, each in `1..=N`, none repeated —
        // checked on a stack bitmap, no heap alloc (this runs on-chain).
        // Zero was already rejected by `Track::validate_format` above.
        let track_count = self.tracks.len();
        let mut seen_numbers = [false; TRACKS_MAX as usize];
        for t in &self.tracks {
            let idx = usize::from(t.number) - 1;
            if idx >= track_count || seen_numbers[idx] {
                return Err(MiddsFormatError::CrossFieldInconsistency);
            }
            seen_numbers[idx] = true;
        }
        for p in &self.producers {
            p.validate_format()?;
        }
        self.release_date.validate_format()?;
        require_non_empty(&self.distributor_name)?;
        for c in &self.cover_contributors {
            require_non_empty(c)?;
        }
        if let Some(h) = &self.offchain_extension {
            validate_offchain_hash(h)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Boundary tests for the tracklist invariants stabilised per
    //! `docs/validation.md` §6/§8: recording-reference uniqueness and the
    //! strict contiguous 1-based numbering (`1, 2, …, N`). Identifier-structure
    //! and empty-mandatory paths are covered by `midds-fixtures`
    //! (`pathological`, proptest).
    use super::*;

    /// Minimal payload that passes `validate_format` — each test mutates
    /// `tracks` only.
    fn base() -> ReleaseV1 {
        ReleaseV1 {
            upc: BoundedVec::try_from(b"000000000000".to_vec()).expect("12-byte UPC-A"),
            title: BoundedVec::try_from(b"x".to_vec()).expect("1-byte title"),
            title_aliases: BoundedVec::default(),
            artist: PartyId::Ipi(BoundedVec::try_from(b"123456789".to_vec()).expect("9-byte IPI")),
            featuring: BoundedVec::default(),
            tracks: BoundedVec::try_from(vec![track(1, RecordingRef::Midds(1))]).expect("1 track"),
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

    fn track(number: u16, recording: RecordingRef) -> Track {
        Track { number, recording }
    }

    #[test]
    fn base_is_valid() {
        base().validate_format().expect("base payload validates");
    }

    #[test]
    fn distinct_tracks_validate() {
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![
            track(1, RecordingRef::Midds(1)),
            track(2, RecordingRef::Midds(2)),
            track(3, isrc(b"USRC17607839")),
            track(4, isrc(b"GBAYE0601477")),
        ])
        .expect("4 distinct tracks");
        r.validate_format().expect("distinct tracklist validates");
    }

    #[test]
    fn out_of_order_but_contiguous_numbers_validate() {
        // Numbering is validated as a set: the stored vector need not be in
        // number order, only the numbers must cover 1..=N. Here recordings are
        // listed with their numbers descending (3, 2, 1) — still a valid
        // tracklist because the set is {1, 2, 3}.
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![
            track(3, isrc(b"USRC17607839")),
            track(2, RecordingRef::Midds(2)),
            track(1, RecordingRef::Midds(1)),
        ])
        .expect("3 tracks");
        r.validate_format()
            .expect("out-of-order but contiguous numbering validates");
    }

    #[test]
    fn gapped_track_numbers_rejected() {
        // Strict 1..=N: a gap (no track 2/3/4) breaks contiguity even though
        // every number is positive and unique.
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![
            track(1, RecordingRef::Midds(1)),
            track(5, RecordingRef::Midds(2)),
            track(42, isrc(b"USRC17607839")),
        ])
        .expect("3 tracks");
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency),
        );
    }

    #[test]
    fn numbering_not_starting_at_one_rejected() {
        // A single track numbered 2 has no gap internally but does not start at
        // 1, so the 1..=N sequence is incomplete.
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![track(2, RecordingRef::Midds(1))]).expect("1 track");
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency),
        );
    }

    #[test]
    fn zero_track_number_rejected() {
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![track(0, RecordingRef::Midds(1))]).expect("1 track");
        assert_eq!(r.validate_format(), Err(MiddsFormatError::OutOfBounds));
    }

    #[test]
    fn duplicate_track_number_rejected() {
        // Same number on two distinct recordings → CrossFieldInconsistency.
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![
            track(7, RecordingRef::Midds(1)),
            track(7, RecordingRef::Midds(2)),
        ])
        .expect("2 tracks");
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency),
        );
    }

    #[test]
    fn duplicate_midds_track_rejected() {
        // Same recording under two different numbers → CrossFieldInconsistency.
        let mut r = base();
        r.tracks = BoundedVec::try_from(vec![
            track(1, RecordingRef::Midds(7)),
            track(2, RecordingRef::Midds(7)),
        ])
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
            track(1, RecordingRef::Midds(1)),
            track(2, isrc(b"USRC17607839")),
            track(3, isrc(b"USRC17607839")),
        ])
        .expect("3 tracks");
        assert_eq!(
            r.validate_format(),
            Err(MiddsFormatError::CrossFieldInconsistency),
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
        assert_eq!(r.validate_format(), Err(MiddsFormatError::InvalidCharset));
    }
}
