//! Benchmarks for `pallet-midds`.
//!
//! Covers each call exposed by the pallet plus the per-record cost of the
//! `on_initialize` finalization sweep. Worst-case scenarios mirror the on-
//! chain logic: fresh-state deposit, growing update with hold delta, force-
//! edit bypassing the window, refund (depositor path), finalize (sweep
//! path), and the two `force_remove_*` variants.
//!
//! The integrating runtime supplies a [`BenchmarkHelper`] producing
//! representative `T::Midds` payloads at the requested size.

#![cfg(feature = "runtime-benchmarks")]

use super::*;
use crate::Pallet as MiddsPallet;
use crate::types::{
    DepositOnBehalfPayload, OnBehalfAction, RemovalKind, RemovalRequest, RemoveOnBehalfPayload,
    UpdateOnBehalfPayload,
};
use parity_scale_codec::Encode as _;

use alloc::vec;
// `frame::benchmarking::prelude` re-exports the v2 benchmarking macros, the
// main `frame::prelude` (`BoundedVec`, `Get`, `BlockNumberFor`, `Bounded`, …)
// and `RawOrigin`; only the fungible `Mutate` trait is imported separately.
use frame::benchmarking::prelude::*;
use frame::token::fungible::Mutate;
use midds_traits::Midds as MiddsTrait;

/// Per-instance hook supplying worst-case [`Config::Midds`] payloads and
/// owner signatures used by the on-behalf benchmarks. Mirrors the pattern
/// from `pallet-ats`'s benchmark helper.
pub trait BenchmarkHelper<M, Signature, AccountId> {
    /// Build a payload whose encoded representation is approximately `size` bytes.
    fn bench_instance(size: u32) -> M;
    /// Produce a `(signature, signer_account)` pair valid against `msg`.
    /// `entropy` lets the helper derive distinct signers across calls.
    fn create_signature(entropy: &[u8], msg: &[u8]) -> (Signature, AccountId);
}

fn fund_caller<T: Config<I>, I: 'static>(account: &T::AccountId) {
    // `max_value() / 16` — astronomically more than any bond, yet small
    // enough that funding several accounts (e.g. the operator *and* owner of
    // a two-layer record) never overflows `TotalIssuance`. With `max_value()
    // / 2`, the second `set_balance` would overflow issuance and the fungible
    // default silently leaves that account at zero, so its later hold fails.
    let endowment = BalanceOf::<T, I>::max_value() / 16u32.into();
    let _ = T::Currency::set_balance(account, endowment);
}

/// Move the chain past the commitment window so finalization-eligible
/// benchmarks can land their extrinsic at a valid block height.
fn warp_past_window<T: Config<I>, I: 'static>() {
    let now = frame_system::Pallet::<T>::block_number();
    let target = now + T::CommitmentWindow::get() + 1u32.into();
    frame_system::Pallet::<T>::set_block_number(target);
}

/// Build a record at id 0 that carries **both** a sponsor layer and an owner
/// layer — the worst case for every settlement path (`finalize_one`,
/// `remove_own`, `force_remove_*`), which must then release/transfer two
/// holds, not one. The operator sponsors the deposit via `deposit_on_behalf`
/// and the owner extends it with a plain `update` (the web3 escape hatch),
/// materialising the owner layer. Returns the owner account so the caller can
/// drive depositor-authenticated extrinsics. Both operator and owner are
/// funded.
fn setup_two_layer_record<T: Config<I>, I: 'static>() -> T::AccountId {
    let operator: T::AccountId = whitelisted_caller();
    fund_caller::<T, I>(&operator);

    let valid_until = BlockNumberFor::<T>::max_value();
    let initial = T::BenchmarkHelper::bench_instance(0);
    let dummy = DepositOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
        kind: MiddsPallet::<T, I>::kind_bytes(),
        genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
        action: OnBehalfAction::Deposit,
        item: initial.clone(),
        operator: operator.clone(),
        nonce: 0,
        valid_until,
    };
    let (_, owner) = T::BenchmarkHelper::create_signature(b"owner", &dummy.encode());

    let nonce0 = crate::OnBehalfNonce::<T, I>::get(&owner);
    let dep_payload = DepositOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
        kind: MiddsPallet::<T, I>::kind_bytes(),
        genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
        action: OnBehalfAction::Deposit,
        item: initial.clone(),
        operator: operator.clone(),
        nonce: nonce0,
        valid_until,
    };
    let (dep_sig, _) = T::BenchmarkHelper::create_signature(b"owner", &dep_payload.encode());
    MiddsPallet::<T, I>::deposit_on_behalf(
        RawOrigin::Signed(operator).into(),
        owner.clone(),
        initial,
        nonce0,
        valid_until,
        dep_sig,
    )
    .expect("deposit_on_behalf in two-layer setup");

    // Owner extends the sponsored record → materialises the owner layer.
    fund_caller::<T, I>(&owner);
    let extended = T::BenchmarkHelper::bench_instance(64);
    MiddsPallet::<T, I>::update(RawOrigin::Signed(owner.clone()).into(), 0, extended)
        .expect("owner update materialises the owner layer");

    owner
}

#[instance_benchmarks]
#[allow(
    clippy::disallowed_methods,
    reason = "benchmarks legitimately unwrap on prepared inputs"
)]
mod benchmarks {
    use super::*;
    use crate::{DepositInfo, IdentifierClaims, Items, NextMiddsId, PayloadHashes};

    #[benchmark]
    fn deposit(s: Linear<0, 1024>) {
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);
        let item = T::BenchmarkHelper::bench_instance(s);

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), item);

        assert!(Items::<T, I>::contains_key(0));
        assert_eq!(NextMiddsId::<T, I>::get(), 1);
    }

    #[benchmark]
    fn update(s: Linear<0, 1024>) -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);

        let initial = T::BenchmarkHelper::bench_instance(0);
        MiddsPallet::<T, I>::deposit(RawOrigin::Signed(caller.clone()).into(), initial)?;

        let updated = T::BenchmarkHelper::bench_instance(s);

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), 0, updated);

        Ok(())
    }

    #[benchmark]
    fn remove_own() -> Result<(), BenchmarkError> {
        // Worst case: a two-layer record removed within the window, so both
        // layers settle (each premium to Treasury, each base refunded).
        let owner = setup_two_layer_record::<T, I>();

        #[extrinsic_call]
        _(RawOrigin::Signed(owner), 0);

        assert!(!Items::<T, I>::contains_key(0));
        Ok(())
    }

    #[benchmark]
    fn finalize_one() -> Result<(), BenchmarkError> {
        // Worst case: a two-layer record, so the finalization sweep settles
        // two holds to the Treasury — the per-record cost `on_initialize`
        // multiplies by `MaxFinalizationsPerBlock`.
        let _owner = setup_two_layer_record::<T, I>();
        warp_past_window::<T, I>();

        let cranker: T::AccountId = whitelisted_caller();

        #[extrinsic_call]
        finalize(RawOrigin::Signed(cranker), 0);

        let info = DepositInfo::<T, I>::get(0).expect("info kept after finalize");
        assert!(info.finalized);
        assert!(info.owner_layer.is_some(), "two-layer worst case");
        Ok(())
    }

    #[benchmark]
    fn force_edit(s: Linear<0, 1024>) -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);

        let initial = T::BenchmarkHelper::bench_instance(0);
        MiddsPallet::<T, I>::deposit(RawOrigin::Signed(caller).into(), initial)?;

        let edited = T::BenchmarkHelper::bench_instance(s);

        #[extrinsic_call]
        _(RawOrigin::Root, 0, edited);

        Ok(())
    }

    #[benchmark]
    fn force_remove_refund() -> Result<(), BenchmarkError> {
        // Worst case: a two-layer record refunded pre-finalization (both
        // layers released back to their respective payers).
        let _owner = setup_two_layer_record::<T, I>();
        let identifier = T::BenchmarkHelper::bench_instance(0).identifier().clone();

        #[extrinsic_call]
        _(RawOrigin::Root, 0);

        assert!(!Items::<T, I>::contains_key(0));
        assert!(!IdentifierClaims::<T, I>::contains_key(&identifier, 0));
        assert!(!DepositInfo::<T, I>::contains_key(0));
        Ok(())
    }

    #[benchmark]
    fn force_remove_slash() -> Result<(), BenchmarkError> {
        // Worst case: a two-layer record slashed pre-finalization, so both
        // layers transfer their full hold to the Treasury. The stored payload
        // is the owner-extended one (`bench_instance(64)`).
        let _owner = setup_two_layer_record::<T, I>();
        let payload_hash = <T::Hashing as Hash>::hash_of(&T::BenchmarkHelper::bench_instance(64));

        #[extrinsic_call]
        _(RawOrigin::Root, 0);

        assert!(!Items::<T, I>::contains_key(0));
        assert!(!PayloadHashes::<T, I>::contains_key(payload_hash));
        Ok(())
    }

    #[benchmark]
    fn deposit_on_behalf(s: Linear<0, 1024>) -> Result<(), BenchmarkError> {
        let operator: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&operator);
        let item = T::BenchmarkHelper::bench_instance(s);

        let valid_until = BlockNumberFor::<T>::max_value();

        let dummy = DepositOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
            kind: MiddsPallet::<T, I>::kind_bytes(),
            genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
            action: OnBehalfAction::Deposit,
            item: item.clone(),
            operator: operator.clone(),
            nonce: 0,
            valid_until,
        };
        let (_, owner) = T::BenchmarkHelper::create_signature(b"owner", &dummy.encode());

        let nonce = crate::OnBehalfNonce::<T, I>::get(&owner);
        let payload = DepositOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
            kind: MiddsPallet::<T, I>::kind_bytes(),
            genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
            action: OnBehalfAction::Deposit,
            item: item.clone(),
            operator: operator.clone(),
            nonce,
            valid_until,
        };
        let (sig, _) = T::BenchmarkHelper::create_signature(b"owner", &payload.encode());

        #[extrinsic_call]
        _(
            RawOrigin::Signed(operator),
            owner,
            item,
            nonce,
            valid_until,
            sig,
        );

        assert!(Items::<T, I>::contains_key(0));
        Ok(())
    }

    #[benchmark]
    fn update_on_behalf(s: Linear<0, 1024>) -> Result<(), BenchmarkError> {
        let operator: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&operator);

        let valid_until = BlockNumberFor::<T>::max_value();
        let initial = T::BenchmarkHelper::bench_instance(0);
        let dummy = DepositOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
            kind: MiddsPallet::<T, I>::kind_bytes(),
            genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
            action: OnBehalfAction::Deposit,
            item: initial.clone(),
            operator: operator.clone(),
            nonce: 0,
            valid_until,
        };
        let (_, owner) = T::BenchmarkHelper::create_signature(b"owner", &dummy.encode());

        let nonce0 = crate::OnBehalfNonce::<T, I>::get(&owner);
        let dep_payload =
            DepositOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
                kind: MiddsPallet::<T, I>::kind_bytes(),
                genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
                action: OnBehalfAction::Deposit,
                item: initial.clone(),
                operator: operator.clone(),
                nonce: nonce0,
                valid_until,
            };
        let (dep_sig, _) = T::BenchmarkHelper::create_signature(b"owner", &dep_payload.encode());
        MiddsPallet::<T, I>::deposit_on_behalf(
            RawOrigin::Signed(operator.clone()).into(),
            owner.clone(),
            initial,
            nonce0,
            valid_until,
            dep_sig,
        )?;

        let updated = T::BenchmarkHelper::bench_instance(s);
        let nonce1 = crate::OnBehalfNonce::<T, I>::get(&owner);
        let upd_payload =
            UpdateOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
                kind: MiddsPallet::<T, I>::kind_bytes(),
                genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
                action: OnBehalfAction::Update,
                id: 0,
                item: updated.clone(),
                operator: operator.clone(),
                nonce: nonce1,
                valid_until,
            };
        let (upd_sig, _) = T::BenchmarkHelper::create_signature(b"owner", &upd_payload.encode());

        #[extrinsic_call]
        _(
            RawOrigin::Signed(operator),
            0,
            updated,
            nonce1,
            valid_until,
            upd_sig,
        );

        Ok(())
    }

    #[benchmark]
    fn remove_own_on_behalf() -> Result<(), BenchmarkError> {
        let operator: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&operator);

        let valid_until = BlockNumberFor::<T>::max_value();
        let initial = T::BenchmarkHelper::bench_instance(0);
        let dummy = DepositOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
            kind: MiddsPallet::<T, I>::kind_bytes(),
            genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
            action: OnBehalfAction::Deposit,
            item: initial.clone(),
            operator: operator.clone(),
            nonce: 0,
            valid_until,
        };
        let (_, owner) = T::BenchmarkHelper::create_signature(b"owner", &dummy.encode());

        let nonce0 = crate::OnBehalfNonce::<T, I>::get(&owner);
        let dep_payload =
            DepositOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash, T::Midds> {
                kind: MiddsPallet::<T, I>::kind_bytes(),
                genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
                action: OnBehalfAction::Deposit,
                item: initial.clone(),
                operator: operator.clone(),
                nonce: nonce0,
                valid_until,
            };
        let (dep_sig, _) = T::BenchmarkHelper::create_signature(b"owner", &dep_payload.encode());
        MiddsPallet::<T, I>::deposit_on_behalf(
            RawOrigin::Signed(operator.clone()).into(),
            owner.clone(),
            initial,
            nonce0,
            valid_until,
            dep_sig,
        )?;

        let relayer: T::AccountId = account("relayer", 0, 0);
        fund_caller::<T, I>(&relayer);

        let nonce1 = crate::OnBehalfNonce::<T, I>::get(&owner);
        let payload = RemoveOnBehalfPayload::<T::AccountId, BlockNumberFor<T>, T::Hash> {
            kind: MiddsPallet::<T, I>::kind_bytes(),
            genesis_hash: MiddsPallet::<T, I>::genesis_hash(),
            action: OnBehalfAction::Remove,
            id: 0,
            operator: relayer.clone(),
            nonce: nonce1,
            valid_until,
        };
        let (sig, _) = T::BenchmarkHelper::create_signature(b"owner", &payload.encode());

        #[extrinsic_call]
        _(RawOrigin::Signed(relayer), 0, nonce1, valid_until, sig);

        assert!(!Items::<T, I>::contains_key(0));
        Ok(())
    }

    #[benchmark]
    fn force_remove_many(n: Linear<1, 64>) -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);

        let mut requests = vec![];
        for i in 0..n {
            let item = T::BenchmarkHelper::bench_instance(i);
            MiddsPallet::<T, I>::deposit(RawOrigin::Signed(caller.clone()).into(), item)?;
            requests.push(RemovalRequest {
                id: i as u64,
                kind: RemovalKind::Slash,
            });
        }
        let requests: BoundedVec<RemovalRequest, T::MaxRemovalsPerCall> = BoundedVec::try_from(
            requests,
        )
        .expect("`n` is bounded by the benchmark Linear range, must fit MaxRemovalsPerCall");

        #[extrinsic_call]
        _(RawOrigin::Root, requests);

        Ok(())
    }

    #[benchmark]
    fn force_set_deposit_base() -> Result<(), BenchmarkError> {
        let new: crate::BalanceOf<T, I> = 1_000u32.into();

        #[extrinsic_call]
        _(RawOrigin::Root, new);

        assert_eq!(crate::DepositBase::<T, I>::get(), new);
        Ok(())
    }

    #[benchmark]
    fn force_set_deposit_per_byte() -> Result<(), BenchmarkError> {
        let new: crate::BalanceOf<T, I> = 7u32.into();

        #[extrinsic_call]
        _(RawOrigin::Root, new);

        assert_eq!(crate::DepositPerByte::<T, I>::get(), new);
        Ok(())
    }

    impl_benchmark_test_suite!(MiddsPallet, crate::mock::new_test_ext(), crate::mock::Test);
}
