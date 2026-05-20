//! Property-based tests for `pallet-midds` over the mock runtime — Couche 2 of
//! `docs/testing.md`.
//!
//! Each invariant gets one `proptest!` block. The default budget is 256
//! cases per property; export `PROPTEST_CASES=<n>` to override (nightly CI
//! runs at 10000 per the plan).
//!
//! Strategies live in this module rather than in `mock.rs` because they are
//! consumed only here — the planned mass-injection layer (Couche 3) uses the
//! deterministic `midds-fixtures::gen_n` instead of proptest.

use crate as pallet_midds;
use crate::mock::{test_helpers::*, *};
use alloc::collections::BTreeMap;
use frame_support::BoundedVec;
use frame_support::assert_noop;
use frame_support::traits::Get;
use frame_support::traits::fungible::InspectHold;
use midds_traits::{Midds as _, MiddsId};
use proptest::prelude::*;
use sp_runtime::FixedU128;

extern crate alloc;

type Instance = ();

fn arb_mock_id() -> impl Strategy<Value = MockId> {
    proptest::collection::vec(arb_alphanumeric_byte(), 1..=8)
        .prop_map(|v| BoundedVec::try_from(v).expect("len ≤ 8 by construction"))
}

fn arb_mock_payload() -> impl Strategy<Value = MockPayload> {
    proptest::collection::vec(any::<u8>(), 0..=32)
        .prop_map(|v| BoundedVec::try_from(v).expect("len ≤ 32 by construction"))
}

fn arb_alphanumeric_byte() -> impl Strategy<Value = u8> {
    prop_oneof![b'0'..=b'9', b'a'..=b'z', b'A'..=b'Z']
}

fn arb_mock_midds() -> impl Strategy<Value = MockMidds> {
    (arb_mock_id(), arb_mock_payload()).prop_map(|(id, data)| MockMidds { id, data })
}

/// Strategy producing payloads that always fail [`MockMidds::validate_format`]:
/// either an empty `id` (`EmptyMandatoryField`) or an `id` containing
/// non-alphanumeric bytes (`InvalidCharset`). Used to drive negative-path
/// invariant checks where every `deposit` must bounce with `InvalidFormat`.
fn arb_invalid_mock_midds() -> impl Strategy<Value = MockMidds> {
    let empty_id = arb_mock_payload().prop_map(|data| MockMidds {
        id: BoundedVec::default(),
        data,
    });
    let bad_charset = (
        proptest::collection::vec(
            prop_oneof![Just(b'!'), Just(b'@'), Just(b'#'), Just(b' '), Just(b'\t')],
            1..=8,
        ),
        arb_mock_payload(),
    )
        .prop_map(|(id, data)| MockMidds {
            id: BoundedVec::try_from(id).expect("len ≤ 8"),
            data,
        });
    prop_oneof![empty_id, bad_charset]
}

/// One step in the random extrinsic sequence driving
/// [`invariants_hold_across_arbitrary_sequences`]. The numeric handle on
/// "id-targeting" actions is interpreted modulo the count of live records;
/// when no record exists the action is a no-op (no extrinsic dispatched).
#[derive(Debug, Clone)]
enum Action {
    Deposit { item: MockMidds, signer: u8 },
    Update { id_pick: u32, new_data: MockPayload },
    RemoveOwn { id_pick: u32 },
    Finalize { id_pick: u32 },
    ForceRemoveRefund { id_pick: u32 },
    ForceRemoveSlash { id_pick: u32 },
    Advance { blocks: u32 },
}

fn arb_action() -> impl Strategy<Value = Action> {
    prop_oneof![
        4 => (arb_mock_midds(), 0u8..=2u8).prop_map(|(item, signer)| Action::Deposit { item, signer }),
        2 => (any::<u32>(), arb_mock_payload())
            .prop_map(|(id_pick, new_data)| Action::Update { id_pick, new_data }),
        1 => any::<u32>().prop_map(|id_pick| Action::RemoveOwn { id_pick }),
        1 => any::<u32>().prop_map(|id_pick| Action::Finalize { id_pick }),
        1 => any::<u32>().prop_map(|id_pick| Action::ForceRemoveRefund { id_pick }),
        1 => any::<u32>().prop_map(|id_pick| Action::ForceRemoveSlash { id_pick }),
        2 => (1u32..=(COMMITMENT_WINDOW as u32 / 4))
            .prop_map(|blocks| Action::Advance { blocks }),
    ]
}

fn signer_for(s: u8) -> AccountId {
    const SIGNERS: [AccountId; 3] = [ALICE, BOB, CHARLIE];
    SIGNERS[(s as usize) % SIGNERS.len()]
}

fn pick_id(live: &[MiddsId], pick: u32) -> Option<MiddsId> {
    if live.is_empty() {
        None
    } else {
        Some(live[pick as usize % live.len()])
    }
}

fn apply_action(action: &Action, live: &[MiddsId]) {
    match action {
        Action::Deposit { item, signer } => {
            let _ = Midds::deposit(RuntimeOrigin::signed(signer_for(*signer)), item.clone());
        }
        Action::Update { id_pick, new_data } => {
            if let Some(id) = pick_id(live, *id_pick) {
                if let Some(item) = pallet_midds::Items::<Test, Instance>::get(id) {
                    let info = pallet_midds::DepositInfo::<Test, Instance>::get(id);
                    let updated = MockMidds {
                        id: item.id.clone(),
                        data: new_data.clone(),
                    };
                    if let Some(info) = info {
                        let _ = Midds::update(RuntimeOrigin::signed(info.depositor), id, updated);
                    }
                }
            }
        }
        Action::RemoveOwn { id_pick } => {
            if let Some(id) = pick_id(live, *id_pick) {
                if let Some(info) = pallet_midds::DepositInfo::<Test, Instance>::get(id) {
                    let _ = Midds::remove_own(RuntimeOrigin::signed(info.depositor), id);
                }
            }
        }
        Action::Finalize { id_pick } => {
            if let Some(id) = pick_id(live, *id_pick) {
                let _ = Midds::finalize(RuntimeOrigin::signed(ALICE), id);
            }
        }
        Action::ForceRemoveRefund { id_pick } => {
            if let Some(id) = pick_id(live, *id_pick) {
                let _ = Midds::force_remove_refund(RuntimeOrigin::root(), id);
            }
        }
        Action::ForceRemoveSlash { id_pick } => {
            if let Some(id) = pick_id(live, *id_pick) {
                let _ = Midds::force_remove_slash(RuntimeOrigin::root(), id);
            }
        }
        Action::Advance { blocks } => {
            advance_to_block(System::block_number() + *blocks as BlockNumber);
        }
    }
}

/// Aggregate of the invariants checked after every step of the random
/// extrinsic sequence. Each field maps directly to a bullet in
/// `docs/plan.md` §7.
struct InvariantSnapshot {
    /// `held(account)` for every account observed.
    holds: BTreeMap<AccountId, Balance>,
    /// `(id, identifier)` pairs at the moment of the snapshot.
    identifiers: BTreeMap<MiddsId, MockId>,
    /// `NextMiddsId` storage.
    next_id: MiddsId,
}

fn snapshot() -> InvariantSnapshot {
    let mut holds = BTreeMap::new();
    let mut identifiers = BTreeMap::new();
    for (id, item) in pallet_midds::Items::<Test, Instance>::iter() {
        identifiers.insert(id, item.id.clone());
    }
    let depositors: alloc::collections::BTreeSet<AccountId> =
        pallet_midds::DepositInfo::<Test, Instance>::iter()
            .map(|(_, info)| info.depositor)
            .collect();
    for account in depositors {
        let h = <Balances as InspectHold<AccountId>>::balance_on_hold(&deposit_reason(), &account);
        if h > 0 {
            holds.insert(account, h);
        }
    }
    InvariantSnapshot {
        holds,
        identifiers,
        next_id: pallet_midds::NextMiddsId::<Test, Instance>::get(),
    }
}

fn proptest_config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256);
    ProptestConfig {
        cases,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(proptest_config())]

    /// Held bond after a fresh deposit equals the unmultiplied base — the
    /// genesis multipliers are 1.0× (verified by the equality `total ==
    /// base`).
    #[test]
    fn bond_formula_matches_held_balance(item in arb_mock_midds()) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            let expected = expected_base_bond_for(&item);
            Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone())
                .map_err(|e| TestCaseError::fail(format!("deposit failed: {e:?}")))?;
            prop_assert_eq!(held(ALICE), expected);
            Ok(())
        })?;
    }

    /// `force_remove_refund` returns the depositor's free balance to its
    /// pre-deposit value and leaves zero held bond, regardless of premium.
    #[test]
    fn force_remove_refund_releases_full_bond(item in arb_mock_midds()) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            let initial_free = free(ALICE);
            Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone())
                .map_err(|e| TestCaseError::fail(format!("deposit failed: {e:?}")))?;
            Midds::force_remove_refund(RuntimeOrigin::root(), 0)
                .map_err(|e| TestCaseError::fail(format!("force_remove_refund failed: {e:?}")))?;
            prop_assert_eq!(held(ALICE), 0);
            prop_assert_eq!(free(ALICE), initial_free);
            Ok(())
        })?;
    }

    /// `update` keeps the canonical identifier intact and never bumps
    /// `NextMiddsId`.
    #[test]
    fn update_preserves_identifier_and_counter(
        initial in arb_mock_midds(),
        new_data in arb_mock_payload(),
    ) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            Midds::deposit(RuntimeOrigin::signed(ALICE), initial.clone())
                .map_err(|e| TestCaseError::fail(format!("deposit failed: {e:?}")))?;
            let updated = MockMidds { id: initial.id.clone(), data: new_data };
            let _ = Midds::update(RuntimeOrigin::signed(ALICE), 0, updated);

            let stored_id = pallet_midds::Items::<Test, Instance>::get(0)
                .map(|m| m.identifier().clone())
                .expect("item present after update");
            prop_assert_eq!(stored_id, initial.identifier().clone());
            prop_assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 1);
            Ok(())
        })?;
    }

    /// Any `update` strictly after `deposited_at + CommitmentWindow` is
    /// rejected with `CommitmentWindowClosed`, and storage is left untouched.
    #[test]
    fn update_after_window_is_rejected(
        item in arb_mock_midds(),
        offset in (COMMITMENT_WINDOW + 1)..=(COMMITMENT_WINDOW * 16),
    ) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone())
                .map_err(|e| TestCaseError::fail(format!("deposit failed: {e:?}")))?;
            System::set_block_number(System::block_number() + offset);
            let mut other = item.clone();
            if other.data.is_empty() {
                let mut v: Vec<u8> = other.data.into_inner();
                v.push(1);
                other.data = BoundedVec::try_from(v).expect("len ≤ 32");
            } else {
                let bytes_mut = other.data.as_mut();
                bytes_mut[0] ^= 0xFF;
            }
            assert_noop!(
                Midds::update(RuntimeOrigin::signed(ALICE), 0, other),
                pallet_midds::Error::<Test, Instance>::CommitmentWindowClosed,
            );
            Ok(())
        })?;
    }

    /// `IdentifierClaims` and `Items` share cardinality, and every reverse
    /// lookup contains the live record's id (multi-claim version of the old
    /// uniqueness invariant — duplicates on identifier are now legal).
    #[test]
    fn identifier_claims_stays_consistent_under_random_deposits(
        items in proptest::collection::vec(arb_mock_midds(), 1..=20),
    ) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            for item in &items {
                let _ = Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone());
            }

            let stored: Vec<_> =
                pallet_midds::Items::<Test, Instance>::iter().collect();
            let claim_count =
                pallet_midds::IdentifierClaims::<Test, Instance>::iter().count();

            prop_assert_eq!(stored.len(), claim_count);

            for (id, item) in &stored {
                prop_assert!(
                    pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(
                        item.identifier(), *id
                    ),
                    "IdentifierClaims missing entry for id {id}"
                );
            }
            Ok(())
        })?;
    }

    /// A successful `deposit` emits a `Deposited` event whose fields exactly
    /// mirror what landed in storage (id, depositor, total bond, base bond).
    #[test]
    fn deposit_emits_event_matching_storage(item in arb_mock_midds()) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone())
                .map_err(|e| TestCaseError::fail(format!("deposit failed: {e:?}")))?;

            let id = pallet_midds::NextMiddsId::<Test, Instance>::get() - 1;
            let info = pallet_midds::DepositInfo::<Test, Instance>::get(id)
                .expect("deposit info present");

            System::assert_has_event(RuntimeEvent::Midds(
                pallet_midds::Event::Deposited {
                    id,
                    depositor: ALICE,
                    bond_payer: ALICE,
                    bond: info.sponsor_layer.amount,
                    base_bond: info.sponsor_layer.base,
                },
            ));
            Ok(())
        })?;
    }

    /// `Refunded.sponsor_refund` reports `min(amount, base)` and the per-event
    /// accounting balances (`sponsor_refund + premium_to_treasury == amount
    /// originally held`) across the full `M_fast` range — `M < 1`, `M = 1`,
    /// `M > 1`. Pinned against the M < 1 regression where the event emitted
    /// raw `base` and over-stated the on-chain balance movement.
    #[test]
    fn refunded_event_matches_net_movement_under_arbitrary_multiplier(
        item in arb_mock_midds(),
        m_num in 1u128..=20,
        m_den in 1u128..=20,
    ) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            let m_min = <Test as pallet_midds::Config>::FastMultiplierMin::get();
            let m_max = <Test as pallet_midds::Config>::FastMultiplierMax::get();
            let m = FixedU128::from_rational(m_num, m_den).max(m_min).min(m_max);
            pallet_midds::FastMultiplier::<Test, Instance>::put(m);

            Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone())
                .map_err(|e| TestCaseError::fail(format!("deposit failed: {e:?}")))?;

            let info = pallet_midds::DepositInfo::<Test, Instance>::get(0)
                .expect("deposit info present");
            let amount = info.sponsor_layer.amount;
            let base = info.sponsor_layer.base;

            Midds::remove_own(RuntimeOrigin::signed(ALICE), 0)
                .map_err(|e| TestCaseError::fail(format!("remove_own failed: {e:?}")))?;

            let refunded = System::events().into_iter().rev().find_map(|er| {
                if let RuntimeEvent::Midds(pallet_midds::Event::Refunded {
                    sponsor_refund,
                    premium_to_treasury,
                    ..
                }) = er.event
                {
                    Some((sponsor_refund, premium_to_treasury))
                } else {
                    None
                }
            });
            let (refund, premium) = refunded
                .ok_or_else(|| TestCaseError::fail("Refunded event not emitted"))?;

            prop_assert_eq!(refund, amount.min(base));
            prop_assert_eq!(premium, amount.saturating_sub(base));
            prop_assert_eq!(refund.saturating_add(premium), amount);
            Ok(())
        })?;
    }

    /// `PayloadHashes` rejects byte-identical re-deposits — multi-claim only
    /// authorises *different* payloads sharing an identifier.
    #[test]
    fn duplicate_exact_payload_is_rejected(item in arb_mock_midds()) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone())
                .map_err(|e| TestCaseError::fail(format!("first deposit: {e:?}")))?;
            assert_noop!(
                Midds::deposit(RuntimeOrigin::signed(BOB), item),
                pallet_midds::Error::<Test, Instance>::DuplicatePayload,
            );
            Ok(())
        })?;
    }

    /// Drive an arbitrary sequence of extrinsics over the mock runtime and
    /// assert the five `docs/plan.md` §7 invariants after every step:
    ///
    /// 1. Bond accounting — `held(account) == Σ DepositInfo[id].amount`
    ///    over non-finalized records owned by `account`.
    /// 2. `Items` ↔ `DepositInfo` cardinality coupling.
    /// 3. `PayloadHashes` cardinality matches `Items` (every record has a
    ///    payload-hash entry, neither orphans nor extras).
    /// 4. Identifier immutability — once a record is born under
    ///    identifier I, it stays under I until it is removed.
    /// 5. `NextMiddsId` is monotonically non-decreasing.
    #[test]
    fn invariants_hold_across_arbitrary_sequences(
        actions in proptest::collection::vec(arb_action(), 1..=40),
    ) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            let mut prev = snapshot();
            for action in &actions {
                let live: Vec<MiddsId> = prev.identifiers.keys().copied().collect();
                apply_action(action, &live);

                let now = snapshot();

                prop_assert!(
                    now.next_id >= prev.next_id,
                    "NextMiddsId regressed: {} → {}",
                    prev.next_id,
                    now.next_id,
                );

                let items_count = pallet_midds::Items::<Test, Instance>::iter().count();
                let deposit_info_count =
                    pallet_midds::DepositInfo::<Test, Instance>::iter().count();
                let hash_count =
                    pallet_midds::PayloadHashes::<Test, Instance>::iter().count();
                prop_assert_eq!(items_count, deposit_info_count, "Items↔DepositInfo drift");
                prop_assert_eq!(items_count, hash_count, "Items↔PayloadHashes drift");

                for (id, prev_ident) in &prev.identifiers {
                    if let Some(curr_ident) = now.identifiers.get(id) {
                        prop_assert_eq!(prev_ident, curr_ident, "identifier of id {} mutated", id);
                    }
                }

                let mut expected: BTreeMap<AccountId, Balance> = BTreeMap::new();
                for (_id, info) in pallet_midds::DepositInfo::<Test, Instance>::iter() {
                    if info.finalized {
                        continue;
                    }
                    if info.sponsor_layer.amount > 0 {
                        *expected.entry(info.sponsor_layer.payer).or_insert(0) +=
                            info.sponsor_layer.amount;
                    }
                    if let Some(layer) = info.owner_layer.as_ref() {
                        if layer.amount > 0 {
                            *expected.entry(layer.payer).or_insert(0) += layer.amount;
                        }
                    }
                }
                prop_assert_eq!(
                    &now.holds, &expected,
                    "held balance vs Σ layer.amount mismatch",
                );

                prev = now;
            }
            Ok(())
        })?;
    }

    /// Every payload that fails `validate_format` is rejected with
    /// `InvalidFormat` and leaves storage untouched. Pins the
    /// pre-uniqueness pre-flight check on the deposit path.
    #[test]
    fn invalid_payloads_bounce_with_invalid_format(item in arb_invalid_mock_midds()) {
        new_test_ext().execute_with(|| -> Result<(), TestCaseError> {
            assert_noop!(
                Midds::deposit(RuntimeOrigin::signed(ALICE), item),
                pallet_midds::Error::<Test, Instance>::InvalidFormat,
            );
            prop_assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 0);
            Ok(())
        })?;
    }
}
