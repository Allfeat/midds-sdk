//! Cross-MIDDS shared types.
//!
//! Anything reused by more than one MIDDS payload lives here so the wire
//! shape stays identical across types and a fix lands once. `MusicalWork`
//! and `Recording` both pull their key, party-identifier and title types
//! from this module; the SCALE / JSON encoding is byte-for-byte the same as
//! when these types lived inside `musical_work::v1` (same fields, same order,
//! same serde shape) — only the module path changed, and the top-level
//! `midds_types` re-exports keep the public names stable.

use midds_traits::{
    Ipi, Isni, Isrc, Iswc, MiddsFormatError, MiddsId, MiddsString, validate_ipi_format,
    validate_isni_format, validate_isrc_format, validate_iswc_format,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Maximum byte length of a title (work title, recording title, alias).
pub const TITLE_MAX_LEN: u32 = 256;

/// A free-text title. Shared by every MIDDS payload that carries one.
pub type Title = MiddsString<TITLE_MAX_LEN>;

/// Inclusive lower bound for a calendar year carried by a payload
/// (`MusicalWork.creation_year`, `Recording.record_year`). Structural range
/// check only — `Release.release_date.year` is deliberately *not* bounded so
/// future-dated (announced) releases stay representable. See
/// `docs/validation.md` §7.
pub const YEAR_MIN: u16 = 1;
/// Inclusive upper bound for `creation_year` / `record_year`. Mirrors the
/// `1..=2999` rule the legacy front enforced; see `docs/validation.md` §4–5.
pub const YEAR_MAX: u16 = 2999;

/// Inclusive lower bound for `bpm`, shared by `MusicalWork` and `Recording`.
pub const BPM_MIN: u16 = 20;
/// Inclusive upper bound for `bpm`, shared by `MusicalWork` and `Recording`.
pub const BPM_MAX: u16 = 300;

/// Pitch class with explicit accidental spelling.
///
/// The 12 chromatic positions are represented with both their sharp and the
/// most common flat spellings (17 variants total). `CSharp` and `DFlat` share
/// the same sounding pitch but are *not* equivalent for notation or
/// publishing: a piece registered as `E♭ major` must round-trip as `E♭`, not
/// as `D♯`. CWR and DDEX both carry the spelling, and the on-chain payload
/// preserves it.
///
/// Theoretical enharmonics (`B♯`, `E♯`, `C♭`, `F♭`) are deliberately not
/// modelled — they are vanishingly rare in commercial catalogues and can be
/// added in a future payload version if a real use case appears.
///
/// SCALE layout: the original 12 sharp / natural variants keep their V1 tag
/// bytes (0..=11). The five flat variants are appended at tags 12..=16, so
/// records encoded before the extension decode unchanged.
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
    DFlat,
    EFlat,
    GFlat,
    AFlat,
    BFlat,
}

/// Diatonic mode. Kept minimal; modal music (Dorian, Phrygian, …) can be
/// modelled later through a fresh payload version.
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

/// Diatonic key: pitch class plus mode. Shared by `MusicalWork` (the work's
/// key) and `Recording` (the recorded performance's key).
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

/// External identifier of a natural or legal person: an IPI, an ISNI, or
/// **both** when the same party is referenced under the two registries
/// simultaneously (a composer with a CISAC IPI *and* an ISO ISNI is the
/// common case). Backs `MusicalWork.creators[*].party`, `Recording`'s
/// artist / performers / contributors, and `Release.artist`.
///
/// The `Both` variant was deliberately *re-introduced* relative to the
/// initial V1 draft: the on-chain representation is the place where the
/// two identifiers can be linked in a single registration, so collapsing
/// them into a single `PartyId` is more faithful to the underlying party
/// than two duplicated entries across a creators list. See
/// `docs/validation.md` §3 / §7.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PartyId {
    Ipi(#[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))] Ipi),
    Isni(#[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))] Isni),
    Both {
        #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
        ipi: Ipi,
        #[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))]
        isni: Isni,
    },
}

impl PartyId {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        match self {
            Self::Ipi(v) => validate_ipi_format(v),
            Self::Isni(v) => validate_isni_format(v),
            Self::Both { ipi, isni } => {
                validate_ipi_format(ipi)?;
                validate_isni_format(isni)
            }
        }
    }
}

/// Reference to a musical work, either by its on-chain MIDDS id (once the
/// work itself is registered) or by its external ISWC (for works not — or
/// not yet — on-chain). The `MiddsId` variant is the cheapest on-chain
/// reference (8 bytes, no string); the ISWC variant keeps the system usable
/// before the referenced work is registered.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum WorkRef {
    /// On-chain `MusicalWork` MIDDS id.
    Midds(MiddsId),
    /// External ISWC of the work.
    Iswc(#[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))] Iswc),
}

impl WorkRef {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        match self {
            Self::Midds(_) => Ok(()),
            Self::Iswc(i) => validate_iswc_format(i),
        }
    }
}

/// Reference to a recording, either by its on-chain MIDDS id (once the
/// recording itself is registered) or by its external ISRC (for recordings
/// not — or not yet — on-chain). The exact `Recording`-side analogue of
/// [`WorkRef`]: the `Midds` variant is the cheapest on-chain reference
/// (8 bytes, no string), the ISRC variant keeps a `Release` tracklist usable
/// before each referenced recording is registered.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RecordingRef {
    /// On-chain `Recording` MIDDS id.
    Midds(MiddsId),
    /// External ISRC of the recording.
    Isrc(#[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))] Isrc),
}

impl RecordingRef {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        match self {
            Self::Midds(_) => Ok(()),
            Self::Isrc(i) => validate_isrc_format(i),
        }
    }
}
