//! Internal helpers for `pallet-midds`.
//!
//! Hosts the bond accounting (`compute_base_bond`, `adjust_hold`,
//! `settle_bond`), the lifecycle bodies shared by extrinsics (`do_deposit`,
//! `apply_edit`, `do_finalize`, `do_force_remove_*`), the on-behalf signature
//! verification, and the runtime-API readers that bridge the bond formula
//! and the multipliers (`current_deposit_price`, `lookup_by_identifier`).
//!
//! Pure pricing dynamics (multiplier adjustment, daily bucket rotation) live
//! in [`crate::multipliers`].

use crate::{BalanceOf, DepositOf, pallet::*, types::*};
use alloc::vec::Vec;
use frame_support::{
    pallet_prelude::*,
    traits::{
        fungible::{Mutate, MutateHold},
        tokens::{Precision, Preservation},
    },
};
use midds_traits::{Midds, MiddsId};
use parity_scale_codec::Encode;
use sp_runtime::traits::{Hash, Saturating, Verify, Zero};

impl<T: Config<I>, I: 'static> Pallet<T, I> {
    fn deposit_reason() -> T::RuntimeHoldReason {
        HoldReason::<I>::Deposit.into()
    }

    /// Format gate (charset / length / structure) shared by every extrinsic
    /// that accepts a fresh `T::Midds`. Wraps the trait-level
    /// `Midds::validate_format` so a single `Error::InvalidFormat` covers
    /// every shape mismatch.
    pub(crate) fn enforce_format(item: &T::Midds) -> DispatchResult {
        item.validate_format()
            .map_err(|_| Error::<T, I>::InvalidFormat.into())
    }

    /// Verify an owner's off-chain signature against `payload_bytes`. The
    /// signer is recovered through `T::Signer: IdentifyAccount` and must
    /// match `expected_owner`.
    pub(crate) fn verify_owner_signature(
        payload_bytes: &[u8],
        signature: &T::OffchainSignature,
        expected_owner: &T::AccountId,
    ) -> DispatchResult {
        ensure!(
            signature.verify(payload_bytes, expected_owner),
            Error::<T, I>::InvalidSignature
        );
        Ok(())
    }

    /// Reject if the new payload's canonical identifier does not still index
    /// to `id`. Covers both single-claim records and one-of-several claims
    /// (multi-claim) on the same identifier.
    pub(crate) fn ensure_identifier_unchanged(id: MiddsId, item: &T::Midds) -> DispatchResult {
        ensure!(
            IdentifierClaims::<T, I>::contains_key(item.identifier(), id),
            Error::<T, I>::IdentifierImmutable
        );
        Ok(())
    }

    /// Reject if the deposit's commitment window has elapsed. The window is
    /// **strictly less than** `CommitmentWindow` blocks: at the expiry block
    /// (`deposited_at + CommitmentWindow`) the record is already
    /// finalizable, and `update` / `remove_own` must yield to `finalize`
    /// regardless of intra-block extrinsic order.
    pub(crate) fn ensure_in_window(info: &DepositOf<T, I>) -> DispatchResult {
        let now = <frame_system::Pallet<T>>::block_number();
        let elapsed = now.saturating_sub(info.deposited_at);
        ensure!(
            elapsed < T::CommitmentWindow::get(),
            Error::<T, I>::CommitmentWindowClosed
        );
        Ok(())
    }

    fn hash_payload(item: &T::Midds) -> T::Hash {
        <T::Hashing as Hash>::hash_of(item)
    }

    fn compute_base_bond(size: u32) -> BalanceOf<T, I> {
        let per_byte = T::DepositPerByte::get();
        let size_balance: BalanceOf<T, I> = size.into();
        T::DepositBase::get().saturating_add(per_byte.saturating_mul(size_balance))
    }

    /// Shared deposit path used by both `deposit` (self-deposit, where
    /// `depositor == bond_payer`) and `deposit_on_behalf` (sponsored, where
    /// `bond_payer != depositor`). All bond holding happens against
    /// `bond_payer`; attribution and authority key off `depositor`.
    pub(crate) fn do_deposit(
        depositor: T::AccountId,
        bond_payer: T::AccountId,
        item: T::Midds,
    ) -> DispatchResult {
        Self::enforce_format(&item)?;

        // Exact-payload uniqueness — a depositor cannot register a byte-
        // identical copy of an existing record (multi-claim only allows
        // *different* versions sharing an identifier).
        let payload_hash = Self::hash_payload(&item);
        ensure!(
            !PayloadHashes::<T, I>::contains_key(payload_hash),
            Error::<T, I>::DuplicatePayload
        );

        // Allocate the id (and check the counter doesn't overflow) before
        // holding any bond — otherwise an overflow would leak a held bond
        // for a record that never made it to storage.
        let id = NextMiddsId::<T, I>::get();
        let next = id.checked_add(1).ok_or(Error::<T, I>::CounterOverflow)?;

        let size = item.encoded_size() as u32;
        let base_bond = Self::compute_base_bond(size);
        let total_bond = Self::apply_multipliers(base_bond);

        <T::Currency as MutateHold<T::AccountId>>::hold(
            &Self::deposit_reason(),
            &bond_payer,
            total_bond,
        )
        .map_err(|_| Error::<T, I>::BondHoldFailed)?;

        let now = <frame_system::Pallet<T>>::block_number();
        let expiry = now.saturating_add(T::CommitmentWindow::get());

        Items::<T, I>::insert(id, &item);
        IdentifierClaims::<T, I>::insert(item.identifier(), id, ());
        PayloadHashes::<T, I>::insert(payload_hash, id);
        DepositInfo::<T, I>::insert(
            id,
            Deposit {
                depositor: depositor.clone(),
                bond_payer: bond_payer.clone(),
                deposited_at: now,
                amount: total_bond,
                base_bond,
                payload_hash,
                finalized: false,
            },
        );
        PendingFinalization::<T, I>::insert(expiry, id, ());
        NextMiddsId::<T, I>::put(next);

        // Update demand trackers AFTER state writes so a failure above
        // doesn't poison the multiplier inputs.
        Self::record_deposit_demand();

        Self::deposit_event(Event::Deposited {
            id,
            depositor,
            bond_payer,
            bond: total_bond,
            base_bond,
        });
        Ok(())
    }

    /// Common write path shared by `update` / `update_on_behalf` /
    /// `force_edit`: recompute the unmultiplied base bond, adjust the hold
    /// by the base delta against the original `bond_payer`, and rewrite the
    /// storage entries. The deposit-time multiplier premium is preserved
    /// across edits.
    pub(crate) fn apply_edit(
        id: MiddsId,
        item: T::Midds,
        mut info: DepositOf<T, I>,
    ) -> Result<BalanceOf<T, I>, sp_runtime::DispatchError> {
        let new_size = item.encoded_size() as u32;
        let new_base = Self::compute_base_bond(new_size);
        let premium = info.amount.saturating_sub(info.base_bond);
        let new_amount = new_base.saturating_add(premium);

        // Update the payload hash index — `update` is allowed to land on a
        // different exact payload (within the commitment window) so we
        // re-key. The new hash must not already point elsewhere.
        let new_hash = Self::hash_payload(&item);
        if new_hash != info.payload_hash {
            if let Some(existing) = PayloadHashes::<T, I>::get(new_hash) {
                if existing != id {
                    return Err(Error::<T, I>::DuplicatePayload.into());
                }
            }
            PayloadHashes::<T, I>::remove(info.payload_hash);
            PayloadHashes::<T, I>::insert(new_hash, id);
            info.payload_hash = new_hash;
        }

        Self::adjust_hold(&info.bond_payer, info.amount, new_amount)?;

        Items::<T, I>::insert(id, item);
        info.amount = new_amount;
        info.base_bond = new_base;
        DepositInfo::<T, I>::insert(id, info);
        Ok(new_amount)
    }

    /// Release the full bond held against `info.bond_payer`, then move
    /// `premium_to_treasury` of their now-free balance to the Treasury.
    ///
    /// Shared by `remove_own`, `do_finalize`, `do_force_remove_refund`, and
    /// the live-bond branch of `do_force_remove_slash` — each picks a
    /// different `premium_to_treasury` (multiplier surplus, full bond,
    /// zero, full bond respectively) and otherwise reduces to this
    /// release-then-transfer dance. `transfer_on_hold` would do the two
    /// steps atomically but isn't in the bound trait set, hence the
    /// transient release into free balance.
    pub(crate) fn settle_bond(
        info: &DepositOf<T, I>,
        premium_to_treasury: BalanceOf<T, I>,
    ) -> DispatchResult {
        <T::Currency as MutateHold<T::AccountId>>::release(
            &Self::deposit_reason(),
            &info.bond_payer,
            info.amount,
            Precision::Exact,
        )
        .map_err(|_| Error::<T, I>::BondReleaseFailed)?;
        if !premium_to_treasury.is_zero() {
            <T::Currency as Mutate<T::AccountId>>::transfer(
                &info.bond_payer,
                &T::TreasuryAccount::get(),
                premium_to_treasury,
                Preservation::Expendable,
            )
            .map_err(|_| Error::<T, I>::BondTransferFailed)?;
        }
        Ok(())
    }

    fn adjust_hold(
        bond_payer: &T::AccountId,
        old: BalanceOf<T, I>,
        new: BalanceOf<T, I>,
    ) -> DispatchResult {
        if new > old {
            let delta = new.saturating_sub(old);
            <T::Currency as MutateHold<T::AccountId>>::hold(
                &Self::deposit_reason(),
                bond_payer,
                delta,
            )
            .map_err(|_| Error::<T, I>::BondHoldFailed)?;
        } else if new < old {
            let delta = old.saturating_sub(new);
            <T::Currency as MutateHold<T::AccountId>>::release(
                &Self::deposit_reason(),
                bond_payer,
                delta,
                Precision::Exact,
            )
            .map_err(|_| Error::<T, I>::BondReleaseFailed)?;
        }
        Ok(())
    }

    /// Convert a record's bond into Treasury revenue and mark the record
    /// permanent. Idempotent — a no-op if `finalized` is already true.
    ///
    /// Used by both `on_initialize` (eager) and `finalize` (fallback).
    pub(crate) fn do_finalize(id: MiddsId) -> DispatchResult {
        let Some(mut info) = DepositInfo::<T, I>::get(id) else {
            return Err(Error::<T, I>::MiddsNotFound.into());
        };
        if info.finalized {
            return Ok(());
        }

        Self::settle_bond(&info, info.amount)?;

        let expiry = info.deposited_at.saturating_add(T::CommitmentWindow::get());
        PendingFinalization::<T, I>::remove(expiry, id);

        let amount_to_treasury = info.amount;
        info.finalized = true;
        DepositInfo::<T, I>::insert(id, &info);

        Self::deposit_event(Event::Finalized {
            id,
            depositor: info.depositor,
            bond_payer: info.bond_payer,
            amount_to_treasury,
        });
        Ok(())
    }

    pub(crate) fn do_force_remove_refund(id: MiddsId) -> DispatchResult {
        let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
        ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);

        Self::settle_bond(&info, Zero::zero())?;

        let refund = info.amount;
        let depositor = info.depositor.clone();
        let bond_payer = info.bond_payer.clone();
        Self::cleanup_storage(id, &info)?;
        Self::deposit_event(Event::ForceRemovedRefund {
            id,
            depositor,
            bond_payer,
            refund,
        });
        Ok(())
    }

    pub(crate) fn do_force_remove_slash(id: MiddsId) -> DispatchResult {
        let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;

        let amount_to_treasury = if info.finalized {
            // Bond already moved at finalize time — nothing to send now.
            Zero::zero()
        } else {
            Self::settle_bond(&info, info.amount)?;
            info.amount
        };

        let depositor = info.depositor.clone();
        let bond_payer = info.bond_payer.clone();
        Self::cleanup_storage(id, &info)?;
        Self::deposit_event(Event::ForceRemovedSlash {
            id,
            depositor,
            bond_payer,
            amount_to_treasury,
        });
        Ok(())
    }

    /// Wipe every storage entry tied to `id`. Called by `remove_own` and the
    /// two `force_remove_*` paths — never by `finalize`, which keeps the
    /// record on-chain (just marks it permanent).
    pub(crate) fn cleanup_storage(id: MiddsId, info: &DepositOf<T, I>) -> DispatchResult {
        let item = Items::<T, I>::take(id).ok_or(Error::<T, I>::MiddsNotFound)?;
        IdentifierClaims::<T, I>::remove(item.identifier(), id);
        PayloadHashes::<T, I>::remove(info.payload_hash);
        DepositInfo::<T, I>::remove(id);
        let expiry = info.deposited_at.saturating_add(T::CommitmentWindow::get());
        PendingFinalization::<T, I>::remove(expiry, id);
        Ok(())
    }

    // ---- Public read helpers (consumed via `midds-runtime-api`) ----

    /// Current bond price for a payload of `size` bytes — `base × M_fast ×
    /// M_slow`. Read at the current block.
    pub fn current_deposit_price(size: u32) -> BalanceOf<T, I> {
        Self::apply_multipliers(Self::compute_base_bond(size))
    }

    /// All `MiddsId`s registered against the canonical identifier
    /// (multi-claim).
    pub fn lookup_by_identifier(identifier: <T::Midds as Midds>::Identifier) -> Vec<MiddsId> {
        IdentifierClaims::<T, I>::iter_prefix(identifier)
            .map(|(id, _)| id)
            .collect()
    }
}
