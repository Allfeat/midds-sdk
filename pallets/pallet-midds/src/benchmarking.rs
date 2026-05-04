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
    DepositOnBehalfPayload, OnBehalfAction, RemovalKind, RemovalRequest, UpdateOnBehalfPayload,
};
use parity_scale_codec::Encode as _;

use alloc::vec;
use frame_benchmarking::v2::*;
use frame_support::{
    BoundedVec,
    traits::{Get, fungible::Mutate},
};
use frame_system::RawOrigin;
use midds_traits::Midds as MiddsTrait;
use sp_runtime::traits::Bounded;

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
    let endowment = BalanceOf::<T, I>::max_value() / 2u32.into();
    let _ = T::Currency::set_balance(account, endowment);
}

/// Move the chain past the commitment window so finalization-eligible
/// benchmarks can land their extrinsic at a valid block height.
fn warp_past_window<T: Config<I>, I: 'static>() {
    let now = frame_system::Pallet::<T>::block_number();
    let target = now + T::CommitmentWindow::get() + 1u32.into();
    frame_system::Pallet::<T>::set_block_number(target);
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
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);

        let item = T::BenchmarkHelper::bench_instance(0);
        MiddsPallet::<T, I>::deposit(RawOrigin::Signed(caller.clone()).into(), item)?;

        #[extrinsic_call]
        _(RawOrigin::Signed(caller), 0);

        assert!(!Items::<T, I>::contains_key(0));
        Ok(())
    }

    #[benchmark]
    fn finalize_one() -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);

        let item = T::BenchmarkHelper::bench_instance(0);
        MiddsPallet::<T, I>::deposit(RawOrigin::Signed(caller).into(), item)?;
        warp_past_window::<T, I>();

        // `finalize`'s origin is intentionally permissionless — anyone can
        // crank it. Use the whitelisted caller to keep the cost stable.
        let cranker: T::AccountId = whitelisted_caller();

        #[extrinsic_call]
        finalize(RawOrigin::Signed(cranker), 0);

        let info = DepositInfo::<T, I>::get(0).expect("info kept after finalize");
        assert!(info.finalized);
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
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);

        let item = T::BenchmarkHelper::bench_instance(0);
        let identifier = item.identifier().clone();
        MiddsPallet::<T, I>::deposit(RawOrigin::Signed(caller).into(), item)?;

        #[extrinsic_call]
        _(RawOrigin::Root, 0);

        assert!(!Items::<T, I>::contains_key(0));
        assert!(!IdentifierClaims::<T, I>::contains_key(&identifier, 0));
        assert!(!DepositInfo::<T, I>::contains_key(0));
        Ok(())
    }

    #[benchmark]
    fn force_remove_slash() -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);

        let item = T::BenchmarkHelper::bench_instance(0);
        let payload_hash = <T::Hashing as sp_runtime::traits::Hash>::hash_of(&item);
        MiddsPallet::<T, I>::deposit(RawOrigin::Signed(caller).into(), item)?;

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

        // Derive the owner deterministically through the helper, then build
        // and sign the payload at the current `OnBehalfNonce[owner]` value.
        let dummy = DepositOnBehalfPayload::<T::AccountId, T::Midds> {
            action: OnBehalfAction::Deposit,
            item: item.clone(),
            operator: operator.clone(),
            nonce: 0,
        };
        let (_, owner) = T::BenchmarkHelper::create_signature(b"owner", &dummy.encode());

        let nonce = crate::OnBehalfNonce::<T, I>::get(&owner);
        let payload = DepositOnBehalfPayload::<T::AccountId, T::Midds> {
            action: OnBehalfAction::Deposit,
            item: item.clone(),
            operator: operator.clone(),
            nonce,
        };
        let (sig, _) = T::BenchmarkHelper::create_signature(b"owner", &payload.encode());

        #[extrinsic_call]
        _(RawOrigin::Signed(operator), owner, item, nonce, sig);

        assert!(Items::<T, I>::contains_key(0));
        Ok(())
    }

    #[benchmark]
    fn update_on_behalf(s: Linear<0, 1024>) -> Result<(), BenchmarkError> {
        let operator: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&operator);

        let initial = T::BenchmarkHelper::bench_instance(0);
        let dummy = DepositOnBehalfPayload::<T::AccountId, T::Midds> {
            action: OnBehalfAction::Deposit,
            item: initial.clone(),
            operator: operator.clone(),
            nonce: 0,
        };
        let (_, owner) = T::BenchmarkHelper::create_signature(b"owner", &dummy.encode());

        let nonce0 = crate::OnBehalfNonce::<T, I>::get(&owner);
        let dep_payload = DepositOnBehalfPayload::<T::AccountId, T::Midds> {
            action: OnBehalfAction::Deposit,
            item: initial.clone(),
            operator: operator.clone(),
            nonce: nonce0,
        };
        let (dep_sig, _) = T::BenchmarkHelper::create_signature(b"owner", &dep_payload.encode());
        MiddsPallet::<T, I>::deposit_on_behalf(
            RawOrigin::Signed(operator.clone()).into(),
            owner.clone(),
            initial,
            nonce0,
            dep_sig,
        )?;

        let updated = T::BenchmarkHelper::bench_instance(s);
        let nonce1 = crate::OnBehalfNonce::<T, I>::get(&owner);
        let upd_payload = UpdateOnBehalfPayload::<T::AccountId, T::Midds> {
            action: OnBehalfAction::Update,
            id: 0,
            item: updated.clone(),
            operator: operator.clone(),
            nonce: nonce1,
        };
        let (upd_sig, _) = T::BenchmarkHelper::create_signature(b"owner", &upd_payload.encode());

        #[extrinsic_call]
        _(RawOrigin::Signed(operator), 0, updated, nonce1, upd_sig);

        Ok(())
    }

    #[benchmark]
    fn force_remove_many(n: Linear<1, 64>) -> Result<(), BenchmarkError> {
        let caller: T::AccountId = whitelisted_caller();
        fund_caller::<T, I>(&caller);

        // Each iteration produces a fresh worst-case payload — the helper
        // contract is that distinct `size` values yield distinct payloads
        // (otherwise the per-payload hash guard refuses the second deposit).
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

    impl_benchmark_test_suite!(MiddsPallet, crate::mock::new_test_ext(), crate::mock::Test);
}
