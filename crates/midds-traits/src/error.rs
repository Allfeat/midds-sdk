use parity_scale_codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Format-validation error for a MIDDS payload or identifier.
///
/// On-chain validation only covers structure / charset / length. Rich
/// validation (checksums, normalisation) lives in `midds-validate` and is
/// intentionally not blocking on-chain.
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
}
