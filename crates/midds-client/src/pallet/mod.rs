//! Generic façade over a single `pallet-midds` instance.
//!
//! [`PalletApi`] holds the deposit/read primitives shared by every MIDDS
//! type. The runtime-side instance name (`"MusicalWorks"`, …) and runtime-API
//! trait name (`"MiddsApi"`) are injected at construction time by
//! [`MiddsClient`](crate::MiddsClient), keeping the namespace where it
//! actually lives — at the runtime — rather than as an associated constant
//! on the payload type.

pub mod api;
pub mod events;
pub(crate) mod names;
pub mod types;

pub use api::PalletApi;
pub use types::{DepositInfo, DepositReceipt, FixedU128Raw, PricingSnapshot, fixed_u128_to_f64};
