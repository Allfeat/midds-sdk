//! Generic types surfaced by [`PalletApi`](super::PalletApi).
//!
//! All types here are MIDDS-payload agnostic — they only depend on the chain's
//! `MiddsId`, `Balance`, and `AccountId` shapes, so they apply uniformly across
//! every `pallet-midds` instance.

use midds_traits::MiddsId;

use crate::{Balance, ChainConfig};

/// Bond information attached to a stored MIDDS record. Re-export of the
/// runtime-API struct so the client doesn't carry a parallel type that
/// drifts on every wire-shape change.
pub type DepositInfo =
    midds_runtime_api::DepositInfoOf<<ChainConfig as subxt::Config>::AccountId, Balance>;

/// Inner representation of `sp_runtime::FixedU128`: a `u128` with 18 decimal
/// places of fixed-point precision. We surface the raw integer because
/// `midds-client` does not depend on `sp-runtime`; consumers convert via
/// [`fixed_u128_to_f64`] when display precision is enough.
pub type FixedU128Raw = u128;

/// FixedU128 accuracy — `10^18`. Matches `sp_runtime::FixedU128::DIV`.
const FIXED_U128_ACCURACY: u128 = 1_000_000_000_000_000_000;

/// Convert a raw FixedU128 (`value * 10^18`) into the floating-point ratio it
/// represents. Lossy for values needing more than ~15 decimal digits, but
/// sufficient for displaying multiplier ratios on a CLI dashboard.
pub fn fixed_u128_to_f64(raw: FixedU128Raw) -> f64 {
    raw as f64 / FIXED_U128_ACCURACY as f64
}

/// Receipt for a single deposited MIDDS record.
///
/// Bundles the allocated id with the on-chain bond breakdown (extracted
/// from the `Deposited` event so callers don't re-derive it from runtime
/// constants and current multipliers) and the inclusion fee paid.
///
/// The `tx_fee` semantics depend on the producing call:
/// - [`PalletApi::deposit_with_receipt_nonce`](super::PalletApi::deposit_with_receipt_nonce) —
///   exact fee, taken straight from the single `TransactionFeePaid` event.
/// - [`PalletApi::deposit_batch_with_receipts_nonce`](super::PalletApi::deposit_batch_with_receipts_nonce) —
///   the per-record share of the outer batch's `TransactionFeePaid` value
///   (`total_batch_fee / batch_size`, integer division). Identical for
///   every record in the batch by construction;
///   `TransactionPayment` only emits one event per outer extrinsic.
///
/// The other fields (`bond`, `base_bond`) are taken from per-inner
/// `Deposited` events and are exact in both cases.
#[derive(Debug, Clone, Copy)]
pub struct DepositReceipt {
    /// On-chain id allocated by the pallet.
    pub id: MiddsId,
    /// Total bond placed on hold against the depositor (`base_bond ×
    /// M_fast × M_slow`). This is the amount [`PalletApi`](super::PalletApi)
    /// callers should use when reasoning about the user-facing cost of a deposit.
    pub bond: Balance,
    /// Unmultiplied portion of the bond. `remove_own` refunds this exact
    /// value; the difference `bond − base_bond` is the multiplier premium
    /// transferred to the Treasury on remove or finalization.
    pub base_bond: Balance,
    /// Inclusion fee paid by the depositor — see the type-level doc for
    /// the exact-vs-amortised distinction. `None` if the runtime does not
    /// emit `TransactionPayment::TransactionFeePaid` (older runtimes).
    pub tx_fee: Option<Balance>,
}

/// Snapshot of the dynamic pricing inputs at the queried block.
#[derive(Debug, Clone, Copy)]
pub struct PricingSnapshot {
    /// Anti-DoS multiplier (per-block reactivity).
    pub fast_multiplier: FixedU128Raw,
    /// Anti-flood multiplier (rolling 7-day window).
    pub slow_multiplier: FixedU128Raw,
    /// Static target deposits per rolling 7-day window — by spec, runtime
    /// parameter, not on-chain state.
    pub weekly_target: u32,
    /// Sum of the 7 daily buckets — actual deposits seen in the last 7 days
    /// at day-resolution.
    pub weekly_actual: u32,
}

/// `(id, bond, base_bond)` extracted from a single `Deposited` event.
pub(crate) type DepositedEvent = (MiddsId, Balance, Balance);
