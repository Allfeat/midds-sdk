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

/// Bond information attached to a stored MIDDS record.
///
/// Replaces the original tuple shape (`(AccountId, Balance, Balance, bool)`)
/// with named fields. The struct is the wire shape: changing it is a
/// breaking SCALE change for every consumer that decodes it (RPC clients,
/// indexers), but the named fields make the contract self-documenting and
/// the layout extends-friendly via deliberate `enum`-based versioning.
#[derive(Encode, Decode, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DepositInfoOf<AccountId, Balance> {
    /// Account that owns the record (attribution holder). Allowed to
    /// `update` / `remove_own` while the commitment window is open. Equal to
    /// `bond_payer` for self-deposits; differs after `deposit_on_behalf`,
    /// where the operator paid on behalf of this owner.
    pub depositor: AccountId,
    /// Account whose balance posted the bond. All refunds (`remove_own`,
    /// `force_remove_refund`) and the post-finalization Treasury flow target
    /// this account, not `depositor`.
    pub bond_payer: AccountId,
    /// Currently held amount = base bond × `M_fast(t0)` × `M_slow(t0)` where
    /// `t0` is the deposit block. Re-pricing on `update` adjusts the base
    /// portion only — the multiplier premium captured at deposit time is
    /// preserved (cf. `docs/economics.md` §5.5).
    pub amount: Balance,
    /// Unmultiplied base bond (`DepositBase + DepositPerByte * size`) at
    /// deposit time. This is what `remove_own` refunds; the difference with
    /// `amount` is the multiplier premium that flows to the Treasury.
    pub base_bond: Balance,
    /// Whether the bond has already been moved to the Treasury at the end of
    /// the commitment window. Once `true` the record is permanent —
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
