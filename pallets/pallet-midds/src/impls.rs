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
use frame_system::pallet_prelude::BlockNumberFor;
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

    /// Genesis hash of the chain — used as the chain-id portion of the
    /// on-behalf domain separator. Different chains / forks produce
    /// different genesis hashes, so a signature captured on one chain is
    /// not replayable on another.
    pub(crate) fn genesis_hash() -> T::Hash {
        <frame_system::Pallet<T>>::block_hash(BlockNumberFor::<T>::zero())
    }

    /// `Midds::KIND` of this instance, encoded as bytes for inclusion in
    /// the on-behalf payload. Pins the signature to a specific MIDDS type
    /// so a signature for `MusicalWork` cannot be replayed against a
    /// `Recording` instance whose `Item: M` happens to share the same
    /// SCALE shape (e.g. a `Remove` payload that carries no `item`).
    pub(crate) fn kind_bytes() -> Vec<u8> {
        T::Midds::KIND.as_bytes().to_vec()
    }

    /// Reject if the signature's `valid_until` window has already passed.
    /// The boundary is inclusive — the signer can pin `valid_until = now`
    /// to authorize a single-block submission.
    pub(crate) fn ensure_signature_fresh(valid_until: BlockNumberFor<T>) -> DispatchResult {
        let now = <frame_system::Pallet<T>>::block_number();
        ensure!(now <= valid_until, Error::<T, I>::SignatureExpired);
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

    /// `owner_layer.amount` or zero if no owner layer exists.
    pub(crate) fn owner_amount(info: &DepositOf<T, I>) -> BalanceOf<T, I> {
        info.owner_layer
            .as_ref()
            .map(|l| l.amount)
            .unwrap_or_else(Zero::zero)
    }

    /// `owner_layer.base` or zero if no owner layer exists.
    pub(crate) fn owner_base(info: &DepositOf<T, I>) -> BalanceOf<T, I> {
        info.owner_layer
            .as_ref()
            .map(|l| l.base)
            .unwrap_or_else(Zero::zero)
    }

    /// Total base across both layers — the canonical "current bond formula
    /// value" for the stored payload size.
    pub(crate) fn total_base(info: &DepositOf<T, I>) -> BalanceOf<T, I> {
        info.sponsor_layer
            .base
            .saturating_add(Self::owner_base(info))
    }

    /// Total amount currently held across both layers — what `do_finalize`
    /// transfers to the Treasury.
    pub(crate) fn total_amount(info: &DepositOf<T, I>) -> BalanceOf<T, I> {
        info.sponsor_layer
            .amount
            .saturating_add(Self::owner_amount(info))
    }

    /// Aggregated multiplier premium (= total amount − total base) across
    /// every layer that survived to remove time. Sent to the Treasury on
    /// `remove_own`; preserved in `total_amount` for finalize / slash paths.
    pub(crate) fn total_premium(info: &DepositOf<T, I>) -> BalanceOf<T, I> {
        Self::total_amount(info).saturating_sub(Self::total_base(info))
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
                deposited_at: now,
                sponsor_layer: BondLayer {
                    payer: bond_payer.clone(),
                    amount: total_bond,
                    base: base_bond,
                },
                owner_layer: None,
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
    /// `force_edit`. Routes the size delta across `sponsor_layer` and
    /// `owner_layer` and adjusts each layer's hold to match.
    ///
    /// Per-layer accounting:
    /// - `layer.base` evolves by an LIFO split of `Δbase_total` — the
    ///   caller's own layer absorbs the change first; a shrink that
    ///   overflows it spills into the other layer.
    /// - `layer.amount` is recomputed as `new_base + max(0, old_amount −
    ///   old_base)`, which preserves the deposit-time multiplier premium
    ///   per layer (cf. `docs/economics.md` §5.5). On a layer where the
    ///   deposit-time `M < 1` (so amount started below base, premium = 0),
    ///   the formula degenerates to `new_amount = new_base`, mirroring the
    ///   pre-stratification behaviour where an underpriced deposit "catches
    ///   up" on edit.
    /// - The owner layer materialises only on the first solo `update` by
    ///   the depositor on a sponsored record; at that moment the new
    ///   `Δbase` is multiplied by `M_current`, banking a fresh premium
    ///   against the depositor (matching how `sponsor_layer` is banked at
    ///   deposit time).
    ///
    /// `caller` decides which layer is "primary" for the LIFO split:
    /// - `update`: caller = depositor. On self-deposits this is the
    ///   sponsor layer's payer (no owner layer involved). On sponsored
    ///   records this is the owner side — the **web3 escape hatch**.
    /// - `update_on_behalf`: caller = original sponsor (= sponsor layer's
    ///   payer).
    /// - `force_edit`: caller = sponsor layer's payer; governance edits
    ///   never conjure an owner layer.
    pub(crate) fn apply_edit(
        id: MiddsId,
        item: T::Midds,
        mut info: DepositOf<T, I>,
        caller: &T::AccountId,
    ) -> Result<DepositOf<T, I>, DispatchError> {
        let new_size = item.encoded_size() as u32;
        let new_total_base = Self::compute_base_bond(new_size);
        let old_total_base = Self::total_base(&info);

        // Re-key the payload-hash index. Identifier immutability is enforced
        // upstream by `ensure_identifier_unchanged`; here we only guard
        // against landing on another record's exact payload. Mutated *after*
        // the bond mutations succeed so a hold/release failure leaves the
        // index untouched.
        let new_hash = Self::hash_payload(&item);
        if new_hash != info.payload_hash {
            if let Some(existing) = PayloadHashes::<T, I>::get(new_hash) {
                if existing != id {
                    return Err(Error::<T, I>::DuplicatePayload.into());
                }
            }
        }

        Self::route_size_delta(&mut info, caller, new_total_base, old_total_base)?;

        if new_hash != info.payload_hash {
            PayloadHashes::<T, I>::remove(info.payload_hash);
            PayloadHashes::<T, I>::insert(new_hash, id);
            info.payload_hash = new_hash;
        }

        Items::<T, I>::insert(id, item);
        DepositInfo::<T, I>::insert(id, &info);
        Ok(info)
    }

    /// Variant of [`Self::apply_edit`] used by `force_edit` on records
    /// that have already finalized: the bond has been transferred to
    /// the Treasury (`finalized = true`) so there is no remaining hold
    /// to rebalance. We only need to update the stored payload and the
    /// reverse `PayloadHashes` index — duplicate detection still applies
    /// since exact-payload uniqueness is a global invariant of the
    /// pallet, finalized or not.
    pub(crate) fn apply_finalized_edit(
        id: MiddsId,
        item: T::Midds,
        mut info: DepositOf<T, I>,
    ) -> Result<DepositOf<T, I>, DispatchError> {
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
        Items::<T, I>::insert(id, item);
        DepositInfo::<T, I>::insert(id, &info);
        Ok(info)
    }

    /// Apply the per-layer base + amount adjustments for an `apply_edit`
    /// call and synchronise the on-chain holds accordingly.
    fn route_size_delta(
        info: &mut DepositOf<T, I>,
        caller: &T::AccountId,
        new_total_base: BalanceOf<T, I>,
        old_total_base: BalanceOf<T, I>,
    ) -> DispatchResult {
        let caller_is_sponsor = caller == &info.sponsor_layer.payer;

        if new_total_base >= old_total_base {
            let delta = new_total_base.saturating_sub(old_total_base);
            if delta.is_zero() {
                return Ok(());
            }
            if caller_is_sponsor {
                // Sponsor extends own layer; premium preserved per layer.
                Self::extend_layer_in_place(
                    &info.sponsor_layer.payer.clone(),
                    &mut info.sponsor_layer,
                    delta,
                )?;
                return Ok(());
            }
            // Owner-driven grow.
            match info.owner_layer.as_mut() {
                Some(layer) => {
                    let payer = layer.payer.clone();
                    Self::extend_layer_in_place(&payer, layer, delta)?;
                }
                None => {
                    // First contribution from the depositor: bank the
                    // current multiplier premium against them.
                    let amount = Self::apply_multipliers(delta);
                    if !amount.is_zero() {
                        <T::Currency as MutateHold<T::AccountId>>::hold(
                            &Self::deposit_reason(),
                            caller,
                            amount,
                        )
                        .map_err(|_| Error::<T, I>::BondHoldFailed)?;
                    }
                    info.owner_layer = Some(BondLayer {
                        payer: caller.clone(),
                        amount,
                        base: delta,
                    });
                }
            }
            return Ok(());
        }

        // Shrink: split base reduction LIFO across layers, then recompute
        // each touched layer's amount via the per-layer premium rule.
        let delta_base = old_total_base.saturating_sub(new_total_base);
        let owner_base = Self::owner_base(info);
        let (sponsor_release_base, owner_release_base) = if caller_is_sponsor {
            // Caller = sponsor: drain sponsor first, overflow into owner
            // (only possible when an owner layer exists).
            let s = info.sponsor_layer.base.min(delta_base);
            let overflow = delta_base.saturating_sub(s);
            (s, overflow.min(owner_base))
        } else {
            // Caller = owner: drain owner first, overflow into sponsor.
            let o = owner_base.min(delta_base);
            let overflow = delta_base.saturating_sub(o);
            (overflow.min(info.sponsor_layer.base), o)
        };

        if !sponsor_release_base.is_zero() {
            let payer = info.sponsor_layer.payer.clone();
            Self::shrink_layer_in_place(&payer, &mut info.sponsor_layer, sponsor_release_base)?;
        }
        if !owner_release_base.is_zero() {
            if let Some(layer) = info.owner_layer.as_mut() {
                let payer = layer.payer.clone();
                Self::shrink_layer_in_place(&payer, layer, owner_release_base)?;
            }
        }
        Ok(())
    }

    /// Grow `layer.base` by `delta_base` and update `layer.amount` via the
    /// per-layer premium rule, holding the resulting amount delta against
    /// `payer`. The per-layer premium is preserved across the edit, so for
    /// a `M ≥ 1` layer the amount delta equals `delta_base`; for a `M < 1`
    /// underpriced layer (where `amount < base`) the formula rebases the
    /// hold up to the new base — matching the pre-stratification semantics
    /// (cf. the original `apply_edit`).
    fn extend_layer_in_place(
        payer: &T::AccountId,
        layer: &mut BondLayer<T::AccountId, BalanceOf<T, I>>,
        delta_base: BalanceOf<T, I>,
    ) -> DispatchResult {
        let new_base = layer.base.saturating_add(delta_base);
        let preserved_premium = layer.amount.saturating_sub(layer.base);
        let new_amount = new_base.saturating_add(preserved_premium);
        let amount_delta = new_amount.saturating_sub(layer.amount);
        let amount_release = layer.amount.saturating_sub(new_amount);
        if !amount_delta.is_zero() {
            <T::Currency as MutateHold<T::AccountId>>::hold(
                &Self::deposit_reason(),
                payer,
                amount_delta,
            )
            .map_err(|_| Error::<T, I>::BondHoldFailed)?;
        } else if !amount_release.is_zero() {
            // Possible only if `delta_base = 0` somehow — defensive.
            <T::Currency as MutateHold<T::AccountId>>::release(
                &Self::deposit_reason(),
                payer,
                amount_release,
                Precision::Exact,
            )
            .map_err(|_| Error::<T, I>::BondReleaseFailed)?;
        }
        layer.base = new_base;
        layer.amount = new_amount;
        Ok(())
    }

    /// Shrink `layer.base` by `release_base` and update `layer.amount` via
    /// the per-layer premium rule, releasing the corresponding amount
    /// delta from `payer`'s hold (or, in the underpriced `M < 1` case,
    /// holding the difference if the new base exceeds the current amount).
    fn shrink_layer_in_place(
        payer: &T::AccountId,
        layer: &mut BondLayer<T::AccountId, BalanceOf<T, I>>,
        release_base: BalanceOf<T, I>,
    ) -> DispatchResult {
        let new_base = layer.base.saturating_sub(release_base);
        let preserved_premium = layer.amount.saturating_sub(layer.base);
        let new_amount = new_base.saturating_add(preserved_premium);
        if new_amount > layer.amount {
            let extra = new_amount.saturating_sub(layer.amount);
            <T::Currency as MutateHold<T::AccountId>>::hold(&Self::deposit_reason(), payer, extra)
                .map_err(|_| Error::<T, I>::BondHoldFailed)?;
        } else if new_amount < layer.amount {
            let release = layer.amount.saturating_sub(new_amount);
            <T::Currency as MutateHold<T::AccountId>>::release(
                &Self::deposit_reason(),
                payer,
                release,
                Precision::Exact,
            )
            .map_err(|_| Error::<T, I>::BondReleaseFailed)?;
        }
        layer.base = new_base;
        layer.amount = new_amount;
        Ok(())
    }

    /// Release every layer's hold and route the per-layer balance per
    /// `kind`:
    ///
    /// - `Refund` — release everything back to each layer's payer (no
    ///   Treasury flow). Used by `force_remove_refund`.
    /// - `PremiumOnly` — refund each layer's `base` to its payer, transfer
    ///   each layer's premium to the Treasury. Used by `remove_own`.
    /// - `Full` — release each layer's amount and transfer it in full to
    ///   the Treasury. Used by `do_finalize` and the live-bond branch of
    ///   `do_force_remove_slash`.
    ///
    /// Each layer settles **independently**: a sponsor and a solo-extending
    /// owner each lose exactly the premium they themselves banked, never a
    /// share of the other's. This is what makes the web3 escape hatch
    /// well-defined: the owner's contribution stays insulated from the
    /// sponsor's bookkeeping.
    pub(crate) fn settle_bond(info: &DepositOf<T, I>, kind: SettlementKind) -> DispatchResult {
        Self::settle_layer(
            &info.sponsor_layer.payer,
            info.sponsor_layer.amount,
            info.sponsor_layer
                .amount
                .saturating_sub(info.sponsor_layer.base),
            kind,
        )?;
        if let Some(layer) = info.owner_layer.as_ref() {
            Self::settle_layer(
                &layer.payer,
                layer.amount,
                layer.amount.saturating_sub(layer.base),
                kind,
            )?;
        }
        Ok(())
    }

    fn settle_layer(
        payer: &T::AccountId,
        amount: BalanceOf<T, I>,
        premium: BalanceOf<T, I>,
        kind: SettlementKind,
    ) -> DispatchResult {
        if amount.is_zero() {
            return Ok(());
        }
        <T::Currency as MutateHold<T::AccountId>>::release(
            &Self::deposit_reason(),
            payer,
            amount,
            Precision::Exact,
        )
        .map_err(|_| Error::<T, I>::BondReleaseFailed)?;

        let to_treasury = match kind {
            SettlementKind::Refund => return Ok(()),
            SettlementKind::PremiumOnly => premium,
            SettlementKind::Full => amount,
        };
        if to_treasury.is_zero() {
            return Ok(());
        }
        <T::Currency as Mutate<T::AccountId>>::transfer(
            payer,
            &T::TreasuryAccount::get(),
            to_treasury,
            Preservation::Expendable,
        )
        .map_err(|_| Error::<T, I>::BondTransferFailed)?;
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

        let amount_to_treasury = Self::total_amount(&info);
        Self::settle_bond(&info, SettlementKind::Full)?;

        let expiry = info.deposited_at.saturating_add(T::CommitmentWindow::get());
        PendingFinalization::<T, I>::remove(expiry, id);

        info.finalized = true;
        let depositor = info.depositor.clone();
        let sponsor = info.sponsor_layer.payer.clone();
        DepositInfo::<T, I>::insert(id, &info);

        Self::deposit_event(Event::Finalized {
            id,
            depositor,
            sponsor,
            amount_to_treasury,
        });
        Ok(())
    }

    /// Surface the post-edit per-layer holds so off-chain consumers can
    /// reflect the stratified bond without re-querying `DepositInfo`. Shared
    /// by `update` and `update_on_behalf`; `force_edit` emits a different
    /// event and intentionally skips this.
    pub(crate) fn emit_updated_event(id: MiddsId, info: &DepositOf<T, I>) {
        Self::deposit_event(Event::Updated {
            id,
            sponsor_bond: info.sponsor_layer.amount,
            owner_bond: Self::owner_amount(info),
        });
    }

    /// Settle a within-window cancellation (shared by `remove_own` and
    /// `remove_own_on_behalf`): refund each layer's base to its own payer,
    /// transfer aggregated premium to the Treasury, wipe storage, emit
    /// `Refunded`. Callers are responsible for the auth and pre-state
    /// checks (depositor / signature / window / finalized).
    pub(crate) fn do_remove_own(id: MiddsId, info: DepositOf<T, I>) -> DispatchResult {
        let sponsor_refund = info.sponsor_layer.base;
        let owner_refund = Self::owner_base(&info);
        let premium_to_treasury = Self::total_premium(&info);

        Self::settle_bond(&info, SettlementKind::PremiumOnly)?;
        Self::cleanup_storage(id, &info)?;

        Self::deposit_event(Event::Refunded {
            id,
            depositor: info.depositor,
            sponsor: info.sponsor_layer.payer,
            sponsor_refund,
            owner_refund,
            premium_to_treasury,
        });
        Ok(())
    }

    pub(crate) fn do_force_remove_refund(id: MiddsId) -> DispatchResult {
        let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
        ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);

        Self::settle_bond(&info, SettlementKind::Refund)?;

        let sponsor_refund = info.sponsor_layer.amount;
        let owner_refund = info
            .owner_layer
            .as_ref()
            .map(|l| l.amount)
            .unwrap_or_else(Zero::zero);
        let depositor = info.depositor.clone();
        let sponsor = info.sponsor_layer.payer.clone();
        Self::cleanup_storage(id, &info)?;
        Self::deposit_event(Event::ForceRemovedRefund {
            id,
            depositor,
            sponsor,
            sponsor_refund,
            owner_refund,
        });
        Ok(())
    }

    pub(crate) fn do_force_remove_slash(id: MiddsId) -> DispatchResult {
        let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;

        let amount_to_treasury = if info.finalized {
            // Bond already moved at finalize time — nothing to send now.
            Zero::zero()
        } else {
            let total = Self::total_amount(&info);
            Self::settle_bond(&info, SettlementKind::Full)?;
            total
        };

        let depositor = info.depositor.clone();
        let sponsor = info.sponsor_layer.payer.clone();
        Self::cleanup_storage(id, &info)?;
        Self::deposit_event(Event::ForceRemovedSlash {
            id,
            depositor,
            sponsor,
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

    /// Hard cap applied by every reverse-lookup helper exposed to the
    /// runtime API / RPC. Multi-claim is unbounded by design (`docs/
    /// economics.md` §1) so a popular identifier could otherwise produce
    /// arbitrarily large `Vec<MiddsId>` payloads and stall the RPC node.
    /// Consumers who need more should paginate via
    /// [`Self::lookup_by_identifier_paged`].
    pub const MAX_LOOKUP_LIMIT: u32 = 256;

    /// First page of `MiddsId`s registered against the canonical
    /// identifier, capped at [`Self::MAX_LOOKUP_LIMIT`]. The result is
    /// sorted by `MiddsId` ascending so the cap is deterministic and
    /// pagination via [`Self::lookup_by_identifier_paged`] resumes
    /// cleanly from the last id returned here.
    pub fn lookup_by_identifier(identifier: <T::Midds as Midds>::Identifier) -> Vec<MiddsId> {
        Self::lookup_by_identifier_paged(identifier, None, Self::MAX_LOOKUP_LIMIT)
    }

    /// Paginated variant: return `MiddsId`s strictly greater than `after`
    /// (or all of them when `after` is `None`), sorted ascending, capped
    /// at `min(limit, MAX_LOOKUP_LIMIT)`. The natural cursor for the next
    /// page is the last id of the returned vector.
    pub fn lookup_by_identifier_paged(
        identifier: <T::Midds as Midds>::Identifier,
        after: Option<MiddsId>,
        limit: u32,
    ) -> Vec<MiddsId> {
        let cap = core::cmp::min(limit, Self::MAX_LOOKUP_LIMIT) as usize;
        if cap == 0 {
            return Vec::new();
        }
        let mut ids: Vec<MiddsId> = IdentifierClaims::<T, I>::iter_prefix(identifier)
            .map(|(id, _)| id)
            .filter(|id| match after {
                Some(after_id) => *id > after_id,
                None => true,
            })
            .collect();
        ids.sort_unstable();
        ids.truncate(cap);
        ids
    }

    /// Total number of `MiddsId`s registered against the canonical
    /// identifier — useful for UIs that need to render "X claims" next
    /// to a paginated list. Iterates the prefix once, so the cost scales
    /// linearly with the number of claims; callers that only need a
    /// "more than N?" indicator should compare against
    /// [`Self::MAX_LOOKUP_LIMIT`] from the lookup helper instead.
    pub fn count_by_identifier(identifier: <T::Midds as Midds>::Identifier) -> u32 {
        IdentifierClaims::<T, I>::iter_prefix(identifier)
            .take(u32::MAX as usize)
            .count() as u32
    }
}
