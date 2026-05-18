//! Façade for the `MusicalWorks` instance of `pallet-midds`.
//!
//! Thin alias over the generic [`PalletApi`] parameterised on the
//! [`MusicalWork`] payload.
//! The deposit/read logic lives in `crate::pallet::api`; this module just
//! pins the runtime-side names so the rest of the SDK and external callers
//! keep their existing `client.musical_works()` entry point.

use midds_types::MusicalWork;

use crate::pallet::PalletApi;

/// Name of the `pallet-midds` instance dedicated to musical works.
pub const PALLET_NAME: &str = "MusicalWorks";

/// Runtime API trait name implemented for this instance in `melodie-runtime`
/// (`impl midds_runtime_api::MusicalWorkApi<...> for Runtime`). Each MIDDS
/// kind has its own runtime-API trait — Substrate keys dispatch on the
/// trait name — and subxt addresses runtime APIs as `<Trait>_<method>`, so
/// this stays in lock-step with the runtime impl.
pub const RUNTIME_API_NAME: &str = "MusicalWorkApi";

/// High-level handle for the MusicalWorks pallet instance — alias of
/// [`PalletApi<'_, MusicalWork>`][PalletApi]. Kept as a distinct public
/// type so existing call sites (`MusicalWorksApi<'_>`) and API docs
/// continue to read naturally.
pub type MusicalWorksApi<'a> = PalletApi<'a, MusicalWork>;
