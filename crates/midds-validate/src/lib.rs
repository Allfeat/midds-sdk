//! Std-side rich validation for MIDDS data.
//!
//! This crate is the dev/SDK companion to `midds-traits::validate_*_format`:
//! tolerant parsers (accept punctuated, lowercase, prefixed inputs and
//! normalise to canonical form) and warning-only check-digit verifiers.
//! None of it ever runs on-chain — the pallet only ever calls
//! `Midds::validate_format`, which intentionally refuses just the coarsest
//! structural errors.

pub mod builder;
pub mod checksum;
pub mod error;
pub mod parse;

pub use builder::{MusicalWorkBuilder, RecordingBuilder, ReleaseBuilder};
pub use checksum::{
    CheckResult, verify_ipi_checksum, verify_isni_checksum, verify_isrc_checksum,
    verify_iswc_checksum, verify_upc_checksum,
};
pub use error::{BuildError, FieldError, ParseError};
pub use parse::{parse_ipi, parse_ipn, parse_isni, parse_isrc, parse_iswc, parse_upc};
