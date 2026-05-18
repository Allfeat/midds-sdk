//! Shared test fixtures for the MIDDS SDK.
//!
//! Single source of truth for "what a plausible MIDDS looks like": all five
//! testing layers (unit, property, mass injection, runtime integration,
//! end-to-end CLI) consume this crate so that a payload that passes here is
//! the same payload that passes elsewhere.
//!
//! # Public API surface
//!
//! - [`MiddsFixtures`] — generic per-MIDDS-type extension trait. Each type
//!   ships a unit struct (e.g. [`musical_work::MusicalWorkFixtures`]) that
//!   implements it, so generic test scaffolding can be written once over
//!   `F: MiddsFixtures` instead of being copy-pasted per MIDDS.
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
//!
//! # Adding a new MIDDS type to the corpus
//!
//! 1. Create a sibling module to `musical_work/` (e.g. `recording/`) with
//!    a `RecordingFixtures` unit struct.
//! 2. Implement [`MiddsFixtures`] on it.
//! 3. The pallet/property/mass-injection harnesses pick it up generically;
//!    no scaffolding duplication.

pub mod identifiers;
pub mod musical_work;
pub mod pathological;
pub mod recording;
pub mod release;
pub mod rng;

pub use musical_work::MusicalWorkBuilder;
pub use recording::RecordingBuilder;
pub use release::ReleaseBuilder;
pub use rng::seeded_rng;

use midds_traits::Midds;
use midds_types::{MusicalWork, Recording, Release};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

/// Generic extension trait implemented by per-MIDDS-type fixture units.
///
/// A `MiddsFixtures` impl bundles the four scaffolding entry points test
/// layers need: deterministic bulk generation, a proptest strategy, a
/// committed corpus, and a set of pathological / boundary payloads.
///
/// `proptest` and `corpus` are feature-gated to mirror the cargo features
/// of this crate — code generic over `MiddsFixtures` should `cfg`-gate
/// access symmetrically.
pub trait MiddsFixtures {
    /// MIDDS payload type this fixture set generates.
    type Item: Midds;

    /// Deterministic bulk generation. Same `(seed, count)` produces the same
    /// `Vec<Self::Item>` byte-for-byte across processes.
    fn gen_n(seed: u64, count: u32) -> Vec<Self::Item>;

    /// Boundary payloads (size extremes, charset edges, …) that pass
    /// `validate_format`. Tests that need invalid payloads use a separate
    /// surface — see e.g. `pathological::invalid_*` constructors.
    fn pathological() -> Vec<Self::Item>;

    /// Realistic payload whose canonical identifier is derived from `index`,
    /// so indices `0..N` yield N distinct identifiers (`IdentifierIndex` in
    /// `pallet-midds` then accepts all N). The building block of [`gen_n`]
    /// and the `Real` size class of `midds-cli`'s `bench fees`.
    fn random_with_index<R: rand::Rng + ?Sized>(rng: &mut R, index: u32) -> Self::Item;

    /// Minimum-size payload carrying an index-unique canonical identifier.
    /// Backs the `Mixed` size class without colliding identifiers.
    fn min_size_with_index(index: u32) -> Self::Item;

    /// Maximum-size payload carrying an index-unique canonical identifier.
    /// Backs the `Max` / `Mixed` size classes without colliding identifiers.
    fn max_size_with_index(index: u32) -> Self::Item;

    /// Iterator over the committed corpus, when one ships for this MIDDS
    /// type. Returns an empty vector when no corpus is bundled.
    #[cfg(feature = "corpus")]
    fn corpus() -> Vec<Self::Item>;

    /// `proptest::Strategy` over valid payloads. Implementations are
    /// expected to honour `validate_format` by construction so property
    /// tests don't need `prop_assume` discards.
    #[cfg(feature = "proptest")]
    fn strategy() -> proptest::strategy::BoxedStrategy<Self::Item>;
}

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

/// Deterministically generate `count` distinct `Recording` records.
///
/// Same reproducibility contract as [`gen_n`]: identical `(seed, count)`
/// always yields the same `Vec<Recording>` byte-for-byte, including across
/// processes (`ChaCha20Rng`). Each record's ISRC is derived from its sequence
/// number so the canonical identifiers are guaranteed unique within the batch.
pub fn gen_n_recording(seed: u64, count: u32) -> Vec<Recording> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    (0..count)
        .map(|i| recording::random_with_isrc_index(&mut rng, i))
        .collect()
}

/// Deterministically generate `count` distinct `Release` records.
///
/// Same reproducibility contract as [`gen_n`]: identical `(seed, count)`
/// always yields the same `Vec<Release>` byte-for-byte, including across
/// processes (`ChaCha20Rng`). Each record's UPC is derived from its sequence
/// number so the canonical identifiers are guaranteed unique within the batch.
pub fn gen_n_release(seed: u64, count: u32) -> Vec<Release> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    (0..count)
        .map(|i| release::random_with_upc_index(&mut rng, i))
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

    #[test]
    fn gen_n_recording_is_deterministic_and_unique() {
        use midds_traits::Midds as _;
        let a = gen_n_recording(0xDEAD_BEEF, 64);
        let b = gen_n_recording(0xDEAD_BEEF, 64);
        assert_eq!(a, b);
        let mut ids: Vec<_> = a.iter().map(|r| r.identifier()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), a.len(), "ISRCs must be unique");
        for r in &a {
            r.validate_format().expect("generated payload validates");
        }
    }

    /// Drives the `MiddsFixtures` impl through its four entry points to
    /// catch a regression where one diverges (e.g. `pathological()` returns
    /// invalid payloads, or `corpus()` panics under feature combinations).
    #[test]
    fn musical_work_fixtures_trait_is_consistent() {
        use crate::musical_work::MusicalWorkFixtures;
        use midds_traits::Midds as _;

        let bulk = <MusicalWorkFixtures as MiddsFixtures>::gen_n(0xCAFE, 8);
        assert_eq!(bulk.len(), 8);
        for w in &bulk {
            w.validate_format().expect("gen_n payload validates");
        }

        let pathological = <MusicalWorkFixtures as MiddsFixtures>::pathological();
        assert!(!pathological.is_empty());
        for w in &pathological {
            w.validate_format()
                .expect("pathological boundary payload validates");
        }

        #[cfg(feature = "corpus")]
        {
            let corpus = <MusicalWorkFixtures as MiddsFixtures>::corpus();
            for w in &corpus {
                w.validate_format().expect("corpus payload validates");
            }
        }
    }

    #[test]
    fn gen_n_release_is_deterministic_and_unique() {
        use midds_traits::Midds as _;
        let a = gen_n_release(0xDEAD_BEEF, 64);
        let b = gen_n_release(0xDEAD_BEEF, 64);
        assert_eq!(a, b);
        let mut ids: Vec<_> = a.iter().map(|r| r.identifier()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), a.len(), "UPCs must be unique");
        for r in &a {
            r.validate_format().expect("generated payload validates");
        }
    }

    /// Same consistency sweep for `ReleaseFixtures`, including the
    /// index-keyed size-class constructors that back `bench fees`.
    #[test]
    fn release_fixtures_trait_is_consistent() {
        use crate::release::ReleaseFixtures;
        use midds_traits::Midds as _;

        let bulk = <ReleaseFixtures as MiddsFixtures>::gen_n(0xCAFE, 8);
        assert_eq!(bulk.len(), 8);
        for r in &bulk {
            r.validate_format().expect("gen_n payload validates");
        }

        let pathological = <ReleaseFixtures as MiddsFixtures>::pathological();
        assert!(!pathological.is_empty());
        for r in &pathological {
            r.validate_format()
                .expect("pathological boundary payload validates");
        }

        let mut rng = seeded_rng(0x5EED);
        let mut ids = Vec::new();
        for i in 0..8u32 {
            let real = <ReleaseFixtures as MiddsFixtures>::random_with_index(&mut rng, i);
            let min = <ReleaseFixtures as MiddsFixtures>::min_size_with_index(i);
            let max = <ReleaseFixtures as MiddsFixtures>::max_size_with_index(i);
            for r in [&real, &min, &max] {
                r.validate_format().expect("index-keyed payload validates");
            }
            ids.push(min.identifier().clone());
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 8, "min_size_with_index UPCs must be unique");

        #[cfg(feature = "corpus")]
        {
            let corpus = <ReleaseFixtures as MiddsFixtures>::corpus();
            assert!(!corpus.is_empty());
            for r in &corpus {
                r.validate_format().expect("corpus payload validates");
            }
        }
    }

    /// Same consistency sweep for `RecordingFixtures`, including the
    /// index-keyed size-class constructors that back `bench fees`.
    #[test]
    fn recording_fixtures_trait_is_consistent() {
        use crate::recording::RecordingFixtures;
        use midds_traits::Midds as _;

        let bulk = <RecordingFixtures as MiddsFixtures>::gen_n(0xCAFE, 8);
        assert_eq!(bulk.len(), 8);
        for r in &bulk {
            r.validate_format().expect("gen_n payload validates");
        }

        let pathological = <RecordingFixtures as MiddsFixtures>::pathological();
        assert!(!pathological.is_empty());
        for r in &pathological {
            r.validate_format()
                .expect("pathological boundary payload validates");
        }

        // Index-keyed size classes must validate and carry distinct ISRCs.
        let mut rng = seeded_rng(0x5EED);
        let mut ids = Vec::new();
        for i in 0..8u32 {
            let real = <RecordingFixtures as MiddsFixtures>::random_with_index(&mut rng, i);
            let min = <RecordingFixtures as MiddsFixtures>::min_size_with_index(i);
            let max = <RecordingFixtures as MiddsFixtures>::max_size_with_index(i);
            for r in [&real, &min, &max] {
                r.validate_format().expect("index-keyed payload validates");
            }
            ids.push(min.identifier().clone());
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 8, "min_size_with_index ISRCs must be unique");

        #[cfg(feature = "corpus")]
        {
            let corpus = <RecordingFixtures as MiddsFixtures>::corpus();
            assert!(!corpus.is_empty());
            for r in &corpus {
                r.validate_format().expect("corpus payload validates");
            }
        }
    }
}
