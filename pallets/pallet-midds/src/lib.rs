//! # `pallet-midds`
//!
//! Generic, multi-instance FRAME pallet managing the lifecycle of a single
//! type of MIDDS record (MusicalWork, Recording, Release, …) per instance.
//!
//! Per `docs/economics.md`: permissionless deposit secured by a size-proportional
//! bond multiplied by two dynamic multipliers (anti-DoS `M_fast`, anti-flood
//! `M_slow`), refundable for 7 days, then converted to Treasury revenue at
//! finalization. `IdentifierClaims` allows multi-claim on the same canonical
//! identifier (e.g. several parties registering their own version of an
//! ISWC); only exact-payload duplicates are rejected via `PayloadHashes`.
//!
//! Sudo split (`force_remove_refund` / `force_remove_slash`) lets governance
//! distinguish good-faith typos from intentional abuse.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;
pub use types::*;
pub use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
pub use benchmarking::BenchmarkHelper;

mod types;
pub mod weights;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;

use frame_support::traits::fungible::Inspect;

/// Balance type carried by the configured fungible currency.
pub type BalanceOf<T, I = ()> =
    <<T as Config<I>>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

/// Bond entry stored alongside each MIDDS record.
pub type DepositOf<T, I = ()> = Deposit<
    <T as frame_system::Config>::AccountId,
    BalanceOf<T, I>,
    frame_system::pallet_prelude::BlockNumberFor<T>,
    <T as frame_system::Config>::Hash,
>;

#[frame_support::pallet]
pub mod pallet {
    use super::{BalanceOf, DepositOf, WeightInfo};
    use crate::types::Deposit;
    use alloc::{vec, vec::Vec};
    use frame_support::{
        BoundedVec,
        pallet_prelude::*,
        traits::{
            EnsureOrigin,
            fungible::{Mutate, MutateHold},
            tokens::{Precision, Preservation},
        },
    };
    use frame_system::pallet_prelude::*;
    use midds_traits::{Midds, MiddsId};
    use parity_scale_codec::Encode;
    use sp_runtime::{
        FixedPointNumber, FixedU128,
        traits::{Hash, Saturating, Zero},
    };

    /// Number of daily buckets composing the slow rolling window. The window
    /// is **7 days exactly** by spec — change this and the `M_slow` semantics
    /// change.
    pub const SLOW_WINDOW_DAYS: u32 = 7;

    #[pallet::pallet]
    pub struct Pallet<T, I = ()>(_);

    #[pallet::config]
    pub trait Config<I: 'static = ()>:
        frame_system::Config<RuntimeEvent: From<Event<Self, I>>>
    {
        /// Fungible currency used to hold deposits as a bond and to transfer
        /// premiums / finalized bonds to the Treasury.
        type Currency: Mutate<Self::AccountId>
            + MutateHold<Self::AccountId, Reason = Self::RuntimeHoldReason>;

        /// Aggregated runtime hold-reason. Must include `HoldReason<I>`.
        type RuntimeHoldReason: From<HoldReason<I>>;

        /// MIDDS payload type stored by this instance.
        type Midds: Midds;

        /// Origin allowed to call `deposit` / `update` / `remove_own`.
        type ProviderOrigin: EnsureOrigin<Self::RuntimeOrigin, Success = Self::AccountId>;

        /// Origin allowed to call the `force_*` extrinsics (sudo at launch).
        type ForceOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// Foundation Treasury account that receives finalized bonds and
        /// non-refundable multiplier premiums. Per `docs/economics.md` §9 we
        /// never burn — the AFT is recycled via Treasury governance.
        type TreasuryAccount: Get<Self::AccountId>;

        /// Flat part of the (unmultiplied) bond formula.
        #[pallet::constant]
        type DepositBase: Get<BalanceOf<Self, I>>;

        /// Per-byte multiplier applied to the SCALE-encoded payload size.
        #[pallet::constant]
        type DepositPerByte: Get<BalanceOf<Self, I>>;

        /// Length of the refundable commitment window. After this many blocks
        /// the depositor can no longer call `remove_own` and the bond gets
        /// converted to Treasury revenue.
        #[pallet::constant]
        type CommitmentWindow: Get<BlockNumberFor<Self>>;

        /// Hard cap on the number of pending finalizations executed by
        /// `on_initialize` per block. The leftover entries are picked up
        /// either by the next blocks' `on_initialize` or by a permissionless
        /// `finalize(id)` call (cf. `docs/economics.md` §4.2).
        #[pallet::constant]
        type MaxFinalizationsPerBlock: Get<u32>;

        /// Number of blocks composing one logical day. Drives the slow-window
        /// bucket rotation. With 6s slots: `BlocksPerDay = 14_400`.
        #[pallet::constant]
        type BlocksPerDay: Get<BlockNumberFor<Self>>;

        /// Target number of deposits per block before `M_fast` adjusts upward.
        #[pallet::constant]
        type FastTargetPerBlock: Get<u32>;

        /// Maximum per-block multiplicative step applied to `M_fast`.
        #[pallet::constant]
        type FastAdjustmentRate: Get<FixedU128>;

        /// Lower clamp on `M_fast`.
        #[pallet::constant]
        type FastMultiplierMin: Get<FixedU128>;

        /// Upper clamp on `M_fast`.
        #[pallet::constant]
        type FastMultiplierMax: Get<FixedU128>;

        /// Target number of deposits per rolling 7-day window before `M_slow`
        /// adjusts upward.
        #[pallet::constant]
        type SlowTargetPerWindow: Get<u32>;

        /// Maximum per-day multiplicative step applied to `M_slow`.
        #[pallet::constant]
        type SlowAdjustmentRate: Get<FixedU128>;

        /// Lower clamp on `M_slow`.
        #[pallet::constant]
        type SlowMultiplierMin: Get<FixedU128>;

        /// Upper clamp on `M_slow`.
        #[pallet::constant]
        type SlowMultiplierMax: Get<FixedU128>;

        /// Weight metadata for this pallet's extrinsics and hook.
        type WeightInfo: WeightInfo;

        /// Helper producing worst-case payloads for benchmarking. Each runtime
        /// instance supplies its own implementation tailored to its `Midds` type.
        #[cfg(feature = "runtime-benchmarks")]
        type BenchmarkHelper: crate::BenchmarkHelper<Self::Midds>;
    }

    /// Reasons the pallet holds funds against an account.
    #[pallet::composite_enum]
    pub enum HoldReason<I: 'static = ()> {
        /// Bond covering a stored MIDDS record.
        #[codec(index = 0)]
        Deposit,
    }

    /// Default value for the multipliers when first read — both start at 1.0×
    /// (neutral) so a fresh chain prices deposits at the unmultiplied bond.
    pub struct DefaultUnitMultiplier;
    impl Get<FixedU128> for DefaultUnitMultiplier {
        fn get() -> FixedU128 {
            FixedU128::one()
        }
    }

    /// Default value for [`SlowWindowBuckets`] — a fully-zero 7-day buffer.
    pub struct DefaultSlowBuckets;
    impl Get<BoundedVec<u32, ConstU32<SLOW_WINDOW_DAYS>>> for DefaultSlowBuckets {
        fn get() -> BoundedVec<u32, ConstU32<SLOW_WINDOW_DAYS>> {
            BoundedVec::try_from(vec![0u32; SLOW_WINDOW_DAYS as usize])
                .expect("SLOW_WINDOW_DAYS u32s ≤ SLOW_WINDOW_DAYS bound; qed")
        }
    }

    /// Monotonic per-instance counter feeding the next [`MiddsId`].
    #[pallet::storage]
    pub type NextMiddsId<T: Config<I>, I: 'static = ()> = StorageValue<_, MiddsId, ValueQuery>;

    /// Stored MIDDS records keyed by their on-chain id.
    #[pallet::storage]
    pub type Items<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, MiddsId, T::Midds>;

    /// Multi-claim reverse index `(canonical identifier, on-chain id) -> ()`.
    /// Several parties may legitimately deposit different versions of the
    /// same identifier (cf. `docs/economics.md` §1) — uniqueness is enforced
    /// per-payload via [`PayloadHashes`], not per-identifier.
    #[pallet::storage]
    pub type IdentifierClaims<T: Config<I>, I: 'static = ()> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        <T::Midds as Midds>::Identifier,
        Twox64Concat,
        MiddsId,
        (),
        OptionQuery,
    >;

    /// Hash of the SCALE-encoded payload → on-chain id. Backs exact-payload
    /// uniqueness: identical payloads are rejected, but different payloads
    /// sharing an identifier are accepted.
    #[pallet::storage]
    pub type PayloadHashes<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Identity, T::Hash, MiddsId>;

    /// Bond information for each stored record.
    #[pallet::storage]
    pub type DepositInfo<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, MiddsId, DepositOf<T, I>>;

    /// Finalization queue keyed by expiry block (= `deposited_at +
    /// CommitmentWindow`). `iter_prefix(n)` enumerates the records due at
    /// block `n` for the eager `on_initialize` path.
    #[pallet::storage]
    pub type PendingFinalization<T: Config<I>, I: 'static = ()> =
        StorageDoubleMap<_, Twox64Concat, BlockNumberFor<T>, Identity, MiddsId, (), OptionQuery>;

    /// Number of `deposit` calls made during the current block. Drained by
    /// `on_initialize` to feed `M_fast` adjustment.
    #[pallet::storage]
    pub type DepositsThisBlock<T: Config<I>, I: 'static = ()> = StorageValue<_, u32, ValueQuery>;

    /// Anti-DoS multiplier (per-block reactivity).
    #[pallet::storage]
    pub type FastMultiplier<T: Config<I>, I: 'static = ()> =
        StorageValue<_, FixedU128, ValueQuery, DefaultUnitMultiplier>;

    /// 7 daily buckets of deposits counts, indexed by `SlowWindowHead`. Sum
    /// over all buckets approximates a rolling 7-day window.
    #[pallet::storage]
    pub type SlowWindowBuckets<T: Config<I>, I: 'static = ()> = StorageValue<
        _,
        BoundedVec<u32, ConstU32<SLOW_WINDOW_DAYS>>,
        ValueQuery,
        DefaultSlowBuckets,
    >;

    /// Index of the "today" bucket inside [`SlowWindowBuckets`]. Advances by
    /// one (mod 7) at every daily rotation.
    #[pallet::storage]
    pub type SlowWindowHead<T: Config<I>, I: 'static = ()> = StorageValue<_, u8, ValueQuery>;

    /// Anti-flood multiplier (per-day reactivity).
    #[pallet::storage]
    pub type SlowMultiplier<T: Config<I>, I: 'static = ()> =
        StorageValue<_, FixedU128, ValueQuery, DefaultUnitMultiplier>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config<I>, I: 'static = ()> {
        /// A new MIDDS record was deposited.
        Deposited {
            id: MiddsId,
            depositor: T::AccountId,
            bond: BalanceOf<T, I>,
            base_bond: BalanceOf<T, I>,
        },
        /// A MIDDS record was updated by its depositor (or by `force_edit`).
        Updated {
            id: MiddsId,
            new_bond: BalanceOf<T, I>,
        },
        /// A MIDDS record was edited by `ForceOrigin`.
        ForceEdited { id: MiddsId },
        /// The depositor refunded their record within the commitment window.
        /// `refund` is the amount returned to the depositor (the unmultiplied
        /// base bond at remove time); `premium_to_treasury` is the multiplier
        /// surplus permanently transferred to the Treasury.
        Refunded {
            id: MiddsId,
            depositor: T::AccountId,
            refund: BalanceOf<T, I>,
            premium_to_treasury: BalanceOf<T, I>,
        },
        /// A MIDDS record's bond was finalized: the full held amount moved
        /// from the depositor's hold to the Treasury and the record became
        /// permanent.
        Finalized {
            id: MiddsId,
            depositor: T::AccountId,
            amount_to_treasury: BalanceOf<T, I>,
        },
        /// A MIDDS record was removed by `ForceOrigin` with the depositor
        /// fully refunded (good-faith typo path).
        ForceRemovedRefund {
            id: MiddsId,
            depositor: T::AccountId,
            refund: BalanceOf<T, I>,
        },
        /// A MIDDS record was removed by `ForceOrigin` without refund. If the
        /// bond was still held at the time, it was sent to the Treasury;
        /// post-finalization the bond was already there.
        ForceRemovedSlash {
            id: MiddsId,
            depositor: T::AccountId,
            amount_to_treasury: BalanceOf<T, I>,
        },
    }

    #[pallet::error]
    pub enum Error<T, I = ()> {
        /// A record with this exact payload (same SCALE encoding) is already
        /// registered for this instance.
        DuplicatePayload,
        /// No record exists at the supplied id.
        MiddsNotFound,
        /// Caller is not the original depositor.
        NotProvider,
        /// The commitment window has elapsed; only `ForceOrigin` (or
        /// `finalize`) can act on the record now.
        CommitmentWindowClosed,
        /// `remove_own` cannot operate on a record whose bond has already
        /// been finalized to the Treasury.
        AlreadyFinalized,
        /// `finalize` was called before the commitment window elapsed.
        CommitmentWindowOpen,
        /// The new payload changes the canonical identifier (immutable).
        IdentifierImmutable,
        /// Format / charset / length validation failed.
        InvalidFormat,
        /// Holding the additional bond failed (insufficient free balance).
        BondHoldFailed,
        /// Releasing the bond failed (corrupt invariant).
        BondReleaseFailed,
        /// Transferring the premium / finalized bond to the Treasury failed.
        BondTransferFailed,
        /// `NextMiddsId` would overflow `u64` — unreachable in practice but
        /// surfaced explicitly so an overflow never silently masquerades as a
        /// different failure.
        CounterOverflow,
    }

    #[pallet::hooks]
    impl<T: Config<I>, I: 'static> Hooks<BlockNumberFor<T>> for Pallet<T, I> {
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            let mut weight = T::DbWeight::get().reads_writes(1, 1);

            // 1) Drain the previous block's deposits counter and adjust M_fast.
            let fast_count = DepositsThisBlock::<T, I>::take();
            Self::adjust_fast_multiplier(fast_count);

            // 2) Daily slow-window rotation (skip genesis to avoid rotating an
            //    all-zero window before any deposit happened).
            if !n.is_zero() && (n % T::BlocksPerDay::get()).is_zero() {
                Self::rotate_slow_bucket();
                Self::adjust_slow_multiplier();
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 2));
            }

            // 3) Eagerly finalize records whose commitment window expires at
            //    this block. Capped to keep the hook bounded; remainders are
            //    picked up by `finalize(id)` (permissionless) or by the next
            //    blocks once the prefix re-fits in budget.
            let cap = T::MaxFinalizationsPerBlock::get() as usize;
            let due: Vec<MiddsId> = PendingFinalization::<T, I>::iter_prefix(n)
                .take(cap)
                .map(|(id, _)| id)
                .collect();
            for id in due {
                // Best-effort: a single failure must not poison the rest of
                // the queue. The slot can be retried via `finalize(id)`.
                let _ = Self::do_finalize(id);
                weight = weight.saturating_add(T::WeightInfo::finalize_one());
            }

            weight
        }
    }

    #[pallet::call]
    impl<T: Config<I>, I: 'static> Pallet<T, I> {
        /// Deposit a new MIDDS record. The caller pays a multiplied,
        /// size-proportional bond which is held against their account for
        /// `CommitmentWindow` blocks. The record is published immediately.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::deposit(item.encoded_size() as u32))]
        pub fn deposit(origin: OriginFor<T>, item: T::Midds) -> DispatchResult {
            let depositor = T::ProviderOrigin::ensure_origin(origin)?;
            Self::do_validate_format(&item)?;

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
            let base_bond = Self::do_compute_base_bond(size);
            let total_bond = Self::apply_multipliers(base_bond);

            <T::Currency as MutateHold<T::AccountId>>::hold(
                &Self::deposit_reason(),
                &depositor,
                total_bond,
            )
            .map_err(|_| Error::<T, I>::BondHoldFailed)?;

            let now = <frame_system::Pallet<T>>::block_number();
            let expiry = now.saturating_add(T::CommitmentWindow::get());
            let identifier = item.identifier();

            Items::<T, I>::insert(id, &item);
            IdentifierClaims::<T, I>::insert(&identifier, id, ());
            PayloadHashes::<T, I>::insert(payload_hash, id);
            DepositInfo::<T, I>::insert(
                id,
                Deposit {
                    depositor: depositor.clone(),
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
                bond: total_bond,
                base_bond,
            });
            Ok(())
        }

        /// Update an existing MIDDS record. Only the original depositor may
        /// call this, and only while still inside the commitment window. The
        /// canonical identifier is immutable; the multiplier premium banked
        /// at deposit time is preserved (cf. `docs/economics.md` §5.5) — only
        /// the unmultiplied base portion is re-priced against the new size.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::update(item.encoded_size() as u32))]
        pub fn update(origin: OriginFor<T>, id: MiddsId, item: T::Midds) -> DispatchResult {
            let caller = T::ProviderOrigin::ensure_origin(origin)?;
            Self::do_validate_format(&item)?;

            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(info.depositor == caller, Error::<T, I>::NotProvider);
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);

            // Identifier must still index back to this id (covers the
            // single-claim case as well as one of several claims).
            ensure!(
                IdentifierClaims::<T, I>::contains_key(item.identifier(), id),
                Error::<T, I>::IdentifierImmutable
            );

            let now = <frame_system::Pallet<T>>::block_number();
            let elapsed = now.saturating_sub(info.deposited_at);
            ensure!(
                elapsed <= T::CommitmentWindow::get(),
                Error::<T, I>::CommitmentWindowClosed
            );

            let new_bond = Self::do_apply_edit(id, item, info)?;
            Self::deposit_event(Event::Updated { id, new_bond });
            Ok(())
        }

        /// Cancel an own deposit while the commitment window is open. The
        /// unmultiplied base bond is refunded to the depositor; the
        /// multiplier premium is permanently transferred to the Treasury,
        /// which removes the burst-arbitrage opportunity (cf.
        /// `docs/economics.md` §5.5).
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::remove_own())]
        pub fn remove_own(origin: OriginFor<T>, id: MiddsId) -> DispatchResult {
            let caller = T::ProviderOrigin::ensure_origin(origin)?;

            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(info.depositor == caller, Error::<T, I>::NotProvider);
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);

            let now = <frame_system::Pallet<T>>::block_number();
            let elapsed = now.saturating_sub(info.deposited_at);
            ensure!(
                elapsed <= T::CommitmentWindow::get(),
                Error::<T, I>::CommitmentWindowClosed
            );

            // Depositor keeps `base_bond`, Treasury receives the multiplier premium.
            let premium = info.amount.saturating_sub(info.base_bond);
            Self::settle_bond(&info, premium)?;
            Self::cleanup_storage(id, &info)?;

            Self::deposit_event(Event::Refunded {
                id,
                depositor: info.depositor,
                refund: info.base_bond,
                premium_to_treasury: premium,
            });
            Ok(())
        }

        /// Permissionless catch-up for finalizations the eager `on_initialize`
        /// queue couldn't drain in time (cf. `docs/economics.md` §4.2). May be
        /// called by anyone at or after the record's expiry block.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::finalize_one())]
        pub fn finalize(_origin: OriginFor<T>, id: MiddsId) -> DispatchResult {
            // Origin is intentionally ignored: this is a maintenance crank.
            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);

            let now = <frame_system::Pallet<T>>::block_number();
            let elapsed = now.saturating_sub(info.deposited_at);
            ensure!(
                elapsed > T::CommitmentWindow::get(),
                Error::<T, I>::CommitmentWindowOpen
            );

            Self::do_finalize(id)
        }

        /// `ForceOrigin` edit, bypassing the commitment window. Bond delta is
        /// taken from / refunded to the original depositor; the deposit-time
        /// multiplier premium is preserved.
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::force_edit(item.encoded_size() as u32))]
        pub fn force_edit(origin: OriginFor<T>, id: MiddsId, item: T::Midds) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            Self::do_validate_format(&item)?;

            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);
            ensure!(
                IdentifierClaims::<T, I>::contains_key(item.identifier(), id),
                Error::<T, I>::IdentifierImmutable
            );

            Self::do_apply_edit(id, item, info)?;
            Self::deposit_event(Event::ForceEdited { id });
            Ok(())
        }

        /// `ForceOrigin` removal with full refund — Foundation indemnifies a
        /// good-faith typo. Only valid before finalization (after which the
        /// bond is already in the Treasury).
        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::force_remove_refund())]
        pub fn force_remove_refund(origin: OriginFor<T>, id: MiddsId) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            Self::do_force_remove_refund(id)
        }

        /// `ForceOrigin` removal without refund — flagged abuse. Pre-
        /// finalization the held bond is transferred to the Treasury;
        /// post-finalization the bond is already there and only storage is
        /// cleaned.
        #[pallet::call_index(6)]
        #[pallet::weight(T::WeightInfo::force_remove_slash())]
        pub fn force_remove_slash(origin: OriginFor<T>, id: MiddsId) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            Self::do_force_remove_slash(id)
        }

        /// Batch sudo cleanup. The same `slash` flag applies to every id in
        /// the list — callers that need a mix should split the call.
        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::force_remove_many(ids.len() as u32))]
        pub fn force_remove_many(
            origin: OriginFor<T>,
            ids: Vec<MiddsId>,
            slash: bool,
        ) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            for id in ids {
                if slash {
                    Self::do_force_remove_slash(id)?;
                } else {
                    Self::do_force_remove_refund(id)?;
                }
            }
            Ok(())
        }
    }

    impl<T: Config<I>, I: 'static> Pallet<T, I> {
        fn deposit_reason() -> T::RuntimeHoldReason {
            HoldReason::<I>::Deposit.into()
        }

        fn do_validate_format(item: &T::Midds) -> DispatchResult {
            item.validate_format()
                .map_err(|_| Error::<T, I>::InvalidFormat.into())
        }

        fn hash_payload(item: &T::Midds) -> T::Hash {
            <T::Hashing as Hash>::hash_of(item)
        }

        fn do_compute_base_bond(size: u32) -> BalanceOf<T, I> {
            let per_byte = T::DepositPerByte::get();
            let size_balance: BalanceOf<T, I> = size.into();
            T::DepositBase::get().saturating_add(per_byte.saturating_mul(size_balance))
        }

        /// Apply the current `M_fast × M_slow` multipliers to a base bond.
        ///
        /// Read once at deposit time and stored as the bond `amount`; the
        /// multiplier premium banked here is *not* re-priced on `update` /
        /// `force_edit` (only the base portion is). This is the spec's
        /// anti-arbitrage guarantee — see `docs/economics.md` §5.5.
        fn apply_multipliers(base: BalanceOf<T, I>) -> BalanceOf<T, I> {
            let fast = FastMultiplier::<T, I>::get();
            let slow = SlowMultiplier::<T, I>::get();
            let combined = fast.saturating_mul(slow);
            combined.saturating_mul_int(base)
        }

        /// Common write path shared by `update` and `force_edit`: recompute
        /// the unmultiplied base bond, adjust the hold by the base delta on
        /// the original depositor, and rewrite the storage entries. The
        /// deposit-time multiplier premium is preserved across edits.
        fn do_apply_edit(
            id: MiddsId,
            item: T::Midds,
            mut info: DepositOf<T, I>,
        ) -> Result<BalanceOf<T, I>, sp_runtime::DispatchError> {
            let new_size = item.encoded_size() as u32;
            let new_base = Self::do_compute_base_bond(new_size);
            let premium = info.amount.saturating_sub(info.base_bond);
            let new_amount = new_base.saturating_add(premium);

            // Update the payload hash index — `update` is allowed to land on
            // a different exact payload (within the commitment window) so we
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

            Self::do_adjust_hold(&info.depositor, info.amount, new_amount)?;

            Items::<T, I>::insert(id, item);
            info.amount = new_amount;
            info.base_bond = new_base;
            DepositInfo::<T, I>::insert(id, info);
            Ok(new_amount)
        }

        /// Release the full bond held against `info.depositor`, then move
        /// `premium_to_treasury` of their now-free balance to the Treasury.
        ///
        /// Shared by `remove_own`, `do_finalize`, `do_force_remove_refund`,
        /// and the live-bond branch of `do_force_remove_slash` — each picks
        /// a different `premium_to_treasury` (multiplier surplus, full bond,
        /// zero, full bond respectively) and otherwise reduces to this
        /// release-then-transfer dance. `transfer_on_hold` would do the two
        /// steps atomically but isn't in the bound trait set, hence the
        /// transient release into free balance.
        fn settle_bond(
            info: &DepositOf<T, I>,
            premium_to_treasury: BalanceOf<T, I>,
        ) -> DispatchResult {
            <T::Currency as MutateHold<T::AccountId>>::release(
                &Self::deposit_reason(),
                &info.depositor,
                info.amount,
                Precision::Exact,
            )
            .map_err(|_| Error::<T, I>::BondReleaseFailed)?;
            if !premium_to_treasury.is_zero() {
                <T::Currency as Mutate<T::AccountId>>::transfer(
                    &info.depositor,
                    &T::TreasuryAccount::get(),
                    premium_to_treasury,
                    Preservation::Expendable,
                )
                .map_err(|_| Error::<T, I>::BondTransferFailed)?;
            }
            Ok(())
        }

        fn do_adjust_hold(
            depositor: &T::AccountId,
            old: BalanceOf<T, I>,
            new: BalanceOf<T, I>,
        ) -> DispatchResult {
            if new > old {
                let delta = new.saturating_sub(old);
                <T::Currency as MutateHold<T::AccountId>>::hold(
                    &Self::deposit_reason(),
                    depositor,
                    delta,
                )
                .map_err(|_| Error::<T, I>::BondHoldFailed)?;
            } else if new < old {
                let delta = old.saturating_sub(new);
                <T::Currency as MutateHold<T::AccountId>>::release(
                    &Self::deposit_reason(),
                    depositor,
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
        fn do_finalize(id: MiddsId) -> DispatchResult {
            let mut info = match DepositInfo::<T, I>::get(id) {
                Some(i) if !i.finalized => i,
                Some(_) => return Ok(()),
                None => return Err(Error::<T, I>::MiddsNotFound.into()),
            };

            Self::settle_bond(&info, info.amount)?;

            let expiry = info.deposited_at.saturating_add(T::CommitmentWindow::get());
            PendingFinalization::<T, I>::remove(expiry, id);

            let amount_to_treasury = info.amount;
            info.finalized = true;
            DepositInfo::<T, I>::insert(id, &info);

            Self::deposit_event(Event::Finalized {
                id,
                depositor: info.depositor,
                amount_to_treasury,
            });
            Ok(())
        }

        fn do_force_remove_refund(id: MiddsId) -> DispatchResult {
            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);

            Self::settle_bond(&info, Zero::zero())?;

            let refund = info.amount;
            let depositor = info.depositor.clone();
            Self::cleanup_storage(id, &info)?;
            Self::deposit_event(Event::ForceRemovedRefund {
                id,
                depositor,
                refund,
            });
            Ok(())
        }

        fn do_force_remove_slash(id: MiddsId) -> DispatchResult {
            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;

            let amount_to_treasury = if info.finalized {
                // Bond already moved at finalize time — nothing to send now.
                Zero::zero()
            } else {
                Self::settle_bond(&info, info.amount)?;
                info.amount
            };

            let depositor = info.depositor.clone();
            Self::cleanup_storage(id, &info)?;
            Self::deposit_event(Event::ForceRemovedSlash {
                id,
                depositor,
                amount_to_treasury,
            });
            Ok(())
        }

        /// Wipe every storage entry tied to `id`. Called by `remove_own` and
        /// the two `force_remove_*` paths — never by `finalize`, which keeps
        /// the record on-chain (just marks it permanent).
        fn cleanup_storage(id: MiddsId, info: &DepositOf<T, I>) -> DispatchResult {
            let item = Items::<T, I>::take(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            IdentifierClaims::<T, I>::remove(item.identifier(), id);
            PayloadHashes::<T, I>::remove(info.payload_hash);
            DepositInfo::<T, I>::remove(id);
            let expiry = info.deposited_at.saturating_add(T::CommitmentWindow::get());
            PendingFinalization::<T, I>::remove(expiry, id);
            Ok(())
        }

        /// Bump the per-block and current-bucket demand counters. Called once
        /// per successful `deposit`.
        fn record_deposit_demand() {
            DepositsThisBlock::<T, I>::mutate(|c| *c = c.saturating_add(1));
            let head = SlowWindowHead::<T, I>::get() as usize;
            SlowWindowBuckets::<T, I>::mutate(|buckets| {
                if let Some(bucket) = buckets.get_mut(head) {
                    *bucket = bucket.saturating_add(1);
                }
            });
        }

        /// EIP-1559-style adjustment: ±AdjustmentRate per block, clamped to
        /// [Min, Max]. Concrete numbers per `docs/economics.md` §5.1.
        fn adjust_fast_multiplier(observed: u32) {
            let target = T::FastTargetPerBlock::get();
            let new = Self::adjust_multiplier(
                FastMultiplier::<T, I>::get(),
                observed,
                target,
                T::FastAdjustmentRate::get(),
                T::FastMultiplierMin::get(),
                T::FastMultiplierMax::get(),
            );
            FastMultiplier::<T, I>::put(new);
        }

        fn rotate_slow_bucket() {
            let len = SLOW_WINDOW_DAYS as u8;
            let next_head = SlowWindowHead::<T, I>::get().wrapping_add(1) % len;
            SlowWindowBuckets::<T, I>::mutate(|buckets| {
                if let Some(slot) = buckets.get_mut(next_head as usize) {
                    *slot = 0;
                }
            });
            SlowWindowHead::<T, I>::put(next_head);
        }

        fn adjust_slow_multiplier() {
            let total: u32 = SlowWindowBuckets::<T, I>::get()
                .iter()
                .copied()
                .fold(0u32, |acc, x| acc.saturating_add(x));
            let target = T::SlowTargetPerWindow::get();
            let new = Self::adjust_multiplier(
                SlowMultiplier::<T, I>::get(),
                total,
                target,
                T::SlowAdjustmentRate::get(),
                T::SlowMultiplierMin::get(),
                T::SlowMultiplierMax::get(),
            );
            SlowMultiplier::<T, I>::put(new);
        }

        /// EIP-1559-style adjustment: shift `current` by ±`rate` toward the
        /// observed/target ratio, clamped to `[min, max]`. The down step uses
        /// `current / (1 + rate)` so a sequence of opposing steps round-trips
        /// back to the original value (rather than asymmetrically biasing
        /// against the floor under `current * (1 - rate)`).
        fn adjust_multiplier(
            current: FixedU128,
            observed: u32,
            target: u32,
            rate: FixedU128,
            min: FixedU128,
            max: FixedU128,
        ) -> FixedU128 {
            use core::cmp::Ordering;
            let one = FixedU128::one();
            let factor_up = one.saturating_add(rate);
            let candidate = match observed.cmp(&target) {
                Ordering::Greater => current.saturating_mul(factor_up),
                Ordering::Less => one
                    .const_checked_div(factor_up)
                    .map(|down| current.saturating_mul(down))
                    .unwrap_or(current),
                Ordering::Equal => current,
            };
            if candidate < min {
                min
            } else if candidate > max {
                max
            } else {
                candidate
            }
        }

        // ---- read helpers used by the runtime API ------------------------

        /// Current bond price for a payload of `size` bytes — base × M_fast ×
        /// M_slow. Read at the current block.
        pub fn current_deposit_price(size: u32) -> BalanceOf<T, I> {
            Self::apply_multipliers(Self::do_compute_base_bond(size))
        }

        /// Current `(M_fast, M_slow)`.
        pub fn current_multipliers() -> (FixedU128, FixedU128) {
            (FastMultiplier::<T, I>::get(), SlowMultiplier::<T, I>::get())
        }

        /// Static target for the rolling 7-day window — by spec, parameter,
        /// not on-chain state.
        pub fn weekly_target() -> u32 {
            T::SlowTargetPerWindow::get()
        }

        /// Sum of the 7 daily buckets — approximates "deposits seen in the
        /// last 7 days" with day-resolution.
        pub fn weekly_actual() -> u32 {
            SlowWindowBuckets::<T, I>::get()
                .iter()
                .copied()
                .fold(0u32, |acc, x| acc.saturating_add(x))
        }

        /// All `MiddsId`s registered against the canonical identifier
        /// (multi-claim).
        pub fn lookup_by_identifier(identifier: <T::Midds as Midds>::Identifier) -> Vec<MiddsId> {
            IdentifierClaims::<T, I>::iter_prefix(identifier)
                .map(|(id, _)| id)
                .collect()
        }
    }
}
