//! Shared types used by `pallet-midds`.

use frame_support::pallet_prelude::*;
use midds_traits::MiddsId;

/// Bond information attached to a stored MIDDS record.
///
/// `amount` is the total currently held against the bond payer — `base_bond +
/// premium`, where `premium` is the surplus produced by the dynamic
/// multipliers at deposit time. We keep the split explicit so `remove_own`
/// can refund the unmultiplied base while the premium goes to the Foundation
/// Treasury (cf. `docs/economics.md` §5.5 — empêche l'arbitrage burst → wait
/// → re-deposit).
///
/// `depositor` is the **attribution holder**: who controls update / remove_own,
/// who appears as the author of the MIDDS. `bond_payer` is the **financial
/// sponsor**: against whom the bond is held and to whom refunds (or the
/// `force_remove_refund` indemnity) flow. For non-sponsored deposits both
/// fields are equal. They diverge only after `deposit_on_behalf`, where an
/// operator pays the bond on behalf of an owner authorized via off-chain
/// signature — the on_behalf model imported from `pallet-ats`.
///
/// `payload_hash` is the BLAKE2b-256 of the SCALE-encoded payload at deposit
/// (or last edit) time and backs the exact-duplicate uniqueness index.
///
/// `finalized` flips to `true` when the commitment window elapses and the
/// bond is moved to the Treasury — at which point the record becomes
/// permanent and is no longer eligible for `remove_own`.
#[derive(
    Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug,
)]
pub struct Deposit<AccountId, Balance, BlockNumber, Hash> {
    pub depositor: AccountId,
    pub bond_payer: AccountId,
    pub deposited_at: BlockNumber,
    pub amount: Balance,
    pub base_bond: Balance,
    pub payload_hash: Hash,
    pub finalized: bool,
}

/// Tag distinguishing the two on-behalf actions inside a signed payload.
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
/// match the original `bond_payer` of the record (enforced on-chain) — the
/// pallet only allows the same sponsor to extend their bond exposure.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct UpdateOnBehalfPayload<AccountId, M> {
    pub action: OnBehalfAction,
    pub id: MiddsId,
    pub item: M,
    pub operator: AccountId,
    pub nonce: u64,
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
    /// Refund the full held bond to the depositor (good-faith typo path).
    Refund,
    /// Send the held bond to the Treasury (or no-op if already finalized);
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
