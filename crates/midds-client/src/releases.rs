//! Façade for the `Releases` instance of `pallet-midds`.
//!
//! Thin alias over the generic [`PalletApi`] parameterised on the
//! [`Release`] payload — the exact sibling of [`crate::recordings`]. The
//! deposit/read logic lives in `crate::pallet::api`; this module just pins
//! the runtime-side names.
//!
//! # Runtime status
//!
//! The `Releases` pallet instance is wired in the `../Allfeat` melodie
//! runtime as `pallet_midds::<Instance3>` (pallet index 108), with its own
//! `impl midds_runtime_api::ReleaseApi<...> for Runtime`. subxt addresses
//! runtime APIs as `<Trait>_<method>`, so [`RUNTIME_API_NAME`] is the
//! instance-specific trait name `"ReleaseApi"`, distinct from the
//! `MusicalWork` / `Recording` traits because Substrate keys runtime-API
//! dispatch on the trait name.

use midds_types::Release;

use crate::pallet::PalletApi;

/// Name of the `pallet-midds` instance dedicated to releases. Matches the
/// runtime's `construct_runtime!` entry once `Instance3` is wired.
pub const PALLET_NAME: &str = "Releases";

/// Runtime API trait name implemented for this instance in `melodie-runtime`
/// (`impl midds_runtime_api::ReleaseApi<...> for Runtime`). Distinct from
/// `MusicalWork`'s / `Recording`'s trait because Substrate keys runtime-API
/// dispatch on the trait name; see the module-level "Runtime status" note.
pub const RUNTIME_API_NAME: &str = "ReleaseApi";

/// High-level handle for the Releases pallet instance — alias of
/// [`PalletApi<'_, Release>`][PalletApi]. Kept as a distinct public type so
/// call sites (`ReleasesApi<'_>`) and API docs read naturally, mirroring
/// [`RecordingsApi`](crate::RecordingsApi).
pub type ReleasesApi<'a> = PalletApi<'a, Release>;
