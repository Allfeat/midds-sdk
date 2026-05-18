//! Runtime APIs exposed by `pallet-midds` instances for off-chain consumers.
//!
//! Each MIDDS kind gets its **own** runtime-API trait — `MusicalWorkApi`,
//! `RecordingApi`, … — stamped from a single canonical method list by the
//! [`decl_midds_api!`] macro. They are byte-for-byte identical in shape;
//! only the trait name differs.
//!
//! Why not one generic `MiddsApi<Identifier, Item, …>` implemented per
//! instance? Substrate keys runtime-API dispatch on the **trait name** (the
//! WASM entrypoints are `<Trait>_<method>`), so a single trait could only
//! ever be implemented once per runtime — a second instance would collide.
//! A distinctly-named trait per kind is what lets the runtime
//! `impl_runtime_apis!` `MusicalWorkApi` *and* `RecordingApi` side by side.
//! Each trait stays generic over its `Identifier` / `Item` / `AccountId` /
//! `Balance` so the implementation and the RPC handler remain zero-dup.
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

/// Stamp out a per-MIDDS-kind runtime-API trait.
///
/// One macro body holds the canonical method list; each `$name` becomes a
/// distinctly-named [`sp_api::decl_runtime_apis!`] trait sharing that exact
/// shape. This is the multi-instance pivot: the runtime needs one API
/// **trait name** per `pallet-midds` instance (see the module docs), and
/// duplicating ~50 lines of signatures per kind would rot. Adding `Release`
/// later is a single extra line in the invocation below.
///
/// All traits are emitted in one `decl_runtime_apis!` invocation so the
/// generated `runtime_decl_for_*` modules and API-id constants stay
/// consistent.
macro_rules! decl_midds_api {
    ($( $(#[$attr:meta])* $name:ident ),+ $(,)?) => {
        sp_api::decl_runtime_apis! {
            $(
                $(#[$attr])*
                pub trait $name<Identifier, Item, AccountId, Balance>
                where
                    Identifier: Codec,
                    Item: Codec,
                    AccountId: Codec,
                    Balance: Codec,
                {
                    /// First page of `MiddsId`s registered against the
                    /// canonical industry identifier, sorted by id ascending
                    /// and capped at the pallet's `MAX_LOOKUP_LIMIT`. Returns
                    /// an empty vector when nothing matches — multi-claim is
                    /// the rule (`docs/economics.md` §1). Use
                    /// `lookup_by_identifier_paged` to walk past the cap.
                    fn lookup_by_identifier(identifier: Identifier) -> Vec<MiddsId>;

                    /// Paginated variant of `lookup_by_identifier`: returns
                    /// ids strictly greater than `after` (or all of them when
                    /// `after` is `None`), sorted ascending, capped at
                    /// `min(limit, MAX_LOOKUP_LIMIT)`. Resume the next page by
                    /// passing the last returned id as the new `after`.
                    fn lookup_by_identifier_paged(
                        identifier: Identifier,
                        after: Option<MiddsId>,
                        limit: u32,
                    ) -> Vec<MiddsId>;

                    /// Total number of `MiddsId`s registered against the
                    /// canonical identifier. Single linear scan over the
                    /// prefix — fine for any realistic claim count; callers
                    /// wanting just a "many?" signal should rely on
                    /// `lookup_by_identifier` returning the `MAX_LOOKUP_LIMIT`
                    /// cap instead.
                    fn count_by_identifier(identifier: Identifier) -> u32;

                    /// Fetch a stored MIDDS record by its on-chain id.
                    fn get(id: MiddsId) -> Option<Item>;

                    /// Bond information attached to a stored record. See
                    /// [`DepositInfoOf`] for the field layout.
                    fn deposit_info(id: MiddsId) -> Option<DepositInfoOf<AccountId, Balance>>;

                    /// Bond a new `deposit(item)` would lock for a payload of
                    /// `size` SCALE-encoded bytes at the current block. Useful
                    /// for pre-flight quoting (see `docs/economics.md` §15.5 —
                    /// `--max-price` UX).
                    fn current_deposit_price(size: u32) -> Balance;

                    /// `(M_fast, M_slow)` at the current block — the two
                    /// dynamic multipliers feeding `current_deposit_price`.
                    fn current_multipliers() -> (FixedU128, FixedU128);

                    /// Static target for the rolling 7-day window — surfaced
                    /// for UIs so they can render `weekly_actual /
                    /// weekly_target` as a load gauge.
                    fn weekly_target() -> u32;

                    /// Sum of the 7 daily buckets, approximating "deposits
                    /// seen in the last 7 days" with day-resolution.
                    fn weekly_actual() -> u32;
                }
            )+
        }
    };
}

decl_midds_api! {
    /// Runtime API for the `MusicalWork` `pallet-midds` instance
    /// (`Instance1` in the Allfeat melodie runtime, ISWC-keyed).
    MusicalWorkApi,
    /// Runtime API for the `Recording` `pallet-midds` instance
    /// (`Instance2` in the Allfeat melodie runtime, ISRC-keyed).
    RecordingApi,
    /// Runtime API for the `Release` `pallet-midds` instance
    /// (`Instance3` in the Allfeat melodie runtime, UPC/EAN-keyed).
    ReleaseApi,
}
