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
//!
//! ## Module layout
//!
//! - `lib.rs` — `#[frame::pallet]` module: `Config`, storage, events,
//!   errors, hooks, extrinsics. Each extrinsic delegates its body to a
//!   helper in `impls.rs`.
//! - `impls.rs` — bond accounting + lifecycle bodies + on-behalf signature
//!   verification + runtime-API readers (`current_deposit_price`,
//!   `lookup_by_identifier`).
//! - `multipliers.rs` — pure pricing dynamics: `M_fast` / `M_slow` adjustment,
//!   daily bucket rotation, demand recording, plus the multiplier-side
//!   readers (`current_multipliers`, `weekly_target`, `weekly_actual`).
//! - `types.rs` — `Deposit`, on-behalf payloads, `RemovalKind` /
//!   `RemovalRequest`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use multipliers::SLOW_WINDOW_DAYS;
pub use pallet::*;
pub use types::*;
pub use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
pub use benchmarking::BenchmarkHelper;

mod impls;
mod multipliers;
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

use frame::deps::frame_system;
use frame::token::fungible::Inspect;

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

#[frame::pallet]
pub mod pallet {
    use super::{BalanceOf, WeightInfo};
    use crate::multipliers::{DefaultSlowBuckets, DefaultUnitMultiplier, SLOW_WINDOW_DAYS};
    use crate::types::{
        DepositOnBehalfPayload, OnBehalfAction, RemovalKind, RemovalRequest, RemoveOnBehalfPayload,
        UpdateOnBehalfPayload,
    };
    use alloc::vec::Vec;
    // `frame::deps::frame_support` keeps the `#[frame_support::transactional]`
    // call attributes and `frame_support::storage::with_storage_layer` resolving
    // under the umbrella. `frame::prelude` covers `BoundedVec`, `EnsureOrigin`,
    // the storage/`frame_system` pallet preludes, `FixedU128`, `IdentifyAccount`,
    // `One`, `Saturating`, `Zero`, …
    use frame::deps::frame_support;
    use frame::prelude::*;
    use frame::token::fungible::{Mutate, MutateHold};
    use frame::traits::Verify;
    use midds_traits::{Midds, MiddsId};
    use parity_scale_codec::Encode;

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

        /// Origin allowed to call `deposit` / `update` / `remove_own` and the
        /// on-behalf variants (the operator side, in the latter case).
        type ProviderOrigin: EnsureOrigin<Self::RuntimeOrigin, Success = Self::AccountId>;

        /// Origin allowed to call the `force_*` extrinsics (sudo at launch).
        type ForceOrigin: EnsureOrigin<Self::RuntimeOrigin>;

        /// Off-chain signature type used to authorize on-behalf operations.
        /// The owner signs a SCALE-encoded payload (cf.
        /// [`DepositOnBehalfPayload`] / [`UpdateOnBehalfPayload`]) and the
        /// operator submits it on-chain along with their own runtime
        /// signature.
        type OffchainSignature: Verify<Signer = Self::Signer> + Parameter + DecodeWithMemTracking;

        /// Signer type backing [`Config::OffchainSignature`]. Maps to the
        /// pallet's `AccountId` so an off-chain signature can be cross-checked
        /// against an on-chain owner.
        type Signer: IdentifyAccount<AccountId = Self::AccountId>
            + Parameter
            + DecodeWithMemTracking;

        /// Foundation Treasury account that receives finalized bonds and
        /// non-refundable multiplier premiums. Per `docs/economics.md` §9 we
        /// never burn — the AFT is recycled via Treasury governance.
        type TreasuryAccount: Get<Self::AccountId>;

        // Note: the bond formula parameters (`DepositBase` and
        // `DepositPerByte`) used to live here as `Get<BalanceOf<_>>`. Per
        // `docs/economics.md` §13.4 they have been moved to runtime-mutable
        // [`DepositBase`] / [`DepositPerByte`] storage values (initialised
        // via [`GenesisConfig`], updated via
        // [`Pallet::force_set_deposit_base`] /
        // [`Pallet::force_set_deposit_per_byte`]) so the Foundation can
        // recalibrate the bond without a runtime upgrade.

        /// Length of the refundable commitment window, in blocks.
        ///
        /// Boundary semantics (cf. `docs/economics.md` §4): the window is
        /// strictly less than `CommitmentWindow`. At block `expiry =
        /// deposited_at + CommitmentWindow` the record is finalizable —
        /// `update` and `remove_own` reject (`CommitmentWindowClosed`),
        /// `finalize` succeeds. This makes the post-window state
        /// deterministic regardless of intra-block extrinsic order.
        #[pallet::constant]
        type CommitmentWindow: Get<BlockNumberFor<Self>>;

        /// Hard cap on the number of pending finalizations executed by
        /// `on_initialize` per block — and, equivalently, the number of
        /// finalization-queue prefixes the eager sweep probes per block (so
        /// the cursor catch-up after a backlog can never do an unbounded,
        /// unweighed `iter_prefix` scan). Leftover entries are picked up by
        /// subsequent blocks' `on_initialize` or by a permissionless
        /// `finalize(id)` call (cf. `docs/economics.md` §4.2).
        ///
        /// **Calibration**: set this **≥ the maximum sustainable deposit rate
        /// per block**, not merely [`Config::FastTargetPerBlock`] — the latter
        /// is a soft target the multipliers price *around*, not a throughput
        /// cap. If deposits durably outpace this cap, the finalization queue
        /// (and the conversion of bonds into Treasury revenue) falls
        /// progressively behind.
        #[pallet::constant]
        type MaxFinalizationsPerBlock: Get<u32>;

        /// Hard cap on the number of records `force_remove_many` may process
        /// per call. Without this bound, `ForceOrigin` could submit an
        /// arbitrarily large list and blow the per-block weight budget.
        #[pallet::constant]
        type MaxRemovalsPerCall: Get<u32>;

        /// Number of blocks composing one logical day. Drives the slow-window
        /// bucket rotation. With 6s slots: `BlocksPerDay = 14_400`.
        #[pallet::constant]
        type BlocksPerDay: Get<BlockNumberFor<Self>>;

        /// Target number of deposits per block before `M_fast` adjusts upward.
        #[pallet::constant]
        type FastTargetPerBlock: Get<u32>;

        /// Per-block step rate applied to `M_fast`. The actual move per
        /// block is `rate × |observed − target| / target`, so a deviation
        /// of `k × target` yields a `k × rate` step (proportional EIP-1559
        /// form). At deviation = 1× target the move equals `rate` exactly.
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

        /// Per-day step rate applied to `M_slow` (same proportional EIP-1559
        /// form as `FastAdjustmentRate`, evaluated against the rolling
        /// 7-day bucket sum).
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

        /// Helper producing worst-case payloads and owner signatures for
        /// benchmarking. Each runtime instance supplies its own
        /// implementation tailored to its `Midds` / signature types.
        #[cfg(feature = "runtime-benchmarks")]
        type BenchmarkHelper: crate::BenchmarkHelper<Self::Midds, Self::OffchainSignature, Self::AccountId>;
    }

    /// Reasons the pallet holds funds against an account.
    #[pallet::composite_enum]
    pub enum HoldReason<I: 'static = ()> {
        /// Bond covering a stored MIDDS record.
        #[codec(index = 0)]
        Deposit,
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
    /// uniqueness: byte-identical payloads are rejected, but different
    /// payloads sharing an identifier are accepted (multi-claim).
    ///
    /// This stops *accidental* exact re-submissions only — flipping a single
    /// optional byte yields a different hash and a distinct accepted record.
    /// Adversarial near-duplicate suppression is therefore **not** an on-chain
    /// guarantee; it is a deliberate off-chain / PoM concern (cf.
    /// `docs/economics.md` §10.1). The on-chain deterrent against near-dup
    /// flooding is the dynamic bond, not this index.
    #[pallet::storage]
    pub type PayloadHashes<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Identity, T::Hash, MiddsId>;

    /// Bond information for each stored record.
    #[pallet::storage]
    pub type DepositInfo<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, MiddsId, super::DepositOf<T, I>>;

    /// Finalization queue keyed by expiry block (= `deposited_at +
    /// CommitmentWindow`). `iter_prefix(n)` enumerates the records due at
    /// block `n` for the eager `on_initialize` path; older prefixes are
    /// rolled over via [`NextFinalizationScan`] when a previous block hit
    /// the per-block cap.
    #[pallet::storage]
    pub type PendingFinalization<T: Config<I>, I: 'static = ()> =
        StorageDoubleMap<_, Twox64Concat, BlockNumberFor<T>, Identity, MiddsId, (), OptionQuery>;

    /// Rolling cursor pointing at the earliest finalization-queue prefix
    /// that may still hold entries. `on_initialize(n)` drains prefixes
    /// from this cursor up to `n` rather than only the current block, so a
    /// burst that overflows [`Config::MaxFinalizationsPerBlock`] catches up
    /// on subsequent blocks instead of stranding entries forever under
    /// their original expiry prefix. Permissionless [`Pallet::finalize`]
    /// remains as a tertiary fallback.
    #[pallet::storage]
    pub type NextFinalizationScan<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BlockNumberFor<T>, ValueQuery>;

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

    /// Per-owner monotonic counter feeding signed-payload replay protection
    /// for the on-behalf flows. The owner's signed payload must carry the
    /// current value; on accept the counter bumps by one.
    #[pallet::storage]
    pub type OnBehalfNonce<T: Config<I>, I: 'static = ()> =
        StorageMap<_, Blake2_128Concat, T::AccountId, u64, ValueQuery>;

    /// Flat part of the unmultiplied bond formula. Initialised from
    /// [`GenesisConfig::deposit_base`] and mutable via
    /// [`Pallet::force_set_deposit_base`] (`ForceOrigin`). Lives in storage
    /// rather than as a `Config` constant so the Foundation can recalibrate
    /// the bond floor without a runtime upgrade — `docs/economics.md` §13.4.
    #[pallet::storage]
    pub type DepositBase<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BalanceOf<T, I>, ValueQuery>;

    /// Per-byte multiplier applied to the SCALE-encoded payload size in the
    /// unmultiplied bond formula. Sibling of [`DepositBase`] with the same
    /// governance rationale: initialised from
    /// [`GenesisConfig::deposit_per_byte`], mutated via
    /// [`Pallet::force_set_deposit_per_byte`].
    #[pallet::storage]
    pub type DepositPerByte<T: Config<I>, I: 'static = ()> =
        StorageValue<_, BalanceOf<T, I>, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config<I>, I: 'static = ()> {
        /// A new MIDDS record was deposited. `depositor` is the attribution
        /// owner; `bond_payer` is who actually posted the bond (= original
        /// sponsor). They differ after [`Pallet::deposit_on_behalf`].
        ///
        /// Only the `sponsor_layer` is populated at this point; the optional
        /// `owner_layer` may appear later if the owner extends a sponsored
        /// record via plain `update` (cf. the web3 escape hatch in
        /// `docs/economics.md`).
        Deposited {
            id: MiddsId,
            depositor: T::AccountId,
            bond_payer: T::AccountId,
            bond: BalanceOf<T, I>,
            base_bond: BalanceOf<T, I>,
        },
        /// A MIDDS record was updated by its depositor (`update`), by the
        /// original sponsor (`update_on_behalf`), or by `force_edit`. The
        /// per-layer holds after the edit are surfaced so off-chain
        /// consumers can reflect the stratified bond without re-querying
        /// `DepositInfo`.
        Updated {
            id: MiddsId,
            sponsor_bond: BalanceOf<T, I>,
            owner_bond: BalanceOf<T, I>,
        },
        /// A MIDDS record was edited by `ForceOrigin`.
        ForceEdited { id: MiddsId },
        /// The owner cancelled within the commitment window. Each layer's
        /// net refund — `min(amount, base)` — returns to its payer
        /// (`sponsor_refund` to the original sponsor, `owner_refund` to the
        /// depositor when an owner layer existed). `premium_to_treasury` is
        /// the aggregated multiplier surplus permanently transferred to the
        /// Treasury (taken from each layer's hold respectively).
        ///
        /// The `min(amount, base)` framing — rather than raw `base` — matters
        /// when a layer was banked under `M < 1` (multipliers at their floor):
        /// the held `amount` was below `base`, no premium was ever paid, and
        /// only `amount` returns to the payer. Off-chain consumers should read
        /// `sponsor_refund` / `owner_refund` directly as the per-layer balance
        /// movement, without re-clamping against locally-tracked state.
        Refunded {
            id: MiddsId,
            depositor: T::AccountId,
            sponsor: T::AccountId,
            sponsor_refund: BalanceOf<T, I>,
            owner_refund: BalanceOf<T, I>,
            premium_to_treasury: BalanceOf<T, I>,
        },
        /// A MIDDS record's bond was finalized: every layer's full hold moved
        /// to the Treasury and the record became permanent.
        Finalized {
            id: MiddsId,
            depositor: T::AccountId,
            sponsor: T::AccountId,
            amount_to_treasury: BalanceOf<T, I>,
        },
        /// A MIDDS record was removed by `ForceOrigin` with the bond fully
        /// refunded to each layer's payer (good-faith typo path).
        ForceRemovedRefund {
            id: MiddsId,
            depositor: T::AccountId,
            sponsor: T::AccountId,
            sponsor_refund: BalanceOf<T, I>,
            owner_refund: BalanceOf<T, I>,
        },
        /// A MIDDS record was removed by `ForceOrigin` without refund. If the
        /// bond was still held at the time, it was sent to the Treasury;
        /// post-finalization the bond was already there.
        ForceRemovedSlash {
            id: MiddsId,
            depositor: T::AccountId,
            sponsor: T::AccountId,
            amount_to_treasury: BalanceOf<T, I>,
        },
        /// `force_set_deposit_base` was called; the flat part of the
        /// unmultiplied bond formula changed. New deposits and the per-layer
        /// extension paths price from `new` immediately; existing layers keep
        /// their banked base (anti-arbitrage `docs/economics.md` §5.5).
        DepositBaseSet { new: BalanceOf<T, I> },
        /// `force_set_deposit_per_byte` was called; the per-byte multiplier
        /// of the unmultiplied bond formula changed. Same per-layer
        /// stickiness as `DepositBaseSet`.
        DepositPerByteSet { new: BalanceOf<T, I> },
        /// A `force_remove_many` entry could not be applied (e.g. the id was
        /// already gone, or `Refund` was requested on an already-finalized
        /// record). The batch is best-effort and continues; this surfaces the
        /// skipped id/kind so the `ForceOrigin` caller can reconcile.
        RemovalFailed { id: MiddsId, kind: RemovalKind },
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
        /// On-behalf payload signature did not verify against the provided
        /// owner.
        InvalidSignature,
        /// Provided nonce does not match `OnBehalfNonce[owner]`.
        InvalidNonce,
        /// Current block exceeds the `valid_until` window the owner pinned
        /// in their on-behalf payload — the signature is no longer
        /// admissible. Reuse requires a fresh signature with a new
        /// `valid_until`.
        SignatureExpired,
        /// Bumping `OnBehalfNonce[owner]` would overflow `u64`. With a
        /// `saturating_add` the counter would freeze at `u64::MAX` and
        /// the same signature could be replayed indefinitely; this error
        /// surfaces the boundary explicitly. Unreachable in practice
        /// (`u64::MAX` signatures per owner) but cheap to guard against.
        NonceOverflow,
        /// `update_on_behalf` caller is not the original sponsor (=
        /// `sponsor_layer.payer`). Only the deposit-time sponsor may extend
        /// their own layer.
        WrongSponsor,
    }

    /// Initial values for the runtime-mutable bond parameters. A real runtime
    /// **must** override these at chain bootstrap — the right calibration
    /// depends on the host chain's currency unit and economic targets
    /// (cf. `docs/economics.md` §6). The [`Default`] impl supplies a
    /// deliberately non-functional `1`-planck placeholder: non-zero so the
    /// bond formula stays non-degenerate (`base × M` can never collapse to a
    /// free deposit), but economically meaningless so a forgetful runtime is
    /// obviously misconfigured rather than silently free.
    #[pallet::genesis_config]
    pub struct GenesisConfig<T: Config<I>, I: 'static = ()> {
        pub deposit_base: BalanceOf<T, I>,
        pub deposit_per_byte: BalanceOf<T, I>,
        #[serde(skip)]
        pub _config: core::marker::PhantomData<(T, I)>,
    }

    impl<T: Config<I>, I: 'static> Default for GenesisConfig<T, I> {
        fn default() -> Self {
            // Non-zero placeholder: keeps the bond formula non-degenerate (so
            // `base × M_fast × M_slow` can never be a free deposit) and lets
            // the `construct_runtime!`-generated `test_genesis_config_builds`
            // pass, while staying an obvious "override me" value for any
            // production runtime.
            Self {
                deposit_base: <BalanceOf<T, I> as One>::one(),
                deposit_per_byte: <BalanceOf<T, I> as One>::one(),
                _config: core::marker::PhantomData,
            }
        }
    }

    #[pallet::genesis_build]
    impl<T: Config<I>, I: 'static> BuildGenesisConfig for GenesisConfig<T, I> {
        fn build(&self) {
            // The bond floor is the load-bearing anti-spam parameter: with a
            // zero `deposit_base`, `base × M_fast × M_slow` collapses to zero
            // and every permissionless deposit becomes free — and multi-claim
            // has no per-account limit by design. Reject an *explicit* zero
            // outright (a copy-paste / unfilled-placeholder hazard) so the
            // chain fails to build rather than silently shipping a registry
            // with no spam cost. The non-functional `Default` (1 planck) is
            // itself non-zero, so a forgetful runtime still gets a
            // non-degenerate formula. Cf. `docs/economics.md` §6.
            assert!(
                !self.deposit_base.is_zero(),
                "pallet-midds: GenesisConfig::deposit_base must be non-zero \
                 (it is the anti-spam bond floor); configure it explicitly per \
                 docs/economics.md §6"
            );
            DepositBase::<T, I>::put(self.deposit_base);
            DepositPerByte::<T, I>::put(self.deposit_per_byte);
        }
    }

    #[pallet::hooks]
    impl<T: Config<I>, I: 'static> Hooks<BlockNumberFor<T>> for Pallet<T, I> {
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            let mut weight = T::DbWeight::get().reads_writes(1, 1);

            let fast_count = DepositsThisBlock::<T, I>::take();
            Self::adjust_fast_multiplier(fast_count);

            if !n.is_zero() && (n % T::BlocksPerDay::get()).is_zero() {
                // Adjust **before** rotating: at a daily rollover the head
                // bucket holds the day that just ended, so the 7 live buckets
                // are exactly the last 7 full days. Rotating first would zero
                // the new bucket and leave only 6 days of data feeding the
                // adjustment (cf. the previous off-by-one).
                Self::adjust_slow_multiplier();
                Self::rotate_slow_bucket();
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(2, 2));
            }

            let cap = T::MaxFinalizationsPerBlock::get() as usize;
            let mut remaining = cap;
            // Bound the number of finalization-queue prefixes probed this
            // block. Without this, a cursor left far behind `n` (e.g. after a
            // sustained backlog finally drains) could sweep an unbounded run
            // of empty prefixes in a single block — real `iter_prefix` DB
            // reads that the old weight formula never accounted for. The
            // cursor catches up over subsequent blocks; `finalize(id)` is the
            // immediate permissionless fallback for anything still due.
            let mut probes_remaining = cap;
            let initial_cursor = NextFinalizationScan::<T, I>::get();
            let mut cursor = initial_cursor;
            weight = weight.saturating_add(T::DbWeight::get().reads(1));
            while cursor <= n && remaining > 0 && probes_remaining > 0 {
                let due: Vec<MiddsId> = PendingFinalization::<T, I>::iter_prefix(cursor)
                    .take(remaining)
                    .map(|(id, _)| id)
                    .collect();
                // A prefix probe costs at least one DB read even when empty —
                // charge it so the catch-up scan is reflected in block weight.
                weight = weight.saturating_add(T::DbWeight::get().reads(1));
                probes_remaining = probes_remaining.saturating_sub(1);
                let count = due.len();
                for id in due {
                    // Each finalization runs in its own storage layer so a
                    // partial multi-layer settlement (e.g. sponsor layer moved
                    // but owner layer's Treasury transfer failing) can never
                    // persist: on error the record rolls back to a clean,
                    // fully-held, not-yet-finalized state, recoverable via
                    // `finalize(id)`. The bond is never half-released.
                    let _ = frame_support::storage::with_storage_layer(|| Self::do_finalize(id));
                    weight = weight.saturating_add(T::WeightInfo::finalize_one());
                }
                remaining = remaining.saturating_sub(count);
                if remaining > 0 && probes_remaining > 0 {
                    cursor = cursor.saturating_add(One::one());
                }
            }
            if cursor != initial_cursor {
                NextFinalizationScan::<T, I>::put(cursor);
                weight = weight.saturating_add(T::DbWeight::get().writes(1));
            }

            weight
        }
    }

    #[pallet::call]
    impl<T: Config<I>, I: 'static> Pallet<T, I> {
        /// Deposit a new MIDDS record. The caller pays a multiplied,
        /// size-proportional bond which is held against their account for
        /// `CommitmentWindow` blocks. The record is published immediately
        /// and attributed to the caller.
        #[pallet::call_index(0)]
        #[pallet::weight(T::WeightInfo::deposit(item.encoded_size() as u32))]
        #[frame_support::transactional]
        pub fn deposit(origin: OriginFor<T>, item: T::Midds) -> DispatchResult {
            let who = T::ProviderOrigin::ensure_origin(origin)?;
            Self::do_deposit(who.clone(), who, item)
        }

        /// Sponsored deposit: the operator (= caller) pays the bond and fees
        /// while the record is attributed to `owner`. The owner authorizes
        /// the operation off-chain by signing a [`DepositOnBehalfPayload`]
        /// that pins the operator account, the payload, and a nonce
        /// (replay protection via [`OnBehalfNonce`]).
        ///
        /// On success, the bond is held against the operator; refunds (via
        /// `remove_own`, `force_remove_refund`, the within-window flow) and
        /// the multiplier premium / finalized bond all flow to/from the
        /// operator. The owner can still call `remove_own` — they retain
        /// attribution authority — and the resulting refund goes to the
        /// operator who took the financial risk.
        #[pallet::call_index(8)]
        #[pallet::weight(T::WeightInfo::deposit_on_behalf(item.encoded_size() as u32))]
        #[frame_support::transactional]
        pub fn deposit_on_behalf(
            origin: OriginFor<T>,
            owner: T::AccountId,
            item: T::Midds,
            nonce: u64,
            valid_until: BlockNumberFor<T>,
            signature: T::OffchainSignature,
        ) -> DispatchResult {
            let operator = T::ProviderOrigin::ensure_origin(origin)?;

            Self::ensure_signature_fresh(valid_until)?;
            Self::consume_on_behalf_authorization(&owner, nonce, &signature, || {
                DepositOnBehalfPayload {
                    kind: Self::kind_bytes(),
                    genesis_hash: Self::genesis_hash(),
                    action: OnBehalfAction::Deposit,
                    item: item.clone(),
                    operator: operator.clone(),
                    nonce,
                    valid_until,
                }
            })?;

            Self::do_deposit(owner, operator, item)
        }

        /// Update an existing MIDDS record. Only the original depositor may
        /// call this, and only while still inside the commitment window. The
        /// canonical identifier is immutable.
        ///
        /// **Self-deposit** (`depositor == sponsor_layer.payer`): the
        /// resulting base delta extends the sponsor layer (no new premium
        /// banked, premium is sticky to deposit time per `docs/economics.md`
        /// §5.5).
        ///
        /// **Sponsored record** (`depositor != sponsor_layer.payer`): the
        /// caller pays the delta out of their own balance — the **web3
        /// escape hatch**. On grow, the delta materializes the
        /// `owner_layer` (creating it on the first solo update with the
        /// current multipliers banking a fresh premium) so the owner
        /// extends their record without depending on the sponsor's balance.
        /// On shrink, the released amount drains the owner layer first
        /// (LIFO), overflowing into the sponsor layer if the shrink exceeds
        /// the owner's contribution — fair, since a smaller payload no
        /// longer requires the sponsor's full bond.
        #[pallet::call_index(1)]
        #[pallet::weight(T::WeightInfo::update(item.encoded_size() as u32))]
        #[frame_support::transactional]
        pub fn update(origin: OriginFor<T>, id: MiddsId, item: T::Midds) -> DispatchResult {
            let caller = T::ProviderOrigin::ensure_origin(origin)?;
            Self::enforce_format(&item)?;

            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(info.depositor == caller, Error::<T, I>::NotProvider);
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);
            Self::ensure_identifier_unchanged(id, &item)?;
            Self::ensure_in_window(&info)?;

            let updated = Self::apply_edit(id, item, info, &caller, true)?;
            Self::emit_updated_event(id, &updated);
            Ok(())
        }

        /// Sponsored update. The original sponsor (= caller) extends their
        /// own layer on the owner's behalf. The owner authorizes the new
        /// payload off-chain via a signed [`UpdateOnBehalfPayload`]. The
        /// caller **must** match the record's `sponsor_layer.payer` — only
        /// the deposit-time sponsor may grow their own hold; to switch
        /// sponsor, the owner must `remove_own` and re-deposit.
        ///
        /// Co-exists with the owner-driven escape hatch: a sponsored record
        /// the owner has already extended via plain `update` (i.e.
        /// `owner_layer = Some(_)`) can still receive sponsor-driven
        /// extensions through this path — each layer grows independently.
        #[pallet::call_index(9)]
        #[pallet::weight(T::WeightInfo::update_on_behalf(item.encoded_size() as u32))]
        #[frame_support::transactional]
        pub fn update_on_behalf(
            origin: OriginFor<T>,
            id: MiddsId,
            item: T::Midds,
            nonce: u64,
            valid_until: BlockNumberFor<T>,
            signature: T::OffchainSignature,
        ) -> DispatchResult {
            let operator = T::ProviderOrigin::ensure_origin(origin)?;
            Self::enforce_format(&item)?;
            Self::ensure_signature_fresh(valid_until)?;

            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);
            ensure!(
                info.sponsor_layer.payer == operator,
                Error::<T, I>::WrongSponsor
            );
            Self::ensure_identifier_unchanged(id, &item)?;
            Self::ensure_in_window(&info)?;

            let owner = info.depositor.clone();
            Self::consume_on_behalf_authorization(&owner, nonce, &signature, || {
                UpdateOnBehalfPayload {
                    kind: Self::kind_bytes(),
                    genesis_hash: Self::genesis_hash(),
                    action: OnBehalfAction::Update,
                    id,
                    item: item.clone(),
                    operator: operator.clone(),
                    nonce,
                    valid_until,
                }
            })?;

            let updated = Self::apply_edit(id, item, info, &operator, true)?;
            Self::emit_updated_event(id, &updated);
            Ok(())
        }

        /// Cancel an own deposit while the commitment window is open. Each
        /// layer's unmultiplied base returns to its payer (sponsor and
        /// owner separately); each layer's multiplier premium is
        /// permanently transferred to the Treasury, which removes the
        /// burst-arbitrage opportunity (cf. `docs/economics.md` §5.5) — and
        /// keeps the sponsor and the owner financially insulated.
        ///
        /// Authority sits with `depositor` (the attribution owner). On a
        /// sponsored record, the owner can still cancel and the sponsor's
        /// base goes back to the sponsor — that is the financial risk the
        /// sponsor accepted at `deposit_on_behalf` time.
        #[pallet::call_index(2)]
        #[pallet::weight(T::WeightInfo::remove_own())]
        #[frame_support::transactional]
        pub fn remove_own(origin: OriginFor<T>, id: MiddsId) -> DispatchResult {
            let caller = T::ProviderOrigin::ensure_origin(origin)?;

            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(info.depositor == caller, Error::<T, I>::NotProvider);
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);
            Self::ensure_in_window(&info)?;

            Self::do_remove_own(id, info)
        }

        /// Permissionless catch-up for finalizations the eager `on_initialize`
        /// queue couldn't drain in time (cf. `docs/economics.md` §4.2). May be
        /// called by anyone at or after the record's expiry block.
        #[pallet::call_index(3)]
        #[pallet::weight(T::WeightInfo::finalize_one())]
        #[frame_support::transactional]
        pub fn finalize(_origin: OriginFor<T>, id: MiddsId) -> DispatchResult {
            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);

            let now = <frame_system::Pallet<T>>::block_number();
            let elapsed = now.saturating_sub(info.deposited_at);
            ensure!(
                elapsed >= T::CommitmentWindow::get(),
                Error::<T, I>::CommitmentWindowOpen
            );

            Self::do_finalize(id)
        }

        /// `ForceOrigin` edit, bypassing the commitment window. The bond
        /// delta (if any) is taken from / refunded to the **original
        /// sponsor** — governance edits are routed through the sponsor
        /// layer, never conjure an owner layer out of an admin
        /// intervention. The deposit-time multiplier premium is
        /// preserved.
        ///
        /// Post-finalization variant (cf. `docs/economics.md` §4): when
        /// the record is already finalized the bond is in the Treasury
        /// and there is nothing to re-price — `force_edit` then only
        /// rewrites the on-chain payload and the reverse hash index.
        /// Lets governance correct a typo on a permanent record without
        /// slashing + re-depositing (which would lose attribution).
        ///
        /// Note this lets `ForceOrigin` enlarge a finalized record's encoded
        /// footprint without posting additional bond (the original bond is
        /// already spent to the Treasury). That is an accepted
        /// governance-trust assumption, not a user-reachable path.
        #[pallet::call_index(4)]
        #[pallet::weight(T::WeightInfo::force_edit(item.encoded_size() as u32))]
        #[frame_support::transactional]
        pub fn force_edit(origin: OriginFor<T>, id: MiddsId, item: T::Midds) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            Self::enforce_format(&item)?;

            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            Self::ensure_identifier_unchanged(id, &item)?;

            if info.finalized {
                Self::apply_finalized_edit(id, item, info)?;
            } else {
                let sponsor = info.sponsor_layer.payer.clone();
                // `count_demand = false`: a governance edit is not organic
                // deposit demand and must not move the dynamic multipliers.
                Self::apply_edit(id, item, info, &sponsor, false)?;
            }
            Self::deposit_event(Event::ForceEdited { id });
            Ok(())
        }

        /// `ForceOrigin` removal with full refund — Foundation indemnifies a
        /// good-faith typo. Only valid before finalization (after which the
        /// bond is already in the Treasury).
        #[pallet::call_index(5)]
        #[pallet::weight(T::WeightInfo::force_remove_refund())]
        #[frame_support::transactional]
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
        #[frame_support::transactional]
        pub fn force_remove_slash(origin: OriginFor<T>, id: MiddsId) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            Self::do_force_remove_slash(id)
        }

        /// Batch sudo cleanup. Each request carries its own
        /// [`RemovalKind`], so a single call may interleave good-faith
        /// refunds and abuse slashes. Bounded by
        /// [`Config::MaxRemovalsPerCall`] for predictable weight.
        ///
        /// **Best-effort**: each entry is applied in its own storage layer and
        /// a failing entry (e.g. an id already removed, or a `Refund`
        /// requested on an already-finalized record) is skipped with a
        /// [`Event::RemovalFailed`] rather than aborting the whole batch — a
        /// single stale id in a large cleanup list no longer reverts every
        /// other removal.
        #[pallet::call_index(7)]
        #[pallet::weight(T::WeightInfo::force_remove_many(requests.len() as u32))]
        pub fn force_remove_many(
            origin: OriginFor<T>,
            requests: BoundedVec<RemovalRequest, T::MaxRemovalsPerCall>,
        ) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            for req in requests {
                let outcome = frame_support::storage::with_storage_layer(|| match req.kind {
                    RemovalKind::Refund => Self::do_force_remove_refund(req.id),
                    RemovalKind::Slash => Self::do_force_remove_slash(req.id),
                });
                if outcome.is_err() {
                    // The failing entry rolled back inside its own layer; emit
                    // the skip in the surviving outer context and carry on.
                    Self::deposit_event(Event::RemovalFailed {
                        id: req.id,
                        kind: req.kind,
                    });
                }
            }
            Ok(())
        }

        /// Sponsored cancellation. The owner authorises a `remove_own`-style
        /// refund off-chain via a signed [`RemoveOnBehalfPayload`], and any
        /// `ProviderOrigin` (typically a relayer paying the fees) submits
        /// the extrinsic on the owner's behalf. Closes the
        /// **meta-transaction** loop opened by `deposit_on_behalf` /
        /// `update_on_behalf`: a sponsored owner can retract their record
        /// without ever holding native tokens.
        ///
        /// The settlement is identical to [`Pallet::remove_own`] — each
        /// layer's `base` returns to its payer and each layer's premium
        /// flows to the Treasury. Authority sits with the off-chain
        /// signature; the on-chain `caller` does not need to match either
        /// the sponsor or the owner. Pinning `operator` inside the signed
        /// payload binds the signature to a specific submitter so a
        /// captured payload cannot be re-targeted.
        #[pallet::call_index(10)]
        #[pallet::weight(T::WeightInfo::remove_own_on_behalf())]
        #[frame_support::transactional]
        pub fn remove_own_on_behalf(
            origin: OriginFor<T>,
            id: MiddsId,
            nonce: u64,
            valid_until: BlockNumberFor<T>,
            signature: T::OffchainSignature,
        ) -> DispatchResult {
            let operator = T::ProviderOrigin::ensure_origin(origin)?;

            Self::ensure_signature_fresh(valid_until)?;
            let info = DepositInfo::<T, I>::get(id).ok_or(Error::<T, I>::MiddsNotFound)?;
            ensure!(!info.finalized, Error::<T, I>::AlreadyFinalized);
            Self::ensure_in_window(&info)?;

            let owner = info.depositor.clone();
            Self::consume_on_behalf_authorization(&owner, nonce, &signature, || {
                RemoveOnBehalfPayload {
                    kind: Self::kind_bytes(),
                    genesis_hash: Self::genesis_hash(),
                    action: OnBehalfAction::Remove,
                    id,
                    operator,
                    nonce,
                    valid_until,
                }
            })?;

            Self::do_remove_own(id, info)
        }

        /// `ForceOrigin` updates the flat part of the unmultiplied bond
        /// formula. Per `docs/economics.md` §13.4 this lives in storage
        /// rather than as a compile-time `Config` constant, so the Foundation
        /// can recalibrate the bond floor without a runtime upgrade — useful
        /// when the AFT/USD ratio drifts. Existing layers keep their banked
        /// base (anti-arbitrage §5.5); the new value applies to subsequent
        /// deposits and update-time extensions.
        #[pallet::call_index(11)]
        #[pallet::weight(T::WeightInfo::force_set_deposit_base())]
        pub fn force_set_deposit_base(
            origin: OriginFor<T>,
            new: BalanceOf<T, I>,
        ) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            DepositBase::<T, I>::put(new);
            Self::deposit_event(Event::DepositBaseSet { new });
            Ok(())
        }

        /// `ForceOrigin` updates the per-byte multiplier of the unmultiplied
        /// bond formula. Sibling of [`Pallet::force_set_deposit_base`]; same
        /// governance rationale and same per-layer stickiness on existing
        /// records.
        #[pallet::call_index(12)]
        #[pallet::weight(T::WeightInfo::force_set_deposit_per_byte())]
        pub fn force_set_deposit_per_byte(
            origin: OriginFor<T>,
            new: BalanceOf<T, I>,
        ) -> DispatchResult {
            T::ForceOrigin::ensure_origin(origin)?;
            DepositPerByte::<T, I>::put(new);
            Self::deposit_event(Event::DepositPerByteSet { new });
            Ok(())
        }
    }
}
