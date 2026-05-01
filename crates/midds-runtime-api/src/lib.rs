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
use parity_scale_codec::Codec;
use sp_runtime::FixedU128;

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

        /// Bond information `(depositor, total_held, base_bond, finalized)`
        /// attached to a stored record. `total_held` is the multiplied
        /// amount currently on hold; `base_bond` is the unmultiplied portion
        /// that `remove_own` would refund.
        fn deposit_info(id: MiddsId) -> Option<(AccountId, Balance, Balance, bool)>;

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
