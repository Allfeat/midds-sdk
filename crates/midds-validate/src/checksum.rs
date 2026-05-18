//! Warning-only check-digit verifiers for canonical industry identifiers.
//!
//! Real-world MIDDS data published by collection societies regularly carries
//! bad check digits. The pallet therefore deliberately does not gate deposits
//! on these checks — they live here for tooling (CLI `validate`, client-side
//! lint) and always return `CheckResult` rather than `Result`.

use midds_traits::{Ipi, Isni, Isrc, Iswc, Upc, validate_isrc_format};

/// Outcome of a checksum verification. Always advisory — the pallet never
/// reads this, and `Fail` does not block a deposit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    /// Check digit matches.
    Pass,
    /// Check digit does not match — likely a typo, but registries do publish
    /// records with bad check digits.
    Fail,
    /// Identifier shape is malformed; running the algorithm makes no sense.
    NotApplicable,
}

/// Verify the ISWC check digit using the CISAC weighted mod-10 algorithm:
/// weights `1..=9` over the 9-digit work code, expected check digit is
/// `(10 − sum mod 10) mod 10`.
pub fn verify_iswc_checksum(iswc: &Iswc) -> CheckResult {
    let bytes = iswc.as_slice();
    if bytes.len() != 11 || bytes[0] != b'T' || !bytes[1..].iter().all(u8::is_ascii_digit) {
        return CheckResult::NotApplicable;
    }
    let digits: [u32; 10] = core::array::from_fn(|i| (bytes[i + 1] - b'0') as u32);
    let sum: u32 = digits[..9]
        .iter()
        .enumerate()
        .map(|(i, d)| d * (i as u32 + 1))
        .sum();
    let expected = (10 - sum % 10) % 10;
    if expected == digits[9] {
        CheckResult::Pass
    } else {
        CheckResult::Fail
    }
}

/// Verify the ISNI check char using ISO 7064 MOD 11,2 over the 15 leading
/// digits. The trailing char may be a digit (0–9) or `X` (representing 10).
pub fn verify_isni_checksum(isni: &Isni) -> CheckResult {
    let bytes = isni.as_slice();
    if bytes.len() != 16 || !bytes[..15].iter().all(u8::is_ascii_digit) {
        return CheckResult::NotApplicable;
    }
    let last = bytes[15];
    let parsed = if last == b'X' {
        10
    } else if last.is_ascii_digit() {
        (last - b'0') as u32
    } else {
        return CheckResult::NotApplicable;
    };

    let mut r: u32 = 0;
    for &b in &bytes[..15] {
        let d = (b - b'0') as u32;
        r = ((r + d) * 2) % 11;
    }
    let expected = (12 - r) % 11;
    if expected == parsed {
        CheckResult::Pass
    } else {
        CheckResult::Fail
    }
}

/// Verify an ISRC's structural shape.
///
/// **Important:** the ISRC standard ([ISO 3901]) does not define a check
/// digit, so there is no checksum to verify in the strict sense. This
/// function returns:
/// - [`CheckResult::Pass`] when the input matches the canonical
///   `CC-XXX-YY-NNNNN` shape (12 chars: 2 alpha country + 3 alphanumeric
///   registrant + 2-digit year + 5-digit designation), and
/// - [`CheckResult::NotApplicable`] otherwise.
///
/// Surfaced under the same name as the other verifiers so generic
/// validation tooling can dispatch on identifier kind without a special
/// case for ISRC. The caller is responsible for understanding that "Pass"
/// here does not assert the identifier exists in any registry — only that
/// it is structurally well-formed.
///
/// [ISO 3901]: https://www.iso.org/standard/64817.html
pub fn verify_isrc_checksum(isrc: &Isrc) -> CheckResult {
    match validate_isrc_format(isrc.as_slice()) {
        Ok(()) => CheckResult::Pass,
        Err(_) => CheckResult::NotApplicable,
    }
}

/// Best-effort weighted mod-10 sanity check on an IPI Name Number.
///
/// The real CISAC algorithm uses ISO 7064 MOD 101,2 with two trailing check
/// digits; this implementation is intentionally simpler — the plan calls for
/// warning-only mod-10, and IPI data in the wild is too noisy to gate on
/// either algorithm. Returns `NotApplicable` for shapes that don't match.
pub fn verify_ipi_checksum(ipi: &Ipi) -> CheckResult {
    let bytes = ipi.as_slice();
    let len = bytes.len();
    if !(9..=11).contains(&len) || !bytes.iter().all(u8::is_ascii_digit) {
        return CheckResult::NotApplicable;
    }
    let sum: u32 = bytes[..len - 1]
        .iter()
        .enumerate()
        .map(|(i, b)| ((b - b'0') as u32) * (i as u32 + 1))
        .sum();
    let expected = (10 - sum % 10) % 10;
    let actual = (bytes[len - 1] - b'0') as u32;
    if expected == actual {
        CheckResult::Pass
    } else {
        CheckResult::Fail
    }
}

/// Verify the UPC / EAN check digit using the standard GS1 mod-10
/// algorithm: starting from the rightmost data digit, weights alternate
/// `3, 1, 3, 1, …`; the expected check digit is `(10 − sum mod 10) mod 10`.
///
/// Works for both UPC-A (12 digits) and EAN-13 / GTIN-13 (13 digits) — the
/// right-aligned weighting is length-agnostic. Returns `NotApplicable` for
/// shapes that aren't 12/13 all-digit.
pub fn verify_upc_checksum(upc: &Upc) -> CheckResult {
    let bytes = upc.as_slice();
    if (bytes.len() != 12 && bytes.len() != 13) || !bytes.iter().all(u8::is_ascii_digit) {
        return CheckResult::NotApplicable;
    }
    let (data, check) = bytes.split_at(bytes.len() - 1);
    let sum: u32 = data
        .iter()
        .rev()
        .enumerate()
        .map(|(i, b)| ((b - b'0') as u32) * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    let expected = (10 - sum % 10) % 10;
    let actual = (check[0] - b'0') as u32;
    if expected == actual {
        CheckResult::Pass
    } else {
        CheckResult::Fail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use midds_traits::{Ipi, Isni, Isrc, Iswc, Upc};

    fn iswc(raw: &[u8]) -> Iswc {
        Iswc::try_from(raw.to_vec()).expect("iswc bound")
    }

    fn isni(raw: &[u8]) -> Isni {
        Isni::try_from(raw.to_vec()).expect("isni bound")
    }

    fn ipi(raw: &[u8]) -> Ipi {
        Ipi::try_from(raw.to_vec()).expect("ipi bound")
    }

    fn isrc(raw: &[u8]) -> Isrc {
        Isrc::try_from(raw.to_vec()).expect("isrc bound")
    }

    /// digits 0,3,4,5,2,4,6,8,0 → sum = 0+6+12+20+10+24+42+64+0 = 178
    /// → expected check (10 − 178 % 10) % 10 = 2.
    #[test]
    fn iswc_pass() {
        assert_eq!(
            verify_iswc_checksum(&iswc(b"T0345246802")),
            CheckResult::Pass
        );
    }

    #[test]
    fn iswc_fail() {
        assert_eq!(
            verify_iswc_checksum(&iswc(b"T0345246809")),
            CheckResult::Fail
        );
    }

    #[test]
    fn iswc_not_applicable_when_short() {
        // Length 11 is enforced by the BoundedVec; we instead probe the
        // structural guard via a wrong prefix.
        assert_eq!(
            verify_iswc_checksum(&iswc(b"X0345246802")),
            CheckResult::NotApplicable
        );
    }

    /// Lou Reed: ISNI 0000 0001 2103 2683 → check digit 3 by ISO 7064 MOD 11,2.
    #[test]
    fn isni_pass() {
        assert_eq!(
            verify_isni_checksum(&isni(b"0000000121032683")),
            CheckResult::Pass
        );
    }

    #[test]
    fn isni_fail() {
        assert_eq!(
            verify_isni_checksum(&isni(b"0000000121032680")),
            CheckResult::Fail
        );
    }

    #[test]
    fn isni_x_check_pass() {
        // Construct a synthetic ISNI whose ISO 7064 check evaluates to 10 ('X').
        // Forward-iterate r = ((r + d) * 2) mod 11 over fifteen 1s:
        //   1 → r=2; 2 → r=6; 3 → r=3; 4 → r=8; 5 → r=7; 6 → r=5;
        //   7 → r=1; 8 → r=4; 9 → r=10; 10 → r=0; 11 → r=2; 12 → r=6;
        //   13 → r=3; 14 → r=8; 15 → r=7
        // (12 − 7) mod 11 = 5, not 10. Easier: brute-force a string in tests.
        let mut found = None;
        'outer: for pad in [b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9'] {
            for tail in 0u8..=9 {
                let mut buf = [pad; 16];
                buf[14] = b'0' + tail;
                buf[15] = b'X';
                let mut r: u32 = 0;
                for &b in &buf[..15] {
                    r = ((r + (b - b'0') as u32) * 2) % 11;
                }
                if (12 - r) % 11 == 10 {
                    found = Some(buf);
                    break 'outer;
                }
            }
        }
        let buf = found.expect("at least one MOD 11,2 check evaluates to X");
        assert_eq!(verify_isni_checksum(&isni(&buf)), CheckResult::Pass);
    }

    #[test]
    fn ipi_pass() {
        // digits 1,2,3,4,5,6,7,8 → sum = 1+4+9+16+25+36+49+64 = 204
        // → expected check (10 − 204 % 10) % 10 = 6.
        assert_eq!(verify_ipi_checksum(&ipi(b"123456786")), CheckResult::Pass);
    }

    #[test]
    fn ipi_fail() {
        assert_eq!(verify_ipi_checksum(&ipi(b"123456789")), CheckResult::Fail);
    }

    #[test]
    fn ipi_not_applicable_when_too_short_to_be_meaningful() {
        // 9..=11 is enforced upstream; here we ensure non-digit bytes short-circuit.
        assert_eq!(
            verify_ipi_checksum(&ipi(b"123A56786")),
            CheckResult::NotApplicable
        );
    }

    // ISRC: structural-only verification, since ISO 3901 has no check digit.

    #[test]
    fn isrc_pass_real_world_examples() {
        for raw in [
            b"USRC17607839" as &[u8],
            b"GBAYE0601477",
            b"FR6V82050001",
            b"JPK001500001",
            b"DEAA51900001",
        ] {
            assert_eq!(
                verify_isrc_checksum(&isrc(raw)),
                CheckResult::Pass,
                "{:?}",
                raw
            );
        }
    }

    #[test]
    fn isrc_not_applicable_bad_country_code() {
        // Country slot must be alpha — digits invalidate the structure.
        assert_eq!(
            verify_isrc_checksum(&isrc(b"1SRC17607839")),
            CheckResult::NotApplicable
        );
    }

    #[test]
    fn isrc_not_applicable_lowercase_country() {
        assert_eq!(
            verify_isrc_checksum(&isrc(b"usRC17607839")),
            CheckResult::NotApplicable
        );
    }

    #[test]
    fn isrc_not_applicable_bad_year() {
        // Year segment must be exactly 2 digits.
        assert_eq!(
            verify_isrc_checksum(&isrc(b"USRC1A607839")),
            CheckResult::NotApplicable
        );
    }

    #[test]
    fn isrc_not_applicable_bad_designation() {
        assert_eq!(
            verify_isrc_checksum(&isrc(b"USRC1760783A")),
            CheckResult::NotApplicable
        );
    }

    #[test]
    fn isrc_not_applicable_lowercase_registrant() {
        // Registrant must be uppercase A-Z or digit; lowercase is rejected.
        assert_eq!(
            verify_isrc_checksum(&isrc(b"USrC17607839")),
            CheckResult::NotApplicable
        );
    }

    fn upc(raw: &[u8]) -> Upc {
        Upc::try_from(raw.to_vec()).expect("upc bound")
    }

    #[test]
    fn upc_pass_upc_a_and_ean_13() {
        // Classic GS1 worked examples: UPC-A 036000291452, EAN-13 4006381333931.
        assert_eq!(
            verify_upc_checksum(&upc(b"036000291452")),
            CheckResult::Pass
        );
        assert_eq!(
            verify_upc_checksum(&upc(b"4006381333931")),
            CheckResult::Pass
        );
    }

    #[test]
    fn upc_fail_wrong_check_digit() {
        assert_eq!(
            verify_upc_checksum(&upc(b"036000291453")),
            CheckResult::Fail
        );
        assert_eq!(
            verify_upc_checksum(&upc(b"4006381333930")),
            CheckResult::Fail
        );
    }

    #[test]
    fn upc_not_applicable_bad_shape() {
        assert_eq!(
            verify_upc_checksum(&upc(b"03600029145")),
            CheckResult::NotApplicable
        );
        assert_eq!(
            verify_upc_checksum(&upc(b"03600029145X")),
            CheckResult::NotApplicable
        );
    }
}
