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
use frame_support::BoundedVec;
use frame_support::assert_noop;
use midds_traits::Midds as _;
use proptest::prelude::*;

type Instance = ();

// -----------------------------------------------------------------------------
// Strategies
// -----------------------------------------------------------------------------

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

// -----------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------

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
            // Update may legitimately fail (e.g. the new payload byte-collides
            // with an existing record). We just need the identifier and
            // counter invariant to hold either way.
            let _ = Midds::update(RuntimeOrigin::signed(ALICE), 0, updated);

            let stored_id = pallet_midds::Items::<Test, Instance>::get(0)
                .map(|m| m.identifier())
                .expect("item present after update");
            prop_assert_eq!(stored_id, initial.identifier());
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
            // Mutate the data so the new payload doesn't equal the original
            // (which would be a no-op identical update).
            let mut other = item.clone();
            // flip a byte; if data is empty, push one (Bounded allows ≤ 32)
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
                    bond: info.amount,
                    base_bond: info.base_bond,
                },
            ));
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
}
