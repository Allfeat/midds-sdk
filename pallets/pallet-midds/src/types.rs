//! Shared types used by `pallet-midds`.

use frame_support::pallet_prelude::*;

/// Bond information attached to a stored MIDDS record.
///
/// `amount` is the total currently held against the depositor — `base_bond +
/// premium`, where `premium` is the surplus produced by the dynamic
/// multipliers at deposit time. We keep the split explicit so `remove_own`
/// can refund the unmultiplied base while the premium goes to the Foundation
/// Treasury (cf. `docs/economics.md` §5.5 — empêche l'arbitrage burst → wait
/// → re-deposit).
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
    pub deposited_at: BlockNumber,
    pub amount: Balance,
    pub base_bond: Balance,
    pub payload_hash: Hash,
    pub finalized: bool,
}
