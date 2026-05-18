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
    Ipi, Isni, Iswc, MiddsFormatError, MiddsId, MiddsString, validate_ipi_format,
    validate_isni_format, validate_iswc_format,
};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Maximum byte length of a title (work title, recording title, alias).
pub const TITLE_MAX_LEN: u32 = 256;

/// A free-text title. Shared by every MIDDS payload that carries one.
pub type Title = MiddsString<TITLE_MAX_LEN>;

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

/// External identifier of a natural or legal person: either an IPI or an
/// ISNI code. Generalises what `MusicalWork` historically called
/// `CreatorId` (kept as a type alias for source compatibility) and backs
/// `Recording`'s artist / performers / contributors.
#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PartyId {
    Ipi(#[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))] Ipi),
    Isni(#[cfg_attr(feature = "serde", serde(with = "midds_traits::serde_helpers::ascii"))] Isni),
}

impl PartyId {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        match self {
            Self::Ipi(v) => validate_ipi_format(v),
            Self::Isni(v) => validate_isni_format(v),
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
