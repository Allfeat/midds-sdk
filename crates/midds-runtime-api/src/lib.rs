//! Runtime API exposed by `pallet-midds` instances for off-chain consumers.
//!
//! Generic over the four payload-shaped types of a single instance: its
//! canonical `Identifier`, its stored `Item`, and the chain's `AccountId`
//! and `Balance`. The runtime implements this trait once per instance.
//!
//! V2 (cf. `docs/economics.md` §12.2): exposes the live deposit price and
//! the `(M_fast, M_slow)` multipliers so dashboards and wallets can render
//! the network's current load and quote a deposit before signing.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use midds_traits::MiddsId;
use parity_scale_codec::{Codec, Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::FixedU128;

/// A single bond contribution attached to a stored MIDDS record.
///
/// Mirrors the on-chain `pallet_midds::types::BondLayer` so consumers see
/// the same per-payer accounting the pallet enforces internally. `amount`
/// is the currently held balance (base + premium); `base` is the
/// unmultiplied portion. Subsequent edits adjust both fields together —
/// extensions of an existing layer add `delta_base` to both, never
/// re-banking premium at the new multipliers.
#[derive(Encode, Decode, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BondLayerOf<AccountId, Balance> {
    /// Account whose balance backs this layer.
    pub payer: AccountId,
    /// Currently held amount (`base + premium` with `M < 1` cases capped to
    /// the held amount).
    pub amount: Balance,
    /// Unmultiplied base portion this layer covers.
    pub base: Balance,
}

/// Bond information attached to a stored MIDDS record.
///
/// Two-layer accounting backing the **web3 escape hatch**: the
/// `sponsor_layer` is always present (it represents the bond posted at
/// deposit time, by the depositor for self-deposits or by the sponsor for
/// `deposit_on_behalf`); the `owner_layer` is `Some` only when the owner
/// has extended a sponsored record via plain `update` and contributed
/// funds out of their own balance.
///
/// Each layer settles independently against its own `payer` on
/// `remove_own` / finalize / `force_remove_*` (cf.
/// `docs/economics.md`). Consumers should not assume `owner_layer.payer`
/// equals `depositor` defensively — the pallet enforces that invariant
/// but exposing the explicit pair keeps the wire shape unambiguous.
#[derive(Encode, Decode, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DepositInfoOf<AccountId, Balance> {
    /// Account that owns the record (attribution holder). Allowed to
    /// `update` / `remove_own` while the commitment window is open.
    pub depositor: AccountId,
    /// Initial bond layer posted at deposit. `payer == depositor` for
    /// self-deposits; differs after `deposit_on_behalf`.
    pub sponsor_layer: BondLayerOf<AccountId, Balance>,
    /// Owner-side bond layer. Materializes the first time the depositor
    /// extends a sponsored record via plain `update`; `None` for
    /// self-deposits and for sponsored records the owner has not touched
    /// in solo.
    pub owner_layer: Option<BondLayerOf<AccountId, Balance>>,
    /// Whether the bond has already been moved to the Treasury at the end
    /// of the commitment window. Once `true` the record is permanent —
    /// `remove_own` becomes a no-op and only `force_remove_*` can act.
    pub finalized: bool,
}

sp_api::decl_runtime_apis! {
    /// Generic lookup + pricing API for a single `pallet-midds` instance.
    pub trait MiddsApi<Identifier, Item, AccountId, Balance>
    where
        Identifier: Codec,
        Item: Codec,
        AccountId: Codec,
        Balance: Codec,
    {
        /// All `MiddsId`s registered against the canonical industry
        /// identifier. Returns an empty vector when nothing matches —
        /// multi-claim is the rule (`docs/economics.md` §1).
        fn lookup_by_identifier(identifier: Identifier) -> Vec<MiddsId>;

        /// Fetch a stored MIDDS record by its on-chain id.
        fn get(id: MiddsId) -> Option<Item>;

        /// Bond information attached to a stored record. See
        /// [`DepositInfoOf`] for the field layout.
        fn deposit_info(id: MiddsId) -> Option<DepositInfoOf<AccountId, Balance>>;

        /// Bond a new `deposit(item)` would lock for a payload of `size`
        /// SCALE-encoded bytes at the current block. Useful for pre-flight
        /// quoting (see `docs/economics.md` §15.5 — `--max-price` UX).
        fn current_deposit_price(size: u32) -> Balance;

        /// `(M_fast, M_slow)` at the current block — the two dynamic
        /// multipliers feeding `current_deposit_price`.
        fn current_multipliers() -> (FixedU128, FixedU128);

        /// Static target for the rolling 7-day window — surfaced for UIs so
        /// they can render `weekly_actual / weekly_target` as a load gauge.
        fn weekly_target() -> u32;

        /// Sum of the 7 daily buckets, approximating "deposits seen in the
        /// last 7 days" with day-resolution.
        fn weekly_actual() -> u32;
    }
}
