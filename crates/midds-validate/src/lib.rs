// Shared curated `clippy::pedantic` policy — identical in every crate root;
// anything not listed here must stay pedantic-clean.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::if_not_else,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::option_option,
    clippy::return_self_not_must_use,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
//! Std-side rich validation for MIDDS data.
//!
//! This crate is the dev/SDK companion to `midds-traits::validate_*_format`:
//! tolerant parsers (accept punctuated, lowercase, prefixed inputs and
//! normalise to canonical form) and warning-only check-digit verifiers.
//! None of it ever runs on-chain — the pallet only ever calls
//! `Midds::validate_format`, which intentionally refuses just the coarsest
//! structural errors.
#![deny(unsafe_code)]
#![warn(missing_docs)]

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
