//! Façade for the `Recordings` instance of `pallet-midds`.
//!
//! Thin alias over the generic [`PalletApi`] parameterised on the
//! [`Recording`] payload — the exact sibling of [`crate::musical_works`]. The
//! deposit/read logic lives in `crate::pallet::api`; this module just pins
//! the runtime-side names.
//!
//! # Runtime status
//!
//! The `Recordings` pallet instance is wired in the `../Allfeat` melodie
//! runtime as `pallet_midds::<Instance2>` (pallet index 107), with its own
//! `impl midds_runtime_api::RecordingApi<...> for Runtime`. subxt addresses
//! runtime APIs as `<Trait>_<method>`, so [`RUNTIME_API_NAME`] is the
//! instance-specific trait name `"RecordingApi"` — reconciled with the
//! runtime impl (no longer provisional).

use midds_types::Recording;

use crate::pallet::PalletApi;

/// Name of the `pallet-midds` instance dedicated to recordings. Matches the
/// runtime's `construct_runtime!` entry once `Instance2` is wired.
pub const PALLET_NAME: &str = "Recordings";

/// Runtime API trait name implemented for this instance in `melodie-runtime`
/// (`impl midds_runtime_api::RecordingApi<...> for Runtime`). Distinct from
/// `MusicalWork`'s trait because Substrate keys runtime-API dispatch on the
/// trait name; see the module-level "Runtime status" note.
pub const RUNTIME_API_NAME: &str = "RecordingApi";

/// High-level handle for the Recordings pallet instance — alias of
/// [`PalletApi<'_, Recording>`][PalletApi]. Kept as a distinct public type so
/// call sites (`RecordingsApi<'_>`) and API docs read naturally, mirroring
/// [`MusicalWorksApi`](crate::MusicalWorksApi).
pub type RecordingsApi<'a> = PalletApi<'a, Recording>;
