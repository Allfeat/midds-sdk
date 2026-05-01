//! Tolerant parsers for canonical industry identifiers.
//!
//! Real-world inputs come in many shapes: ISWCs are quoted as
//! `T-034.524.680-1` or `T0345246801`, ISNIs are usually space-grouped, IPIs
//! are sometimes prefixed with `I-`. These parsers accept the common variants,
//! strip separators, uppercase letters and emit the strict canonical form
//! that `midds-traits::validate_*_format` expects on-chain.

use std::sync::LazyLock;

use midds_traits::{Ipi, Isni, Iswc};
use regex::Regex;

use crate::error::ParseError;

static ISWC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^T-?(\d{3})\.?(\d{3})\.?(\d{3})-?(\d)$").unwrap());

static ISNI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4})[\s-]?(\d{4})[\s-]?(\d{4})[\s-]?(\d{3}[\dX])$").unwrap());

static IPI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:I-)?(\d{9,11})$").unwrap());

/// Parse a tolerant ISWC string into the canonical 11-byte form `T` + 10 digits.
pub fn parse_iswc(s: &str) -> Result<Iswc, ParseError> {
    let upper = s.trim().to_ascii_uppercase();
    let caps = ISWC_RE
        .captures(&upper)
        .ok_or(ParseError::PatternMismatch)?;
    let mut out = String::with_capacity(11);
    out.push('T');
    for i in 1..=4 {
        out.push_str(&caps[i]);
    }
    Iswc::try_from(out.into_bytes()).map_err(|_| ParseError::OutOfBounds)
}

/// Parse a tolerant ISNI string into the canonical 16-byte form
/// (15 digits + final digit or `X`).
pub fn parse_isni(s: &str) -> Result<Isni, ParseError> {
    let upper = s.trim().to_ascii_uppercase();
    let caps = ISNI_RE
        .captures(&upper)
        .ok_or(ParseError::PatternMismatch)?;
    let mut out = String::with_capacity(16);
    for i in 1..=4 {
        out.push_str(&caps[i]);
    }
    Isni::try_from(out.into_bytes()).map_err(|_| ParseError::OutOfBounds)
}

/// Parse a tolerant IPI Name Number into 9–11 digits.
pub fn parse_ipi(s: &str) -> Result<Ipi, ParseError> {
    let upper = s.trim().to_ascii_uppercase();
    let caps = IPI_RE.captures(&upper).ok_or(ParseError::PatternMismatch)?;
    let digits = caps[1].as_bytes().to_vec();
    Ipi::try_from(digits).map_err(|_| ParseError::OutOfBounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iswc_canonical_round_trips() {
        let v = parse_iswc("T0345246801").expect("canonical input");
        assert_eq!(v.as_slice(), b"T0345246801");
    }

    #[test]
    fn iswc_with_separators_normalises() {
        let v = parse_iswc("T-034.524.680-1").expect("punctuated input");
        assert_eq!(v.as_slice(), b"T0345246801");
    }

    #[test]
    fn iswc_lowercase_normalises() {
        let v = parse_iswc("t-034.524.680-1").expect("lowercase prefix");
        assert_eq!(v.as_slice(), b"T0345246801");
    }

    #[test]
    fn iswc_trim() {
        let v = parse_iswc("  T0345246801  ").expect("padded");
        assert_eq!(v.as_slice(), b"T0345246801");
    }

    #[test]
    fn iswc_pattern_mismatch() {
        assert_eq!(parse_iswc("X0345246801"), Err(ParseError::PatternMismatch));
        assert_eq!(parse_iswc("T03452468"), Err(ParseError::PatternMismatch));
        assert_eq!(parse_iswc("T03452468AB"), Err(ParseError::PatternMismatch));
        assert_eq!(parse_iswc(""), Err(ParseError::PatternMismatch));
    }

    #[test]
    fn isni_grouped_normalises() {
        let v = parse_isni("0000 0001 2103 2683").expect("space-grouped");
        assert_eq!(v.as_slice(), b"0000000121032683");
    }

    #[test]
    fn isni_dashed_normalises() {
        let v = parse_isni("0000-0001-2103-2683").expect("dash-grouped");
        assert_eq!(v.as_slice(), b"0000000121032683");
    }

    #[test]
    fn isni_with_x_check_digit() {
        let v = parse_isni("000000012103268X").expect("X check digit");
        assert_eq!(v.as_slice(), b"000000012103268X");
    }

    #[test]
    fn isni_lowercase_x_normalises() {
        let v = parse_isni("000000012103268x").expect("lowercase X");
        assert_eq!(v.as_slice(), b"000000012103268X");
    }

    #[test]
    fn isni_pattern_mismatch() {
        assert_eq!(parse_isni("123"), Err(ParseError::PatternMismatch));
        assert_eq!(
            parse_isni("000000012103268Y"),
            Err(ParseError::PatternMismatch),
        );
    }

    #[test]
    fn ipi_strips_prefix() {
        let v = parse_ipi("I-123456789").expect("prefixed");
        assert_eq!(v.as_slice(), b"123456789");
    }

    #[test]
    fn ipi_accepts_lengths() {
        for raw in ["123456789", "1234567890", "12345678901"] {
            assert_eq!(parse_ipi(raw).unwrap().as_slice(), raw.as_bytes(), "{raw}");
        }
    }

    #[test]
    fn ipi_pattern_mismatch() {
        assert_eq!(parse_ipi("12345678"), Err(ParseError::PatternMismatch));
        assert_eq!(parse_ipi("123456789012"), Err(ParseError::PatternMismatch));
        assert_eq!(parse_ipi("12345A789"), Err(ParseError::PatternMismatch));
    }
}
