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

use midds_traits::{Ipi, Isni, Iswc};
use rand::Rng;

/// Build an ISWC from a 9-digit work code with the correct CISAC mod-10
/// check digit appended.
///
/// `work_code` is taken modulo 1_000_000_000 — the ISWC work-code field
/// holds nine decimals.
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
        .map(|(i, d)| (*d as u32) * (i as u32 + 1))
        .sum();
    let check = (10 - sum % 10) % 10;

    let mut bytes = Vec::with_capacity(11);
    bytes.push(b'T');
    bytes.extend(digits.iter().map(|d| b'0' + d));
    bytes.push(b'0' + check as u8);
    Iswc::try_from(bytes).expect("11-byte ISWC fits the bound")
}

/// Map an index to a unique ISWC. Pure function over `index % 1_000_000_000`.
pub fn iswc_for_index(index: u32) -> Iswc {
    iswc_from_work_code(index)
}

/// Build an IPI of the requested length (9..=11) from a numeric stem.
///
/// The last digit is overwritten with the CISAC mod-10 check digit so the
/// returned identifier verifies via `midds_validate::verify_ipi_checksum`.
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
        .map(|(i, d)| (*d as u32) * (i as u32 + 1))
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

/// Build an ISNI from 15 raw decimal digits. The check char (digit or `X`)
/// is computed via ISO 7064 MOD 11,2 and appended.
///
/// `digits` is taken modulo 10 element-wise — pass arbitrary `u8` values.
pub fn isni_from_body(digits: [u8; 15]) -> Isni {
    let normalised: [u8; 15] = core::array::from_fn(|i| digits[i] % 10);
    let mut r: u32 = 0;
    for &d in &normalised {
        r = ((r + d as u32) * 2) % 11;
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

#[cfg(test)]
mod tests {
    use super::*;
    use midds_traits::{validate_ipi_format, validate_isni_format, validate_iswc_format};

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
        // Known case from midds-validate: T0345246802 (work code 034524680, check 2).
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
        // `ipi_from_stem` overwrites the last digit with the computed check.
        // For len=9, the leading 8 digits of the stem feed the mod-10 sum:
        // sum(1..=8 * digits) = 1+4+9+16+25+36+49+64 = 204 ⇒ check 6.
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
        // Lou Reed: 0000000121032683 → check digit 3.
        let isni = isni_from_body([0, 0, 0, 0, 0, 0, 0, 1, 2, 1, 0, 3, 2, 6, 8]);
        assert_eq!(isni.as_slice(), b"0000000121032683");
    }

    #[test]
    fn random_ids_validate() {
        let mut rng = crate::rng::seeded_rng(0xDEAD_BEEF);
        for _ in 0..32 {
            assert!(validate_ipi_format(ipi_random(&mut rng).as_slice()).is_ok());
            assert!(validate_isni_format(isni_random(&mut rng).as_slice()).is_ok());
        }
    }
}
