//! Format-validation error type shared by every MIDDS payload.

use parity_scale_codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Format-validation error for a MIDDS payload or identifier.
///
/// On-chain validation only covers structure / charset / length. Rich
/// validation (checksums, normalisation) lives in `midds-validate` and is
/// intentionally not blocking on-chain.
///
/// Adding variants is **breaking** SCALE — every consumer that decodes this
/// enum must be rebuilt. Prospective variants for future MIDDS types
/// (`Recording`, `Release`) are reserved here so a later addition does not
/// require a runtime upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, MaxEncodedLen, TypeInfo)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MiddsFormatError {
    /// Identifier does not match the expected high-level structure.
    InvalidIdentifierStructure,
    /// Bytes outside the allowed charset for this field.
    InvalidCharset,
    /// Length below the minimum or above the maximum allowed.
    OutOfBounds,
    /// A mandatory field is empty.
    EmptyMandatoryField,
    /// Two date / year fields in the same payload disagree on a relation
    /// they should always satisfy (e.g. `release_year < creation_year`,
    /// `recording_year > work_year`). Reserved for `Recording` / `Release`.
    DateInconsistency,
    /// Two non-date fields constrain each other but disagree (e.g. the
    /// declared tracklist length does not match the number of `Recording`
    /// references, a creator's role contradicts the work type, or a
    /// `MusicalWork` medley/mashup/adaptation references itself or lists the
    /// same source ISWC twice). Used for cross-field invariants on
    /// `MusicalWork`, `Recording`, and `Release`.
    CrossFieldInconsistency,
}

impl core::fmt::Display for MiddsFormatError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::InvalidIdentifierStructure => "identifier structure is invalid",
            Self::InvalidCharset => "bytes outside the allowed charset",
            Self::OutOfBounds => "length or value out of the allowed range",
            Self::EmptyMandatoryField => "a mandatory field is empty",
            Self::DateInconsistency => "date fields are mutually inconsistent",
            Self::CrossFieldInconsistency => "fields are mutually inconsistent",
        };
        f.write_str(msg)
    }
}

impl core::error::Error for MiddsFormatError {}
