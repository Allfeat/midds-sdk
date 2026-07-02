//! Synthetic, checksum-correct identifier generators.
//!
//! These produce identifiers whose check digits actually verify (CISAC
//! mod-10 for ISWC/IPI, ISO 7064 MOD 11,2 for ISNI). On-chain validation in
//! `pallet-midds` is structural-only and accepts identifiers with broken
//! check digits, but the warning-only verifiers in `midds-validate` flag
//! them — so generating valid ones here keeps test corpora clean and lets
//! us prove the verifier path on success cases.
//!
//! The `_for_index` variants below derive an identifier from a sequence
//! number. They are the building block for `gen_n`: index → ISWC mapping
//! is injective over the relevant ranges, so a `Vec<MusicalWork>` of size
//! `N` produced by indices `0..N` is guaranteed to have unique ISWCs.

use midds_traits::{Ipi, Ipn, Isni, Isrc, Iswc, Upc};
use rand::Rng;

/// Build an ISWC from a 9-digit work code with the correct CISAC mod-10
/// check digit appended.
///
/// `work_code` is taken modulo `1_000_000_000` — the ISWC work-code field
/// holds nine decimals.
#[must_use]
pub fn iswc_from_work_code(work_code: u32) -> Iswc {
    let body = work_code % 1_000_000_000;
    let mut digits = [0u8; 9];
    let mut n = body;
    for slot in digits.iter_mut().rev() {
        *slot = (n % 10) as u8;
        n /= 10;
    }
    let sum: u32 = digits
        .iter()
        .enumerate()
        .map(|(i, d)| u32::from(*d) * (i as u32 + 1))
        .sum();
    let check = (10 - sum % 10) % 10;

    let mut bytes = Vec::with_capacity(11);
    bytes.push(b'T');
    bytes.extend(digits.iter().map(|d| b'0' + d));
    bytes.push(b'0' + check as u8);
    Iswc::try_from(bytes).expect("11-byte ISWC fits the bound")
}

/// Map an index to a unique ISWC. Pure function over `index % 1_000_000_000`.
#[must_use]
pub fn iswc_for_index(index: u32) -> Iswc {
    iswc_from_work_code(index)
}

/// Build an IPI of the requested length (9..=11) from a numeric stem.
///
/// The last digit is overwritten with the CISAC mod-10 check digit so the
/// returned identifier verifies via `midds_validate::verify_ipi_checksum`.
#[must_use]
pub fn ipi_from_stem(stem: u64, len: usize) -> Ipi {
    assert!((9..=11).contains(&len), "IPI length must be 9..=11");
    let modulus: u64 = 10u64.pow(len as u32);
    let body = stem % modulus;
    let mut digits = vec![0u8; len];
    let mut n = body;
    for slot in digits.iter_mut().rev() {
        *slot = (n % 10) as u8;
        n /= 10;
    }
    let sum: u32 = digits[..len - 1]
        .iter()
        .enumerate()
        .map(|(i, d)| u32::from(*d) * (i as u32 + 1))
        .sum();
    let check = (10 - sum % 10) % 10;
    digits[len - 1] = check as u8;

    let bytes: Vec<u8> = digits.into_iter().map(|d| b'0' + d).collect();
    Ipi::try_from(bytes).expect("9..=11-byte IPI fits the bound")
}

/// Random IPI with a correct check digit. Length picked uniformly at random
/// in 9..=11.
pub fn ipi_random<R: Rng + ?Sized>(rng: &mut R) -> Ipi {
    let len = rng.gen_range(9..=11);
    ipi_from_stem(rng.r#gen::<u64>(), len)
}

/// Build an IPN (exactly 8 decimal digits) from a numeric stem. IPN has no
/// public check digit specification on the IPD side, so the body is taken
/// modulo `10^11` and emitted verbatim — that's the strongest structural
/// guarantee `validate_ipn_format` needs.
#[must_use]
pub fn ipn_from_stem(stem: u64) -> Ipn {
    let body = stem % 100_000_000;
    let mut digits = [0u8; 8];
    let mut n = body;
    for slot in digits.iter_mut().rev() {
        *slot = (n % 10) as u8;
        n /= 10;
    }
    let bytes: Vec<u8> = digits.iter().map(|d| b'0' + d).collect();
    Ipn::try_from(bytes).expect("8-byte IPN fits the bound")
}

/// Random 8-digit IPN.
pub fn ipn_random<R: Rng + ?Sized>(rng: &mut R) -> Ipn {
    ipn_from_stem(rng.r#gen::<u64>())
}

/// Build an ISNI from 15 raw decimal digits. The check char (digit or `X`)
/// is computed via ISO 7064 MOD 11,2 and appended.
///
/// `digits` is taken modulo 10 element-wise — pass arbitrary `u8` values.
#[must_use]
pub fn isni_from_body(digits: [u8; 15]) -> Isni {
    let normalised: [u8; 15] = core::array::from_fn(|i| digits[i] % 10);
    let mut r: u32 = 0;
    for &d in &normalised {
        r = ((r + u32::from(d)) * 2) % 11;
    }
    let check = (12 - r) % 11;

    let mut bytes = Vec::with_capacity(16);
    bytes.extend(normalised.iter().map(|d| b'0' + d));
    bytes.push(if check == 10 {
        b'X'
    } else {
        b'0' + check as u8
    });
    Isni::try_from(bytes).expect("16-byte ISNI fits the bound")
}

/// Random ISNI with a correct ISO 7064 MOD 11,2 check char.
pub fn isni_random<R: Rng + ?Sized>(rng: &mut R) -> Isni {
    let mut body = [0u8; 15];
    rng.fill(&mut body[..]);
    isni_from_body(body)
}

/// Build a structurally valid ISRC from an `index`. Country defaults to
/// `US`, registrant cycles through `RC0..=RC9` (3 chars), year is `00..=99`,
/// designation is the last 5 digits of `index`. Pure function — same
/// `index` always produces the same ISRC.
///
/// ISRC has no check digit (ISO 3901), so "structurally valid" is the
/// strongest guarantee we can offer here.
#[must_use]
pub fn isrc_for_index(index: u32) -> Isrc {
    let registrant = (index / 100_000) % 10;
    let year = (index / 1_000_000) % 100;
    let designation = index % 100_000;
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(b"USRC");
    bytes.push(b'0' + registrant as u8);
    bytes.push(b'0' + (year / 10) as u8);
    bytes.push(b'0' + (year % 10) as u8);
    let mut tail = [0u8; 5];
    let mut n = designation;
    for slot in tail.iter_mut().rev() {
        *slot = b'0' + (n % 10) as u8;
        n /= 10;
    }
    bytes.extend_from_slice(&tail);
    Isrc::try_from(bytes).expect("12-byte ISRC fits the bound")
}

/// Random structurally valid ISRC. Country picked from a fixed pool of
/// real ISO 3166 alpha-2 codes, registrant is 3 random alphanumeric
/// uppercase chars, year is `00..=99`, designation is 5 random digits.
pub fn isrc_random<R: Rng + ?Sized>(rng: &mut R) -> Isrc {
    const COUNTRIES: &[&[u8]] = &[
        b"US", b"GB", b"FR", b"DE", b"JP", b"BR", b"NL", b"CA", b"AU", b"SE",
    ];
    const ALPHANUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(COUNTRIES[rng.gen_range(0..COUNTRIES.len())]);
    for _ in 0..3 {
        bytes.push(ALPHANUM[rng.gen_range(0..ALPHANUM.len())]);
    }
    for _ in 0..7 {
        bytes.push(b'0' + rng.gen_range(0..10) as u8);
    }
    Isrc::try_from(bytes).expect("12-byte ISRC fits the bound")
}

/// Compute the GTIN mod-10 check digit over `data` (12 digits for EAN-13).
///
/// Standard GS1 weighting: from the leftmost data digit, weights alternate
/// `1, 3, 1, 3, …`. Shared by [`upc_for_index`] / [`upc_random`] so the
/// generated barcodes pass `midds_validate::verify_upc_checksum`.
fn gtin_check_digit(data: &[u8]) -> u8 {
    let sum: u32 = data
        .iter()
        .enumerate()
        .map(|(i, d)| u32::from(*d) * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    ((10 - sum % 10) % 10) as u8
}

/// Build a 13-digit EAN-13 / GTIN-13 from an `index`, with the correct GS1
/// check digit appended. Injective over `index < 10^12`, so a batch built
/// from indices `0..N` always has unique canonical identifiers — the
/// `Release` analogue of [`isrc_for_index`]. EAN-13 (the longer of the two
/// accepted lengths) is generated so corpora exercise the wider bound.
#[must_use]
pub fn upc_for_index(index: u32) -> Upc {
    let body = u64::from(index) % 1_000_000_000_000;
    let mut digits = [0u8; 12];
    let mut n = body;
    for slot in digits.iter_mut().rev() {
        *slot = (n % 10) as u8;
        n /= 10;
    }
    let check = gtin_check_digit(&digits);
    let mut bytes = Vec::with_capacity(13);
    bytes.extend(digits.iter().map(|d| b'0' + d));
    bytes.push(b'0' + check);
    Upc::try_from(bytes).expect("13-byte EAN-13 fits the bound")
}

/// Random 13-digit EAN-13 with a correct GS1 check digit.
pub fn upc_random<R: Rng + ?Sized>(rng: &mut R) -> Upc {
    let mut digits = [0u8; 12];
    for d in &mut digits {
        *d = rng.gen_range(0..10);
    }
    let check = gtin_check_digit(&digits);
    let mut bytes = Vec::with_capacity(13);
    bytes.extend(digits.iter().map(|d| b'0' + d));
    bytes.push(b'0' + check);
    Upc::try_from(bytes).expect("13-byte EAN-13 fits the bound")
}

#[cfg(test)]
mod tests {
    use super::*;
    use midds_traits::{
        validate_ipi_format, validate_ipn_format, validate_isni_format, validate_isrc_format,
        validate_iswc_format, validate_upc_format,
    };

    #[test]
    fn iswc_from_work_code_is_structurally_valid() {
        for code in [0u32, 1, 999_999_999, 345_246_801, 12_345_678] {
            let iswc = iswc_from_work_code(code);
            assert_eq!(iswc.len(), 11);
            assert!(validate_iswc_format(iswc.as_slice()).is_ok());
        }
    }

    #[test]
    fn iswc_from_work_code_known_check_digit() {
        let iswc = iswc_from_work_code(34_524_680);
        assert_eq!(iswc.as_slice(), b"T0345246802");
    }

    #[test]
    fn iswc_for_index_is_unique_in_range() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..2048u32 {
            assert!(seen.insert(iswc_for_index(i)));
        }
    }

    #[test]
    fn ipi_from_stem_for_each_supported_length() {
        for len in 9..=11usize {
            let ipi = ipi_from_stem(123_456_789, len);
            assert_eq!(ipi.len(), len);
            assert!(validate_ipi_format(ipi.as_slice()).is_ok());
        }
    }

    #[test]
    fn ipi_known_check_digit() {
        let ipi = ipi_from_stem(123_456_789, 9);
        assert_eq!(ipi.as_slice(), b"123456786");
    }

    #[test]
    fn isni_from_body_is_structurally_valid() {
        let isni = isni_from_body([0; 15]);
        assert_eq!(isni.len(), 16);
        assert!(validate_isni_format(isni.as_slice()).is_ok());
    }

    #[test]
    fn isni_known_value() {
        let isni = isni_from_body([0, 0, 0, 0, 0, 0, 0, 1, 2, 1, 0, 3, 2, 6, 8]);
        assert_eq!(isni.as_slice(), b"0000000121032683");
    }

    #[test]
    fn random_ids_validate() {
        let mut rng = crate::rng::seeded_rng(0xDEAD_BEEF);
        for _ in 0..32 {
            assert!(validate_ipi_format(ipi_random(&mut rng).as_slice()).is_ok());
            assert!(validate_ipn_format(ipn_random(&mut rng).as_slice()).is_ok());
            assert!(validate_isni_format(isni_random(&mut rng).as_slice()).is_ok());
            assert!(validate_isrc_format(isrc_random(&mut rng).as_slice()).is_ok());
            assert!(validate_upc_format(upc_random(&mut rng).as_slice()).is_ok());
        }
    }

    #[test]
    fn ipn_from_stem_is_structurally_valid() {
        for stem in [0u64, 1, 12_345, 99_999_999_999, u64::MAX] {
            let ipn = ipn_from_stem(stem);
            assert_eq!(ipn.len(), 8);
            assert!(validate_ipn_format(ipn.as_slice()).is_ok());
        }
    }

    #[test]
    fn upc_for_index_is_structurally_valid() {
        for i in [0u32, 1, 99_999, 1_234_567, u32::MAX] {
            let upc = upc_for_index(i);
            assert_eq!(upc.len(), 13);
            assert!(validate_upc_format(upc.as_slice()).is_ok());
        }
    }

    #[test]
    fn upc_for_index_is_unique_in_range() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..2048u32 {
            assert!(seen.insert(upc_for_index(i)));
        }
    }

    #[test]
    fn upc_known_check_digit() {
        assert_eq!(gtin_check_digit(&[4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3]), 1,);
        let upc = upc_for_index(12_345);
        assert_eq!(&upc[..12], b"000000012345");
        assert!(validate_upc_format(upc.as_slice()).is_ok());
    }

    #[test]
    fn isrc_for_index_is_structurally_valid() {
        for i in [0u32, 1, 99_999, 1_234_567, u32::MAX / 2] {
            let isrc = isrc_for_index(i);
            assert_eq!(isrc.len(), 12);
            assert!(
                validate_isrc_format(isrc.as_slice()).is_ok(),
                "{:?}",
                core::str::from_utf8(isrc.as_slice()).unwrap_or("?"),
            );
        }
    }
}
