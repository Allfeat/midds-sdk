//! Shared types used by `pallet-midds`.

use frame_support::pallet_prelude::*;
use midds_traits::MiddsId;

/// A single bond contribution attached to a stored MIDDS record.
///
/// `amount` is the total currently held against `payer` — `base + premium`,
/// where `premium` is the surplus produced by the dynamic multipliers at the
/// moment this layer was created. Subsequent grow/shrink operations adjust
/// `base` (and `amount` by the same delta) but never re-price the premium —
/// extending a layer pays the raw `delta_base` (no current multiplier),
/// shrinking releases `delta_base` (premium stays absolute). This mirrors
/// `docs/economics.md` §5.5: the multiplier premium is non-refundable on
/// remove_own, sticky to the layer that paid it.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct BondLayer<AccountId, Balance> {
    pub payer: AccountId,
    pub amount: Balance,
    pub base: Balance,
}

/// Bond information attached to a stored MIDDS record.
///
/// Two-layer accounting backing the **web3 escape hatch**: the `sponsor_layer`
/// is the bond posted at deposit time (= depositor for self-deposits, ≠ for
/// `deposit_on_behalf`); the `owner_layer` materialises the **first time the
/// owner extends the payload via plain `update` on a sponsored record**, so
/// the owner never depends on the sponsor's balance to grow their own MIDDS.
///
/// On every edit the total base is recomputed from the new size and the
/// delta is routed to the caller's layer first, with LIFO overflow into the
/// other layer when the shrink exceeds the caller's own contribution. On
/// `remove_own` and the `force_remove_*` paths each layer settles
/// independently with its own `payer`. Both layers can survive with `base =
/// 0` and `amount = premium > 0` — the premium is sticky until finalize.
///
/// `payload_hash` is the BLAKE2b-256 of the SCALE-encoded payload at deposit
/// (or last edit) time and backs the exact-duplicate uniqueness index.
///
/// `finalized` flips to `true` when the commitment window elapses and every
/// layer's full hold is moved to the Treasury — at which point the record
/// becomes permanent and is no longer eligible for `remove_own`.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct Deposit<AccountId, Balance, BlockNumber, Hash> {
    /// Attribution holder — controls `update` / `remove_own` and is the
    /// canonical author of the MIDDS.
    pub depositor: AccountId,
    /// Block at which the deposit was first registered. Drives the commitment
    /// window for both layers; the window does **not** reset when the owner
    /// adds an `owner_layer` mid-flight.
    pub deposited_at: BlockNumber,
    /// Initial bond posted at `deposit` / `deposit_on_behalf`. `payer` equals
    /// `depositor` for self-deposits, differs for sponsored deposits.
    pub sponsor_layer: BondLayer<AccountId, Balance>,
    /// Owner-side contribution. `None` until the owner first extends a
    /// sponsored payload via plain `update`. Once `Some`, `payer == depositor`
    /// for the entire lifetime of the record (the owner cannot transfer
    /// authority to a different account without `remove_own` + re-deposit).
    pub owner_layer: Option<BondLayer<AccountId, Balance>>,
    pub payload_hash: Hash,
    pub finalized: bool,
}

/// Tag distinguishing the on-behalf actions inside a signed payload.
/// Prevents a `Deposit` signature from being replayed as an `Update` (and
/// vice-versa) since the `action` field is part of the SCALE-encoded message
/// the owner signs.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub enum OnBehalfAction {
    Deposit,
    Update,
}

/// Off-chain payload an owner signs to authorize a sponsored deposit.
///
/// The `operator` field binds the signature to a specific sponsor — a third
/// party who picks up the signature off-chain cannot submit it via a
/// different account. `nonce` carries the per-owner replay protection value
/// (`OnBehalfNonce<owner>`); the chain rejects a payload whose nonce does not
/// match the current value, then bumps it.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct DepositOnBehalfPayload<AccountId, M> {
    pub action: OnBehalfAction,
    pub item: M,
    pub operator: AccountId,
    pub nonce: u64,
}

/// Off-chain payload an owner signs to authorize a sponsored update.
///
/// `id` pins the signature to a specific MIDDS record so it cannot be
/// replayed against a different one the owner also owns. `operator` must
/// match the original `sponsor_layer.payer` of the record (enforced
/// on-chain) — only the deposit-time sponsor may extend their own hold.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct UpdateOnBehalfPayload<AccountId, M> {
    pub action: OnBehalfAction,
    pub id: MiddsId,
    pub item: M,
    pub operator: AccountId,
    pub nonce: u64,
}

/// Tear-down policy applied to each [`BondLayer`] of a removed record.
///
/// - `Refund` — return every layer's amount in full to its payer; nothing
///   flows to the Treasury. (`force_remove_refund`)
/// - `PremiumOnly` — refund each layer's `base` to its payer, send each
///   layer's premium to the Treasury. (`remove_own`)
/// - `Full` — transfer every layer's full amount to the Treasury.
///   (`finalize`, `force_remove_slash` pre-finalization)
#[derive(Clone, Copy)]
pub(crate) enum SettlementKind {
    Refund,
    PremiumOnly,
    Full,
}

/// Per-id action carried by [`Pallet::force_remove_many`].
///
/// Replaces the original `(Vec<MiddsId>, slash: bool)` shape that forced a
/// single fate on every entry — a typo intermixed with deliberate abuse in
/// the same batch had to be split into two extrinsics. Tagging each id
/// individually closes that limitation while keeping the bond accounting
/// per-record.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
pub enum RemovalKind {
    /// Refund the full held bond to each layer's payer (good-faith typo path).
    Refund,
    /// Send the held bonds to the Treasury (or no-op if already finalized);
    /// flagged abuse path.
    Slash,
}

/// `(id, kind)` pair fed into the bounded list of
/// [`Pallet::force_remove_many`].
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    MaxEncodedLen,
    TypeInfo,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
)]
pub struct RemovalRequest {
    pub id: MiddsId,
    pub kind: RemovalKind,
}
