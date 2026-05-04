//! Error types surfaced by `midds-validate`.

use thiserror::Error;

/// Reason a tolerant parser refused an input.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    /// Input did not match the canonical pattern.
    #[error("input does not match the expected identifier pattern")]
    PatternMismatch,
    /// The normalised value would exceed the on-chain `BoundedVec` capacity.
    #[error("normalised value exceeds the on-chain capacity")]
    OutOfBounds,
}

/// Per-field diagnostic emitted by a builder during finalization. Builders
/// aggregate one of these per failing field so the caller can surface the
/// full list to the user instead of a single first-error blocking iteration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// Builder method whose input failed validation (e.g. `"iswc"`,
    /// `"title"`, `"creators[2]"`).
    pub field: &'static str,
    /// Human-readable cause. Kept as a `String` so callers don't need to
    /// match on a sealed enum to render the diagnostic.
    pub message: String,
}

/// Aggregated outcome of a builder's `build()` call.
#[derive(Debug, Clone, Error)]
pub enum BuildError {
    /// Required field never set (e.g. `iswc`, `title`, no `add_creator`).
    #[error("missing required field: {0}")]
    Missing(&'static str),
    /// One or more fields rejected — every diagnostic preserved so callers
    /// can render the full list at once.
    #[error("{} field(s) failed validation", .0.len())]
    Fields(Vec<FieldError>),
}
