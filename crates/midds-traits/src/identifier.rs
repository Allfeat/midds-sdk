//! Canonical industry-identifier aliases and their on-chain format rules.
//!
//! Every alias is a [`MiddsString`] (a bounded ASCII byte string) so the
//! types stay `no_std` / `MaxEncodedLen`-friendly. The paired
//! `validate_*_format` functions enforce **structure only** (length, charset,
//! prefix) — deliberately not check digits: real-world registries publish
//! records with bad checksums, so checksum verification is warning-only and
//! lives in `midds-validate`.

use bounded_collections::{BoundedVec, ConstU32};

use crate::error::MiddsFormatError;

/// A bounded ASCII byte string of capacity `N`, used for canonical identifiers.
pub type MiddsString<const N: u32> = BoundedVec<u8, ConstU32<N>>;

/// ISWC: `T` followed by 10 decimal digits (the last is the check digit).
pub type Iswc = MiddsString<11>;
/// ISNI: 16 chars — 15 decimal digits + a final digit or `X`.
pub type Isni = MiddsString<16>;
/// IPI: 9 to 11 decimal digits.
pub type Ipi = MiddsString<11>;
/// IPN: International Performer Number — exactly 8 decimal digits. Issued by
/// SCAPR's IPD registry to performers declared at a performer CMO. Distinct
/// from IPI (CISAC, songwriters/publishers) and ISNI (ISO, any party), so it
/// gets its own validator instead of being collapsed into [`Ipi`].
pub type Ipn = MiddsString<8>;
/// ISRC: 2 alpha country + 3 alphanumeric registrant + 2-digit year + 5 digits.
pub type Isrc = MiddsString<12>;
/// UPC / EAN barcode keying a `Release`: a GTIN-12 (UPC-A, 12 digits) or
/// GTIN-13 (EAN-13, 13 digits). Bound is 13 — the wider of the two.
pub type Upc = MiddsString<13>;

/// Validates the structure of an [`Iswc`]: `T` followed by 10 decimal digits.
/// Errors: [`MiddsFormatError::OutOfBounds`] on a wrong length,
/// [`MiddsFormatError::InvalidIdentifierStructure`] on a missing `T` prefix,
/// [`MiddsFormatError::InvalidCharset`] on a non-digit payload.
///
/// ```
/// use midds_traits::validate_iswc_format;
///
/// assert!(validate_iswc_format(b"T0345246801").is_ok());
/// assert!(validate_iswc_format(b"X0345246801").is_err());
/// ```
pub fn validate_iswc_format(b: &[u8]) -> Result<(), MiddsFormatError> {
    if b.len() != 11 {
        return Err(MiddsFormatError::OutOfBounds);
    }
    if b[0] != b'T' {
        return Err(MiddsFormatError::InvalidIdentifierStructure);
    }
    if !b[1..].iter().all(u8::is_ascii_digit) {
        return Err(MiddsFormatError::InvalidCharset);
    }
    Ok(())
}

/// Validates the structure of an [`Isni`]: 15 decimal digits plus a final
/// digit or `X`.
/// Errors: [`MiddsFormatError::OutOfBounds`] on a wrong length,
/// [`MiddsFormatError::InvalidCharset`] on any other malformed byte.
///
/// ```
/// use midds_traits::validate_isni_format;
///
/// assert!(validate_isni_format(b"000000012103268X").is_ok());
/// assert!(validate_isni_format(b"123").is_err());
/// ```
pub fn validate_isni_format(b: &[u8]) -> Result<(), MiddsFormatError> {
    if b.len() != 16 {
        return Err(MiddsFormatError::OutOfBounds);
    }
    if !b[..15].iter().all(u8::is_ascii_digit) {
        return Err(MiddsFormatError::InvalidCharset);
    }
    let last = b[15];
    if !last.is_ascii_digit() && last != b'X' {
        return Err(MiddsFormatError::InvalidCharset);
    }
    Ok(())
}

/// Validates the structure of an [`Ipi`]: 9 to 11 decimal digits.
/// Errors: [`MiddsFormatError::OutOfBounds`] on a wrong length,
/// [`MiddsFormatError::InvalidCharset`] on a non-digit byte.
///
/// ```
/// use midds_traits::validate_ipi_format;
///
/// assert!(validate_ipi_format(b"123456789").is_ok());
/// assert!(validate_ipi_format(b"12345678").is_err());
/// ```
pub fn validate_ipi_format(b: &[u8]) -> Result<(), MiddsFormatError> {
    if b.len() < 9 || b.len() > 11 {
        return Err(MiddsFormatError::OutOfBounds);
    }
    if !b.iter().all(u8::is_ascii_digit) {
        return Err(MiddsFormatError::InvalidCharset);
    }
    Ok(())
}

/// Validates the structure of an [`Ipn`]: exactly 8 decimal digits.
/// Errors: [`MiddsFormatError::OutOfBounds`] on a wrong length,
/// [`MiddsFormatError::InvalidCharset`] on a non-digit byte.
///
/// ```
/// use midds_traits::validate_ipn_format;
///
/// assert!(validate_ipn_format(b"12345678").is_ok());
/// assert!(validate_ipn_format(b"1234567A").is_err());
/// ```
pub fn validate_ipn_format(b: &[u8]) -> Result<(), MiddsFormatError> {
    if b.len() != 8 {
        return Err(MiddsFormatError::OutOfBounds);
    }
    if !b.iter().all(u8::is_ascii_digit) {
        return Err(MiddsFormatError::InvalidCharset);
    }
    Ok(())
}

/// Validates the structure of an [`Isrc`]: 2 uppercase country letters,
/// 3 alphanumeric registrant characters, then 7 decimal digits (2-digit
/// year + 5-digit designation).
/// Errors: [`MiddsFormatError::OutOfBounds`] on a wrong length,
/// [`MiddsFormatError::InvalidCharset`] on any malformed segment.
///
/// ```
/// use midds_traits::validate_isrc_format;
///
/// assert!(validate_isrc_format(b"USRC17607839").is_ok());
/// assert!(validate_isrc_format(b"usrc17607839").is_err());
/// ```
pub fn validate_isrc_format(b: &[u8]) -> Result<(), MiddsFormatError> {
    if b.len() != 12 {
        return Err(MiddsFormatError::OutOfBounds);
    }
    if !b[..2].iter().all(u8::is_ascii_uppercase) {
        return Err(MiddsFormatError::InvalidCharset);
    }
    if !b[2..5]
        .iter()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(MiddsFormatError::InvalidCharset);
    }
    if !b[5..].iter().all(u8::is_ascii_digit) {
        return Err(MiddsFormatError::InvalidCharset);
    }
    Ok(())
}

/// Validates the structure of a [`Upc`]: UPC-A (12 digits) or EAN-13 /
/// GTIN-13 (13 digits), all decimal. The trailing check digit is **not**
/// verified on-chain — like every other identifier, structure only; the
/// warning-only GTIN mod-10 verifier lives in `midds-validate`.
/// Errors: [`MiddsFormatError::OutOfBounds`] on a wrong length,
/// [`MiddsFormatError::InvalidCharset`] on a non-digit byte.
///
/// ```
/// use midds_traits::validate_upc_format;
///
/// assert!(validate_upc_format(b"036000291452").is_ok());
/// assert!(validate_upc_format(b"4006381333931").is_ok());
/// assert!(validate_upc_format(b"0360002914").is_err());
/// ```
pub fn validate_upc_format(b: &[u8]) -> Result<(), MiddsFormatError> {
    if b.len() != 12 && b.len() != 13 {
        return Err(MiddsFormatError::OutOfBounds);
    }
    if !b.iter().all(u8::is_ascii_digit) {
        return Err(MiddsFormatError::InvalidCharset);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity_scale_codec::MaxEncodedLen;

    /// Pin every identifier's `max_encoded_len` so a typo in the alias bound
    /// (`MiddsString<11>` accidentally widened to `<12>`) surfaces here
    /// before it becomes a wire-format break. SCALE encodes a `BoundedVec`
    /// as `Compact(len) ++ bytes`. The `Compact` length prefix is 1 byte
    /// for `len < 64`, 2 bytes for `64 <= len < 16384`.
    #[test]
    fn identifier_max_encoded_lens_are_stable() {
        assert_eq!(<Iswc as MaxEncodedLen>::max_encoded_len(), 1 + 11);
        assert_eq!(<Isni as MaxEncodedLen>::max_encoded_len(), 1 + 16);
        assert_eq!(<Ipi as MaxEncodedLen>::max_encoded_len(), 1 + 11);
        assert_eq!(<Ipn as MaxEncodedLen>::max_encoded_len(), 1 + 8);
        assert_eq!(<Isrc as MaxEncodedLen>::max_encoded_len(), 1 + 12);
        assert_eq!(<Upc as MaxEncodedLen>::max_encoded_len(), 1 + 13);
    }

    #[test]
    fn iswc_pass() {
        assert!(validate_iswc_format(b"T1234567890").is_ok());
        assert!(validate_iswc_format(b"T0000000000").is_ok());
    }

    #[test]
    fn iswc_wrong_length() {
        assert_eq!(
            validate_iswc_format(b"T123"),
            Err(MiddsFormatError::OutOfBounds)
        );
        assert_eq!(
            validate_iswc_format(b"T12345678901"),
            Err(MiddsFormatError::OutOfBounds)
        );
        assert_eq!(
            validate_iswc_format(b""),
            Err(MiddsFormatError::OutOfBounds)
        );
    }

    #[test]
    fn iswc_missing_t_prefix() {
        assert_eq!(
            validate_iswc_format(b"X1234567890"),
            Err(MiddsFormatError::InvalidIdentifierStructure),
        );
    }

    #[test]
    fn iswc_non_digit_payload() {
        assert_eq!(
            validate_iswc_format(b"T12345A7890"),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn isni_pass() {
        assert!(validate_isni_format(b"0000000121032683").is_ok());
        assert!(validate_isni_format(b"000000012103268X").is_ok());
    }

    #[test]
    fn isni_wrong_length() {
        assert_eq!(
            validate_isni_format(b"123"),
            Err(MiddsFormatError::OutOfBounds)
        );
        assert_eq!(
            validate_isni_format(b"00000001210326830"),
            Err(MiddsFormatError::OutOfBounds),
        );
    }

    #[test]
    fn isni_invalid_charset() {
        assert_eq!(
            validate_isni_format(b"00000001A1032683"),
            Err(MiddsFormatError::InvalidCharset),
        );
        assert_eq!(
            validate_isni_format(b"000000012103268Y"),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn ipi_pass() {
        assert!(validate_ipi_format(b"123456789").is_ok());
        assert!(validate_ipi_format(b"1234567890").is_ok());
        assert!(validate_ipi_format(b"12345678901").is_ok());
    }

    #[test]
    fn ipi_wrong_length() {
        assert_eq!(
            validate_ipi_format(b"12345678"),
            Err(MiddsFormatError::OutOfBounds)
        );
        assert_eq!(
            validate_ipi_format(b"123456789012"),
            Err(MiddsFormatError::OutOfBounds)
        );
    }

    #[test]
    fn ipi_invalid_charset() {
        assert_eq!(
            validate_ipi_format(b"12345A789"),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn ipn_pass() {
        assert!(validate_ipn_format(b"12345678").is_ok());
        assert!(validate_ipn_format(b"00000000").is_ok());
    }

    #[test]
    fn ipn_wrong_length() {
        assert_eq!(
            validate_ipn_format(b"1234567"),
            Err(MiddsFormatError::OutOfBounds),
        );
        assert_eq!(
            validate_ipn_format(b"123456789"),
            Err(MiddsFormatError::OutOfBounds),
        );
        assert_eq!(validate_ipn_format(b""), Err(MiddsFormatError::OutOfBounds));
    }

    #[test]
    fn ipn_invalid_charset() {
        assert_eq!(
            validate_ipn_format(b"1234567A"),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn isrc_pass() {
        assert!(validate_isrc_format(b"USRC17607839").is_ok());
        assert!(validate_isrc_format(b"GBAYE0601477").is_ok());
        assert!(validate_isrc_format(b"FR6V82050001").is_ok());
    }

    #[test]
    fn isrc_wrong_length() {
        assert_eq!(
            validate_isrc_format(b"US"),
            Err(MiddsFormatError::OutOfBounds)
        );
        assert_eq!(
            validate_isrc_format(b"USRC176078390"),
            Err(MiddsFormatError::OutOfBounds),
        );
    }

    #[test]
    fn isrc_invalid_country_code() {
        assert_eq!(
            validate_isrc_format(b"u5RC17607839"),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn isrc_invalid_year_or_designation() {
        assert_eq!(
            validate_isrc_format(b"USRC1760A839"),
            Err(MiddsFormatError::InvalidCharset),
        );
    }

    #[test]
    fn upc_pass() {
        assert!(validate_upc_format(b"036000291452").is_ok());
        assert!(validate_upc_format(b"4006381333931").is_ok());
        assert!(validate_upc_format(b"000000000000").is_ok());
    }

    #[test]
    fn upc_wrong_length() {
        assert_eq!(
            validate_upc_format(b"03600029145"),
            Err(MiddsFormatError::OutOfBounds),
        );
        assert_eq!(
            validate_upc_format(b"40063813339311"),
            Err(MiddsFormatError::OutOfBounds),
        );
        assert_eq!(validate_upc_format(b""), Err(MiddsFormatError::OutOfBounds));
    }

    #[test]
    fn upc_invalid_charset() {
        assert_eq!(
            validate_upc_format(b"03600029145X"),
            Err(MiddsFormatError::InvalidCharset),
        );
        assert_eq!(
            validate_upc_format(b"40063813339A"),
            Err(MiddsFormatError::InvalidCharset),
        );
    }
}
