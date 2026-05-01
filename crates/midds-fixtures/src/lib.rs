//! Shared test fixtures for the MIDDS SDK.
//!
//! Single source of truth for "what a plausible MIDDS looks like": all five
//! testing layers (unit, property, mass injection, runtime integration,
//! end-to-end CLI) consume this crate so that a payload that passes here is
//! the same payload that passes elsewhere.
//!
//! # Public API surface
//!
//! - [`musical_work::MusicalWorkBuilder`] — fluent builder over already-bounded
//!   bytes, producing a `MusicalWork::V1`. Test-ergonomic: panics on bound
//!   overflow rather than aggregating errors. See `midds_validate::MusicalWorkBuilder`
//!   for the parser-tolerant runtime variant.
//! - [`identifiers`] — synthetic, checksum-correct ISWC / IPI / ISNI generators.
//! - [`pathological`] — borderline payloads (max size, charset edges, ...).
//! - [`gen_n`] — deterministic bulk generation seeded for reproducibility.
//! - `musical_work::strategy` (feature `proptest`) — `proptest::Strategy`
//!   implementations for property tests.
//! - `musical_work::corpus` (feature `corpus`) — iterators over committed
//!   datasets (`data/*.json`).

pub mod identifiers;
pub mod musical_work;
pub mod pathological;
pub mod rng;

pub use musical_work::MusicalWorkBuilder;
pub use rng::seeded_rng;

use midds_types::MusicalWork;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

/// Deterministically generate `count` distinct `MusicalWork` records.
///
/// Reproducibility contract: same `(seed, count)` always yields the same
/// `Vec<MusicalWork>` byte-for-byte, including across processes — backed by
/// `ChaCha20Rng`. Each record's ISWC is derived from its sequence number so
/// the canonical identifiers are guaranteed unique within the batch.
///
/// Used by mass-injection tests and the planned `midds-cli seed` command.
pub fn gen_n(seed: u64, count: u32) -> Vec<MusicalWork> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    (0..count)
        .map(|i| musical_work::random_with_iswc_index(&mut rng, i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_n_is_deterministic() {
        let a = gen_n(0xDEAD_BEEF, 32);
        let b = gen_n(0xDEAD_BEEF, 32);
        assert_eq!(a, b);
    }

    #[test]
    fn gen_n_yields_unique_iswcs() {
        use midds_traits::Midds as _;
        let works = gen_n(42, 256);
        let mut ids: Vec<_> = works.iter().map(|w| w.identifier()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), works.len(), "ISWCs must be unique");
    }

    #[test]
    fn gen_n_payloads_pass_on_chain_validation() {
        use midds_traits::Midds as _;
        for w in gen_n(7, 64) {
            w.validate_format().expect("generated payload validates");
        }
    }
}
