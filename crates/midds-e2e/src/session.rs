//! Per-run identifier minting.
//!
//! The e2e suite always points at the same chain (the user-managed
//! `allfeat --dev`), so deposits accumulate forever. To stop tests from
//! tripping on `AlreadyExists` after the second run, every payload built
//! through this module carries an ISWC (or future ISRC/UPC) derived from a
//! unique `(base_index + slot)` pair:
//!
//! - **`base_index`** — nanos since UNIX epoch, taken modulo `10^9` (the ISWC
//!   work-code field width). Read once per process via [`base_index`]. Two
//!   `cargo test` invocations a microsecond apart get disjoint bases.
//! - **slot** — process-local atomic counter. Each call to [`next_index`]
//!   bumps it, so parallel tests inside the same run also never share an
//!   identifier.
//!
//! Wrapping at `10^9` is fine: tests deposit a handful of records per run,
//! so the chance of an index collision after `10^9` distinct seconds
//! (~31.7 years of test runs) is negligible.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use midds_fixtures::musical_work::random_with_iswc_index;
use midds_types::MusicalWork;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

/// Modulus of the ISWC work-code field — see `iswc_for_index` in
/// `midds-fixtures`. Indices wrap at this boundary; staying below it
/// means the fixture's modulo doesn't silently fold two different slots
/// onto the same ISWC.
const INDEX_MODULUS: u32 = 1_000_000_000;

static BASE_INDEX: OnceLock<u32> = OnceLock::new();
static SLOT: AtomicU32 = AtomicU32::new(0);

/// Per-process base offset, lazily seeded from system time. Stable for the
/// life of the process, but a fresh process gets a fresh value.
pub fn base_index() -> u32 {
    *BASE_INDEX.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        (nanos % u64::from(INDEX_MODULUS)) as u32
    })
}

/// Reserve and return the next unique index for this process. Each call
/// advances the atomic slot — parallel tests calling this never see the
/// same value.
pub fn next_index() -> u32 {
    let slot = SLOT.fetch_add(1, Ordering::Relaxed);
    base_index().wrapping_add(slot) % INDEX_MODULUS
}

/// Build a fresh, valid [`MusicalWork`] with a session-unique ISWC.
///
/// The body of the work is randomised (RNG seeded from the reserved index)
/// so two consecutive calls produce visibly distinct payloads — handy when
/// a test wants to assert "the payload was actually updated" without having
/// to wire its own fixture state.
#[must_use]
pub fn fresh_musical_work() -> MusicalWork {
    let idx = next_index();
    let mut rng = ChaCha20Rng::seed_from_u64(u64::from(idx));
    random_with_iswc_index(&mut rng, idx)
}
