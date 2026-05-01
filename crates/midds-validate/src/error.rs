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
