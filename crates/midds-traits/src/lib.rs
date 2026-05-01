#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::Parameter;
use parity_scale_codec::MaxEncodedLen;

pub mod error;
pub mod identifier;

pub use error::MiddsFormatError;
pub use identifier::*;

/// Per-instance unique on-chain identifier of a MIDDS record.
pub type MiddsId = u64;

/// Common interface implemented by every MIDDS payload type.
///
/// Stored generically by `pallet-midds` and queried by the runtime API. The
/// associated `Identifier` is the canonical industry code (ISWC for
/// `MusicalWork`, ISRC for `Recording`, …) backing the reverse uniqueness index.
pub trait Midds: Parameter + MaxEncodedLen {
    /// Canonical industry identifier used for the reverse uniqueness index.
    type Identifier: Parameter + MaxEncodedLen + Ord;

    /// Extract the canonical identifier of this record.
    fn identifier(&self) -> Self::Identifier;

    /// Charset / length / structure validation. Does not verify checksums.
    fn validate_format(&self) -> Result<(), MiddsFormatError>;
}
