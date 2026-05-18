//! Façade for the `Recordings` instance of `pallet-midds`.
//!
//! Thin alias over the generic [`PalletApi`] parameterised on the
//! [`Recording`] payload — the exact sibling of [`crate::musical_works`]. The
//! deposit/read logic lives in `crate::pallet::api`; this module just pins
//! the runtime-side names.
//!
//! # Runtime status
//!
//! The `Recordings` pallet instance lives in the `../Allfeat` runtime
//! (`pallet_midds::<Instance2>`), which is **not** wired yet — that work is
//! deliberately out of scope here. This façade is therefore complete and
//! type-checked but cannot round-trip against a node until that instance and
//! its per-instance runtime-API impl land. [`RUNTIME_API_NAME`] in
//! particular is provisional: subxt addresses runtime APIs as
//! `<Trait>_<method>`, and the runtime will expose the second instance's API
//! under an instance-specific trait name. Reconcile this constant with the
//! runtime's actual `impl` when `../Allfeat` wires `Instance2`.

use midds_types::Recording;

use crate::pallet::PalletApi;

/// Name of the `pallet-midds` instance dedicated to recordings. Matches the
/// runtime's `construct_runtime!` entry once `Instance2` is wired.
pub const PALLET_NAME: &str = "Recordings";

/// Runtime API trait name implemented for this instance in `melodie-runtime`.
/// Provisional — see the module-level "Runtime status" note.
pub const RUNTIME_API_NAME: &str = "MiddsApi";

/// High-level handle for the Recordings pallet instance — alias of
/// [`PalletApi<'_, Recording>`][PalletApi]. Kept as a distinct public type so
/// call sites (`RecordingsApi<'_>`) and API docs read naturally, mirroring
/// [`MusicalWorksApi`](crate::MusicalWorksApi).
pub type RecordingsApi<'a> = PalletApi<'a, Recording>;
