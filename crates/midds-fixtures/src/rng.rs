//! Seeded RNG helpers.
//!
//! Test code in this crate uses `ChaCha20Rng` for cross-process determinism:
//! `SmallRng`'s output is stable within a Rust release, but `ChaCha20Rng`'s
//! is fixed by spec, which matters for committed `storage_root` fixtures
//! generated on a CI machine and replayed locally.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

/// Build a `ChaCha20Rng` from a 64-bit seed.
///
/// Convenience wrapper around `ChaCha20Rng::seed_from_u64` so callers don't
/// need to depend on `rand_chacha` directly.
pub fn seeded_rng(seed: u64) -> ChaCha20Rng {
    ChaCha20Rng::seed_from_u64(seed)
}
