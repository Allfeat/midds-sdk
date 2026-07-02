//! Static corpora committed alongside the crate (`data/*.json`).
//!
//! Datasets are intentionally small in v1 — the API matters more than the
//! volume; consumers wanting realistic distributions can grow the JSON files
//! over time. Every identifier is checksum-correct (CISAC mod-10 for
//! ISWC/IPI, ISO 7064 MOD 11,2 for ISNI), enforced by the test below so
//! drift cannot creep in through edits.
//!
//! The JSONs are embedded with `include_str!`, so consumers don't need to
//! configure runtime data paths — the corpus is queryable from any process
//! that links this crate.

use bounded_collections::BoundedVec;
use midds_traits::{Ipi, Isni, Iswc};
use midds_types::Language;

const RAW_ISWC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/data/iswc_real_sample.json"
));
const RAW_IPI: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/ipi_codes.json"));
const RAW_ISNI: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/isni_codes.json"));
const RAW_LANGUAGES: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/languages.json"));
const RAW_TITLES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/data/titles_corpus.json"
));

fn parse_string_array(raw: &'static str) -> Vec<String> {
    serde_json::from_str(raw).expect("corpus JSON is a string array")
}

/// Iterator over the committed ISWC corpus.
///
/// Allocates per call so the iterator is `'static`-safe; the corpus is small
/// (≤ low hundreds), so the cost is irrelevant for tests.
pub fn iter_real_iswcs() -> impl Iterator<Item = Iswc> {
    parse_string_array(RAW_ISWC)
        .into_iter()
        .map(|s| BoundedVec::try_from(s.into_bytes()).expect("corpus ISWC fits the on-chain bound"))
}

/// Iterator over the committed IPI corpus.
pub fn iter_real_ipis() -> impl Iterator<Item = Ipi> {
    parse_string_array(RAW_IPI)
        .into_iter()
        .map(|s| BoundedVec::try_from(s.into_bytes()).expect("corpus IPI fits the on-chain bound"))
}

/// Iterator over the committed ISNI corpus.
pub fn iter_real_isnis() -> impl Iterator<Item = Isni> {
    parse_string_array(RAW_ISNI)
        .into_iter()
        .map(|s| BoundedVec::try_from(s.into_bytes()).expect("corpus ISNI fits the on-chain bound"))
}

/// Iterator over the committed language codes, parsed into [`Language`].
pub fn iter_languages() -> impl Iterator<Item = Language> {
    parse_string_array(RAW_LANGUAGES).into_iter().map(|s| {
        Language::from_code(s.as_bytes())
            .unwrap_or_else(|| panic!("unknown ISO 639-1 code in corpus: {s}"))
    })
}

/// Iterator over the committed title corpus. Titles are returned as owned
/// `String`s; convert with `try_from` if a `BoundedVec<u8, ConstU32<TITLE_MAX_LEN>>`
/// is needed.
pub fn iter_titles() -> impl Iterator<Item = String> {
    parse_string_array(RAW_TITLES).into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;
    use midds_traits::{validate_ipi_format, validate_isni_format, validate_iswc_format};

    #[test]
    fn iswc_corpus_is_structurally_valid() {
        let mut count = 0;
        for iswc in iter_real_iswcs() {
            validate_iswc_format(iswc.as_slice()).expect("iswc structurally valid");
            count += 1;
        }
        assert!(count > 0, "ISWC corpus must not be empty");
    }

    #[test]
    fn iswc_corpus_is_checksum_correct() {
        for iswc in iter_real_iswcs() {
            let bytes = iswc.as_slice();
            let digits: [u32; 10] = core::array::from_fn(|i| (bytes[i + 1] - b'0') as u32);
            let sum: u32 = digits[..9]
                .iter()
                .enumerate()
                .map(|(i, d)| d * (i as u32 + 1))
                .sum();
            let expected = (10 - sum % 10) % 10;
            assert_eq!(
                expected,
                digits[9],
                "broken ISWC check digit in corpus: {}",
                std::str::from_utf8(bytes).unwrap_or("<non-utf8>")
            );
        }
    }

    #[test]
    fn ipi_corpus_is_checksum_correct() {
        for ipi in iter_real_ipis() {
            let bytes = ipi.as_slice();
            validate_ipi_format(bytes).expect("ipi structurally valid");
            let len = bytes.len();
            let sum: u32 = bytes[..len - 1]
                .iter()
                .enumerate()
                .map(|(i, b)| ((b - b'0') as u32) * (i as u32 + 1))
                .sum();
            let expected = (10 - sum % 10) % 10;
            let actual = (bytes[len - 1] - b'0') as u32;
            assert_eq!(
                expected,
                actual,
                "broken IPI check digit in corpus: {}",
                std::str::from_utf8(bytes).unwrap_or("<non-utf8>")
            );
        }
    }

    #[test]
    fn isni_corpus_is_checksum_correct() {
        for isni in iter_real_isnis() {
            let bytes = isni.as_slice();
            validate_isni_format(bytes).expect("isni structurally valid");
            let mut r: u32 = 0;
            for &b in &bytes[..15] {
                r = ((r + (b - b'0') as u32) * 2) % 11;
            }
            let expected = (12 - r) % 11;
            let actual = if bytes[15] == b'X' {
                10
            } else {
                (bytes[15] - b'0') as u32
            };
            assert_eq!(
                expected,
                actual,
                "broken ISNI check digit in corpus: {}",
                std::str::from_utf8(bytes).unwrap_or("<non-utf8>")
            );
        }
    }

    #[test]
    fn languages_corpus_parses() {
        let codes: Vec<Language> = iter_languages().collect();
        assert!(!codes.is_empty());
        assert!(codes.contains(&Language::En));
    }

    #[test]
    fn titles_corpus_is_non_empty_strings() {
        let titles: Vec<String> = iter_titles().collect();
        assert!(!titles.is_empty());
        for t in titles {
            assert!(!t.is_empty(), "corpus title must not be empty");
        }
    }
}
