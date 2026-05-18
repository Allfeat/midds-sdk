//! Tolerant parsers for canonical industry identifiers.
//!
//! Real-world inputs come in many shapes: ISWCs are quoted as
//! `T-034.524.680-1` or `T0345246801`, ISNIs are usually space-grouped, IPIs
//! are sometimes prefixed with `I-`. These parsers accept the common variants,
//! strip separators, uppercase letters and emit the strict canonical form
//! that `midds-traits::validate_*_format` expects on-chain.

use std::sync::LazyLock;

use midds_traits::{Ipi, Isni, Isrc, Iswc, Upc};
use regex::Regex;

use crate::error::ParseError;

// Each regex pattern is a `const &'static str` literal; `Regex::new` over a
// statically valid pattern cannot fail in practice. The `unwrap` here is
// the documented escape hatch for compile-time-guaranteed inputs.
#[allow(clippy::disallowed_methods)]
static ISWC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^T-?(\d{3})\.?(\d{3})\.?(\d{3})-?(\d)$").unwrap());

#[allow(clippy::disallowed_methods)]
static ISNI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4})[\s-]?(\d{4})[\s-]?(\d{4})[\s-]?(\d{3}[\dX])$").unwrap());

#[allow(clippy::disallowed_methods)]
static IPI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:I-)?(\d{9,11})$").unwrap());

// `[A-Z]{2}` country, `[A-Z0-9]{3}` registrant, `\d{2}` year, `\d{5}`
// designation. Separators are optional dashes between each segment.
#[allow(clippy::disallowed_methods)]
static ISRC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Z]{2})-?([A-Z0-9]{3})-?(\d{2})-?(\d{5})$").unwrap());

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

/// Parse a tolerant ISRC string into the canonical 12-byte form
/// (country + registrant + year + designation, no separators).
///
/// Accepted shapes (case-insensitive):
/// - `USRC17607839` (canonical, no separators)
/// - `US-RC1-76-07839` (separators between every segment)
/// - `US-RC1-7607839` / `USRC1-76-07839` (partial separators)
/// - leading / trailing whitespace
pub fn parse_isrc(s: &str) -> Result<Isrc, ParseError> {
    let upper = s.trim().to_ascii_uppercase();
    let caps = ISRC_RE
        .captures(&upper)
        .ok_or(ParseError::PatternMismatch)?;
    let mut out = String::with_capacity(12);
    for i in 1..=4 {
        out.push_str(&caps[i]);
    }
    Isrc::try_from(out.into_bytes()).map_err(|_| ParseError::OutOfBounds)
}

/// Parse a tolerant UPC / EAN string into the canonical digits-only form
/// (12 digits for UPC-A, 13 for EAN-13 / GTIN-13).
///
/// Accepted shapes: barcodes are quoted with all kinds of spacing and
/// hyphenation (`4 006381 333931`, `0-36000-29145-2`, `978-3-16-148410-0`),
/// so every ASCII space and `-` is stripped before the digit/length check.
/// The check digit is **not** verified here — that is the warning-only
/// `verify_upc_checksum` in `crate::checksum`.
pub fn parse_upc(s: &str) -> Result<Upc, ParseError> {
    let cleaned: Vec<u8> = s
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'-')
        .collect();
    if cleaned.len() != 12 && cleaned.len() != 13 {
        return Err(ParseError::PatternMismatch);
    }
    if !cleaned.iter().all(u8::is_ascii_digit) {
        return Err(ParseError::PatternMismatch);
    }
    Upc::try_from(cleaned).map_err(|_| ParseError::OutOfBounds)
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
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

    // ---- ISRC -------------------------------------------------------------

    #[test]
    fn isrc_canonical_round_trips() {
        let v = parse_isrc("USRC17607839").expect("canonical");
        assert_eq!(v.as_slice(), b"USRC17607839");
    }

    #[test]
    fn isrc_dashed_normalises() {
        let v = parse_isrc("US-RC1-76-07839").expect("dashed");
        assert_eq!(v.as_slice(), b"USRC17607839");
    }

    #[test]
    fn isrc_partial_dashes_normalise() {
        let v = parse_isrc("USRC1-76-07839").expect("partial");
        assert_eq!(v.as_slice(), b"USRC17607839");
        let v = parse_isrc("US-RC176-07839").expect("partial alt");
        assert_eq!(v.as_slice(), b"USRC17607839");
    }

    #[test]
    fn isrc_lowercase_normalises() {
        let v = parse_isrc("usrc17607839").expect("lowercase");
        assert_eq!(v.as_slice(), b"USRC17607839");
    }

    #[test]
    fn isrc_trim() {
        let v = parse_isrc("  USRC17607839  ").expect("padded");
        assert_eq!(v.as_slice(), b"USRC17607839");
    }

    #[test]
    fn isrc_pattern_mismatch_too_short() {
        assert_eq!(parse_isrc("USRC1760783"), Err(ParseError::PatternMismatch));
    }

    #[test]
    fn isrc_pattern_mismatch_too_long() {
        assert_eq!(
            parse_isrc("USRC176078390"),
            Err(ParseError::PatternMismatch),
        );
    }

    #[test]
    fn isrc_pattern_mismatch_bad_country() {
        // Country code must be alpha — digits in the first two slots fail.
        assert_eq!(parse_isrc("12RC17607839"), Err(ParseError::PatternMismatch));
    }

    #[test]
    fn isrc_pattern_mismatch_bad_year() {
        // Year segment must be exactly 2 digits.
        assert_eq!(parse_isrc("USRC1A607839"), Err(ParseError::PatternMismatch));
    }

    #[test]
    fn isrc_pattern_mismatch_bad_designation() {
        // Designation must be exactly 5 digits — letters are rejected.
        assert_eq!(parse_isrc("USRC1760783A"), Err(ParseError::PatternMismatch));
    }

    #[test]
    fn isrc_pattern_mismatch_empty() {
        assert_eq!(parse_isrc(""), Err(ParseError::PatternMismatch));
    }

    // ---- UPC / EAN --------------------------------------------------------

    #[test]
    fn upc_canonical_round_trips() {
        assert_eq!(
            parse_upc("036000291452").unwrap().as_slice(),
            b"036000291452"
        );
        assert_eq!(
            parse_upc("4006381333931").unwrap().as_slice(),
            b"4006381333931"
        );
    }

    #[test]
    fn upc_strips_separators_and_whitespace() {
        assert_eq!(
            parse_upc("0-36000-29145-2").unwrap().as_slice(),
            b"036000291452"
        );
        assert_eq!(
            parse_upc(" 4 006381 333931 ").unwrap().as_slice(),
            b"4006381333931"
        );
    }

    #[test]
    fn upc_pattern_mismatch() {
        assert_eq!(parse_upc("03600029145"), Err(ParseError::PatternMismatch));
        assert_eq!(
            parse_upc("40063813339311"),
            Err(ParseError::PatternMismatch)
        );
        assert_eq!(parse_upc("03600029145X"), Err(ParseError::PatternMismatch));
        assert_eq!(parse_upc(""), Err(ParseError::PatternMismatch));
    }
}
