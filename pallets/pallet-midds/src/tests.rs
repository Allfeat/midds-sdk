//! Lifecycle tests for `pallet-midds` over the mock runtime.
//!
//! Mirrors the matrix of `docs/economics.md`: deposit (with multipliers),
//! update (bond up/down, identifier immuable, premium preserved),
//! `remove_own` (base refund + premium to Treasury), `finalize` (eager via
//! hook + permissionless catch-up), the two `force_remove_*` variants, and
//! batch removal.

use crate as pallet_midds;
use crate::mock::{test_helpers::*, *};
use frame_support::{assert_noop, assert_ok};
use midds_traits::Midds as _;
use sp_runtime::{FixedU128, traits::One};

type Instance = ();

// -----------------------------------------------------------------------------
// deposit
// -----------------------------------------------------------------------------

#[test]
fn deposit_works() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let identifier = item.identifier();
        let base = expected_base_bond_for(&item);
        let bond = expected_total_bond_for(&item);

        // At genesis multipliers default to 1.0× so total = base — the test
        // also pins this assumption explicitly so a future change to the
        // default multipliers fails loudly here.
        assert_eq!(bond, base);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone()));

        assert_eq!(pallet_midds::Items::<Test, Instance>::get(0), Some(item));
        assert!(pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(&identifier, 0));
        let info =
            pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("deposit info present");
        assert_eq!(info.depositor, ALICE);
        assert_eq!(info.amount, bond);
        assert_eq!(info.base_bond, base);
        assert_eq!(info.deposited_at, System::block_number());
        assert!(!info.finalized);
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 1);

        // PendingFinalization queues at deposited_at + CommitmentWindow.
        let expected_expiry = info.deposited_at + COMMITMENT_WINDOW;
        assert!(
            pallet_midds::PendingFinalization::<Test, Instance>::contains_key(expected_expiry, 0)
        );

        assert_eq!(held(ALICE), bond);

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Deposited {
            id: 0,
            depositor: ALICE,
            bond,
            base_bond: base,
        }));
    });
}

#[test]
fn deposit_accepts_same_identifier_with_different_payload() {
    // Multi-claim is a feature, not a bug: two different parties may register
    // their own version of the same canonical ISWC.
    new_test_ext().execute_with(|| {
        let first = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), first.clone()));

        let second = mock_midds(b"abc", 7); // same id, different payload
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(BOB), second.clone()));

        assert!(
            pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(first.identifier(), 0)
        );
        assert!(
            pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(second.identifier(), 1)
        );
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 2);
    });
}

#[test]
fn deposit_rejects_exact_duplicate_payload() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone()));

        // Byte-identical payload → DuplicatePayload, even from a different
        // depositor.
        assert_noop!(
            Midds::deposit(RuntimeOrigin::signed(BOB), item),
            pallet_midds::Error::<Test, Instance>::DuplicatePayload,
        );
    });
}

#[test]
fn deposit_rejects_invalid_format() {
    new_test_ext().execute_with(|| {
        let bad = mock_midds(b"", 5);
        assert_noop!(
            Midds::deposit(RuntimeOrigin::signed(ALICE), bad),
            pallet_midds::Error::<Test, Instance>::InvalidFormat,
        );

        let bad = mock_midds(b"!ok", 5);
        assert_noop!(
            Midds::deposit(RuntimeOrigin::signed(ALICE), bad),
            pallet_midds::Error::<Test, Instance>::InvalidFormat,
        );
    });
}

#[test]
fn deposit_holds_correct_bond() {
    new_test_ext().execute_with(|| {
        let small = mock_midds(b"a", 0);
        let big = mock_midds(b"b", 30);

        let small_bond = expected_total_bond_for(&small);
        let big_bond = expected_total_bond_for(&big);
        assert!(big_bond > small_bond);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), small));
        assert_eq!(held(ALICE), small_bond);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(BOB), big));
        assert_eq!(held(BOB), big_bond);
    });
}

#[test]
fn deposit_increments_demand_trackers() {
    new_test_ext().execute_with(|| {
        // No deposit yet → counters at zero.
        assert_eq!(pallet_midds::DepositsThisBlock::<Test, Instance>::get(), 0);
        assert_eq!(Midds::weekly_actual(), 0);

        assert_ok!(Midds::deposit(
            RuntimeOrigin::signed(ALICE),
            mock_midds(b"a", 0)
        ));
        assert_ok!(Midds::deposit(
            RuntimeOrigin::signed(ALICE),
            mock_midds(b"b", 0)
        ));

        assert_eq!(pallet_midds::DepositsThisBlock::<Test, Instance>::get(), 2);
        assert_eq!(Midds::weekly_actual(), 2);
    });
}

// -----------------------------------------------------------------------------
// update
// -----------------------------------------------------------------------------

#[test]
fn update_within_window_works() {
    new_test_ext().execute_with(|| {
        let initial = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), initial));

        let mut updated = mock_midds(b"abc", 5);
        updated.data.iter_mut().for_each(|b| *b = 1);
        let new_base = expected_base_bond_for(&updated);

        System::set_block_number(System::block_number() + COMMITMENT_WINDOW);
        assert_ok!(Midds::update(
            RuntimeOrigin::signed(ALICE),
            0,
            updated.clone()
        ));

        let info = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("deposit info");
        assert_eq!(pallet_midds::Items::<Test, Instance>::get(0), Some(updated));
        assert_eq!(info.base_bond, new_base);
        // Multipliers unchanged across the test → premium = 0, amount = base.
        assert_eq!(info.amount, new_base);

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Updated {
            id: 0,
            new_bond: new_base,
        }));
    });
}

#[test]
fn update_after_window_freezes() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone()));

        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);
        // The same payload is still indexed by hash, so we have to point at a
        // distinct payload to ensure the failure is the window check, not the
        // duplicate-payload guard.
        let other = mock_midds(b"abc", 6);
        assert_noop!(
            Midds::update(RuntimeOrigin::signed(ALICE), 0, other),
            pallet_midds::Error::<Test, Instance>::CommitmentWindowClosed,
        );
    });
}

#[test]
fn update_only_by_depositor() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone()));

        let other = mock_midds(b"abc", 6);
        assert_noop!(
            Midds::update(RuntimeOrigin::signed(BOB), 0, other),
            pallet_midds::Error::<Test, Instance>::NotProvider,
        );
    });
}

#[test]
fn update_keeps_identifier_immutable() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));

        let renamed = mock_midds(b"xyz", 5);
        assert_noop!(
            Midds::update(RuntimeOrigin::signed(ALICE), 0, renamed),
            pallet_midds::Error::<Test, Instance>::IdentifierImmutable,
        );
    });
}

#[test]
fn update_adjusts_bond_up() {
    new_test_ext().execute_with(|| {
        let small = mock_midds(b"abc", 5);
        let initial_total = expected_total_bond_for(&small);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), small));
        let free_after_deposit = free(ALICE);

        let bigger = mock_midds(b"abc", 25);
        let new_base = expected_base_bond_for(&bigger);
        assert!(new_base > initial_total);

        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, bigger));

        assert_eq!(held(ALICE), new_base);
        assert_eq!(free(ALICE), free_after_deposit - (new_base - initial_total));
    });
}

#[test]
fn update_adjusts_bond_down() {
    new_test_ext().execute_with(|| {
        let big = mock_midds(b"abc", 25);
        let initial_total = expected_total_bond_for(&big);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), big));
        let free_after_deposit = free(ALICE);

        let small = mock_midds(b"abc", 5);
        let new_base = expected_base_bond_for(&small);
        assert!(new_base < initial_total);

        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, small));

        assert_eq!(held(ALICE), new_base);
        assert_eq!(free(ALICE), free_after_deposit + (initial_total - new_base));
    });
}

/// Premium banked at deposit time is *not* re-priced on update (cf. spec
/// §5.5). We force a non-trivial multiplier by writing it directly, then
/// verify a same-size update preserves the premium.
#[test]
fn update_preserves_deposit_time_premium() {
    new_test_ext().execute_with(|| {
        // Inflate M_fast to 2.0× so a fresh deposit pays 2 × base.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::from_u32(2));

        let item = mock_midds(b"abc", 5);
        let base = expected_base_bond_for(&item);
        let total = expected_total_bond_for(&item);
        assert_eq!(total, 2 * base);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone()));
        assert_eq!(held(ALICE), total);

        // Drop M_fast back to 1.0× — a fresh deposit *now* would pay base.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::one());

        // Update with a same-size payload (different bytes so the hash moves
        // off the original).
        let mut other = mock_midds(b"abc", 5);
        other.data.iter_mut().for_each(|b| *b = 1);
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, other));

        // The held amount stays at `2 × base` — the premium was banked at
        // deposit time and survives the price drop.
        assert_eq!(held(ALICE), total);
        let info = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        assert_eq!(info.amount, total);
        assert_eq!(info.base_bond, base);
    });
}

#[test]
fn update_rejects_collision_with_other_payload() {
    // Update can change the exact payload, but it can't land on a hash
    // already owned by *another* record.
    new_test_ext().execute_with(|| {
        let a = mock_midds(b"a", 5);
        let b = mock_midds(b"b", 8);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), a.clone()));
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), b.clone()));

        // Try to update record 0 to look exactly like record 1 → rejected.
        assert_noop!(
            Midds::update(RuntimeOrigin::signed(ALICE), 0, b.clone()),
            pallet_midds::Error::<Test, Instance>::IdentifierImmutable,
        );
    });
}

// -----------------------------------------------------------------------------
// remove_own
// -----------------------------------------------------------------------------

#[test]
fn remove_own_refunds_base_and_sends_premium_to_treasury() {
    new_test_ext().execute_with(|| {
        // 2× M_fast so there's a non-zero premium to track.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::from_u32(2));

        let item = mock_midds(b"abc", 5);
        let base = expected_base_bond_for(&item);
        let total = expected_total_bond_for(&item);
        let premium = total - base;
        assert!(premium > 0);

        let free_before = free(ALICE);
        let treasury_before = treasury_balance();

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone()));
        assert_ok!(Midds::remove_own(RuntimeOrigin::signed(ALICE), 0));

        assert_eq!(held(ALICE), 0);
        // Depositor gets `base` back (paid `total`, refunded `total`, then
        // `premium` flowed to Treasury).
        assert_eq!(free(ALICE), free_before - premium);
        assert_eq!(treasury_balance(), treasury_before + premium);

        // Storage cleared.
        assert!(pallet_midds::Items::<Test, Instance>::get(0).is_none());
        assert!(pallet_midds::DepositInfo::<Test, Instance>::get(0).is_none());
        assert!(
            !pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(item.identifier(), 0)
        );

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Refunded {
            id: 0,
            depositor: ALICE,
            refund: base,
            premium_to_treasury: premium,
        }));
    });
}

#[test]
fn remove_own_within_window_with_no_premium_is_full_refund() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let bond = expected_total_bond_for(&item);
        let free_before = free(ALICE);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        assert_ok!(Midds::remove_own(RuntimeOrigin::signed(ALICE), 0));

        assert_eq!(held(ALICE), 0);
        assert_eq!(free(ALICE), free_before);
        assert_eq!(treasury_balance(), 0);
        // Sanity: at unit multipliers, the held amount equalled the base.
        assert_eq!(bond, expected_base_bond_for(&mock_midds(b"abc", 5)));
    });
}

#[test]
fn remove_own_only_by_depositor() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));

        assert_noop!(
            Midds::remove_own(RuntimeOrigin::signed(BOB), 0),
            pallet_midds::Error::<Test, Instance>::NotProvider,
        );
    });
}

#[test]
fn remove_own_after_window_is_rejected() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);

        assert_noop!(
            Midds::remove_own(RuntimeOrigin::signed(ALICE), 0),
            pallet_midds::Error::<Test, Instance>::CommitmentWindowClosed,
        );
    });
}

// -----------------------------------------------------------------------------
// finalize
// -----------------------------------------------------------------------------

#[test]
fn finalize_after_window_moves_bond_to_treasury() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let bond = expected_total_bond_for(&item);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));

        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);
        assert_ok!(Midds::finalize(RuntimeOrigin::signed(BOB), 0));

        // Bond moved.
        assert_eq!(held(ALICE), 0);
        assert_eq!(treasury_balance(), bond);
        // Record stays on chain, marked permanent.
        let info =
            pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("kept after finalize");
        assert!(info.finalized);
        assert!(pallet_midds::Items::<Test, Instance>::get(0).is_some());
        // Pending finalization slot drained.
        let expiry = info.deposited_at + COMMITMENT_WINDOW;
        assert!(!pallet_midds::PendingFinalization::<Test, Instance>::contains_key(expiry, 0));

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Finalized {
            id: 0,
            depositor: ALICE,
            amount_to_treasury: bond,
        }));
    });
}

#[test]
fn finalize_before_window_close_is_rejected() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));

        // Still inside the window.
        assert_noop!(
            Midds::finalize(RuntimeOrigin::signed(BOB), 0),
            pallet_midds::Error::<Test, Instance>::CommitmentWindowOpen,
        );
    });
}

#[test]
fn finalize_is_idempotent() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);

        assert_ok!(Midds::finalize(RuntimeOrigin::signed(BOB), 0));
        assert_noop!(
            Midds::finalize(RuntimeOrigin::signed(BOB), 0),
            pallet_midds::Error::<Test, Instance>::AlreadyFinalized,
        );
    });
}

/// Eager finalization through the runtime hook — exercises the path that
/// matters for production usage.
#[test]
fn on_initialize_finalizes_at_expiry_block() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let bond = expected_total_bond_for(&item);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        let info = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        let expiry = info.deposited_at + COMMITMENT_WINDOW;

        // Hooks fire at expiry +1 (since `deposited_at + window` is the last
        // window-block; `> window` triggers finalization).
        advance_to_block(expiry + 1);

        assert_eq!(treasury_balance(), bond);
        let after = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info kept");
        assert!(after.finalized);
    });
}

// -----------------------------------------------------------------------------
// force_edit
// -----------------------------------------------------------------------------

#[test]
fn force_edit_bypasses_window() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let initial = expected_total_bond_for(&item);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));

        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 100);

        let edited = mock_midds(b"abc", 20);
        let new_base = expected_base_bond_for(&edited);
        assert_ok!(Midds::force_edit(RuntimeOrigin::root(), 0, edited.clone()));

        assert_eq!(pallet_midds::Items::<Test, Instance>::get(0), Some(edited));
        assert_eq!(held(ALICE), new_base);
        assert!(new_base > initial);
        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::ForceEdited {
            id: 0,
        }));
    });
}

#[test]
fn force_edit_requires_root() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone()));

        let other = mock_midds(b"abc", 6);
        assert_noop!(
            Midds::force_edit(RuntimeOrigin::signed(ALICE), 0, other.clone()),
            sp_runtime::DispatchError::BadOrigin,
        );
        assert_noop!(
            Midds::force_edit(RuntimeOrigin::signed(BOB), 0, other),
            sp_runtime::DispatchError::BadOrigin,
        );
    });
}

#[test]
fn force_edit_rejects_after_finalization() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);
        assert_ok!(Midds::finalize(RuntimeOrigin::signed(BOB), 0));

        let other = mock_midds(b"abc", 6);
        assert_noop!(
            Midds::force_edit(RuntimeOrigin::root(), 0, other),
            pallet_midds::Error::<Test, Instance>::AlreadyFinalized,
        );
    });
}

// -----------------------------------------------------------------------------
// force_remove_refund
// -----------------------------------------------------------------------------

#[test]
fn force_remove_refund_returns_full_bond() {
    new_test_ext().execute_with(|| {
        // 2× premium to verify the refund is *total* (premium included).
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::from_u32(2));

        let item = mock_midds(b"abc", 5);
        let total = expected_total_bond_for(&item);
        let free_before = free(ALICE);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item.clone()));
        assert_ok!(Midds::force_remove_refund(RuntimeOrigin::root(), 0));

        assert!(pallet_midds::Items::<Test, Instance>::get(0).is_none());
        assert!(
            !pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(item.identifier(), 0)
        );
        assert_eq!(held(ALICE), 0);
        assert_eq!(free(ALICE), free_before);
        assert_eq!(treasury_balance(), 0);
        System::assert_has_event(RuntimeEvent::Midds(
            pallet_midds::Event::ForceRemovedRefund {
                id: 0,
                depositor: ALICE,
                refund: total,
            },
        ));
    });
}

#[test]
fn force_remove_refund_requires_root() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        assert_noop!(
            Midds::force_remove_refund(RuntimeOrigin::signed(ALICE), 0),
            sp_runtime::DispatchError::BadOrigin,
        );
    });
}

#[test]
fn force_remove_refund_rejects_after_finalization() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);
        assert_ok!(Midds::finalize(RuntimeOrigin::signed(BOB), 0));

        // Bond is already in Treasury — there's nothing to refund.
        assert_noop!(
            Midds::force_remove_refund(RuntimeOrigin::root(), 0),
            pallet_midds::Error::<Test, Instance>::AlreadyFinalized,
        );
    });
}

// -----------------------------------------------------------------------------
// force_remove_slash
// -----------------------------------------------------------------------------

#[test]
fn force_remove_slash_pre_finalization_sends_bond_to_treasury() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let bond = expected_total_bond_for(&item);
        let free_before = free(ALICE);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        assert_ok!(Midds::force_remove_slash(RuntimeOrigin::root(), 0));

        assert!(pallet_midds::Items::<Test, Instance>::get(0).is_none());
        assert_eq!(held(ALICE), 0);
        // Depositor lost the entire bond.
        assert_eq!(free(ALICE), free_before - bond);
        assert_eq!(treasury_balance(), bond);
    });
}

#[test]
fn force_remove_slash_post_finalization_only_cleans_storage() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let bond = expected_total_bond_for(&item);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);
        assert_ok!(Midds::finalize(RuntimeOrigin::signed(BOB), 0));

        let treasury_after_finalize = treasury_balance();
        assert_eq!(treasury_after_finalize, bond);

        assert_ok!(Midds::force_remove_slash(RuntimeOrigin::root(), 0));

        assert!(pallet_midds::Items::<Test, Instance>::get(0).is_none());
        assert!(pallet_midds::DepositInfo::<Test, Instance>::get(0).is_none());
        // Treasury balance unchanged: bond was already there.
        assert_eq!(treasury_balance(), treasury_after_finalize);
    });
}

#[test]
fn force_remove_slash_requires_root() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        assert_noop!(
            Midds::force_remove_slash(RuntimeOrigin::signed(BOB), 0),
            sp_runtime::DispatchError::BadOrigin,
        );
    });
}

// -----------------------------------------------------------------------------
// force_remove_many
// -----------------------------------------------------------------------------

#[test]
fn force_remove_many_batch_refund() {
    new_test_ext().execute_with(|| {
        let a = mock_midds(b"a", 5);
        let b = mock_midds(b"b", 6);
        let c = mock_midds(b"c", 7);
        let free_before = free(ALICE);
        for it in [&a, &b, &c] {
            assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), it.clone()));
        }

        assert_ok!(Midds::force_remove_many(
            RuntimeOrigin::root(),
            vec![0, 1, 2],
            false, // refund flag
        ));

        for id in 0..3 {
            assert!(pallet_midds::Items::<Test, Instance>::get(id).is_none());
        }
        assert_eq!(held(ALICE), 0);
        assert_eq!(free(ALICE), free_before);
        assert_eq!(treasury_balance(), 0);
    });
}

#[test]
fn force_remove_many_batch_slash() {
    new_test_ext().execute_with(|| {
        let a = mock_midds(b"a", 5);
        let b = mock_midds(b"b", 6);
        let total = expected_total_bond_for(&a) + expected_total_bond_for(&b);

        for it in [&a, &b] {
            assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), it.clone()));
        }

        assert_ok!(Midds::force_remove_many(
            RuntimeOrigin::root(),
            vec![0, 1],
            true, // slash flag
        ));

        assert_eq!(held(ALICE), 0);
        assert_eq!(treasury_balance(), total);
    });
}

// -----------------------------------------------------------------------------
// pathological: bond hold failure
// -----------------------------------------------------------------------------

#[test]
fn deposit_fails_when_account_lacks_funds_for_bond() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 30);

        const POOR: AccountId = 4;

        assert_noop!(
            Midds::deposit(RuntimeOrigin::signed(POOR), item),
            pallet_midds::Error::<Test, Instance>::BondHoldFailed,
        );

        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 0);
        assert!(
            pallet_midds::Items::<Test, Instance>::iter()
                .next()
                .is_none()
        );
        assert!(
            pallet_midds::IdentifierClaims::<Test, Instance>::iter()
                .next()
                .is_none()
        );
        assert!(
            pallet_midds::PayloadHashes::<Test, Instance>::iter()
                .next()
                .is_none()
        );
    });
}

// -----------------------------------------------------------------------------
// multiplier dynamics
// -----------------------------------------------------------------------------

/// A burst beyond `FastTargetPerBlock` pushes `M_fast` upward at the next
/// `on_initialize`. Mirrors the spec's "block stuffing → fast multiplier
/// spike" claim (§5.1).
#[test]
fn fast_multiplier_climbs_on_burst() {
    new_test_ext().execute_with(|| {
        // Saturate this block: deposits beyond FastTargetPerBlock.
        for i in 0..(FAST_TARGET_PER_BLOCK as u8 + 5) {
            let id_bytes = format!("a{i}");
            assert_ok!(Midds::deposit(
                RuntimeOrigin::signed(ALICE),
                mock_midds(id_bytes.as_bytes(), 1)
            ));
        }
        let before = pallet_midds::FastMultiplier::<Test, Instance>::get();
        assert_eq!(before, FixedU128::one());

        advance_to_block(System::block_number() + 1);

        let after = pallet_midds::FastMultiplier::<Test, Instance>::get();
        assert!(after > before, "M_fast must climb after a burst");
    });
}

#[test]
fn fast_multiplier_drops_on_idle_block() {
    new_test_ext().execute_with(|| {
        // Pin M_fast at 2.0× and idle through one block.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::from_u32(2));
        advance_to_block(System::block_number() + 1);
        assert!(pallet_midds::FastMultiplier::<Test, Instance>::get() < FixedU128::from_u32(2));
    });
}

// -----------------------------------------------------------------------------
// invariants
// -----------------------------------------------------------------------------

/// Walk a sequence of extrinsics and check the storage invariants the SDK
/// commits to.
#[test]
fn invariants_hold_across_full_lifecycle() {
    new_test_ext().execute_with(|| {
        let first = mock_midds(b"first", 5);
        let second = mock_midds(b"second", 8);
        let third = mock_midds(b"third", 3);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), first.clone()));
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 1);
        check_invariants(&[(0, ALICE, first.identifier())]);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(BOB), second.clone()));
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 2);
        check_invariants(&[
            (0, ALICE, first.identifier()),
            (1, BOB, second.identifier()),
        ]);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), third.clone()));
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 3);
        check_invariants(&[
            (0, ALICE, first.identifier()),
            (1, BOB, second.identifier()),
            (2, ALICE, third.identifier()),
        ]);

        let mut grown = mock_midds(b"first", 25);
        grown.data.iter_mut().for_each(|b| *b = 1);
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, grown));
        check_invariants(&[
            (0, ALICE, first.identifier()),
            (1, BOB, second.identifier()),
            (2, ALICE, third.identifier()),
        ]);

        assert_ok!(Midds::force_remove_refund(RuntimeOrigin::root(), 1));
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 3);
        assert!(
            !pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(second.identifier(), 1)
        );
        check_invariants(&[
            (0, ALICE, first.identifier()),
            (2, ALICE, third.identifier()),
        ]);
    });
}

fn check_invariants(expected: &[(u64, AccountId, MockId)]) {
    let mut total_held = std::collections::BTreeMap::<AccountId, Balance>::new();

    for (id, depositor, identifier) in expected {
        let item = pallet_midds::Items::<Test, Instance>::get(id)
            .unwrap_or_else(|| panic!("Items[{id}] missing"));
        let info = pallet_midds::DepositInfo::<Test, Instance>::get(id)
            .unwrap_or_else(|| panic!("DepositInfo[{id}] missing"));

        assert_eq!(info.depositor, *depositor, "depositor mismatch at id={id}");
        assert_eq!(
            item.identifier(),
            *identifier,
            "identifier drift at id={id}"
        );
        assert!(
            pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(identifier, *id),
            "IdentifierClaims missing entry for id={id}"
        );
        assert_eq!(
            info.base_bond,
            expected_base_bond_for(&item),
            "DepositInfo.base_bond drifted from the bond formula at id={id}"
        );
        // amount = base + premium ≥ base.
        assert!(info.amount >= info.base_bond);

        if !info.finalized {
            *total_held.entry(*depositor).or_default() += info.amount;
        }
    }

    for (account, expected_held) in total_held {
        assert_eq!(
            held(account),
            expected_held,
            "hold(account={account}) ≠ Σ DepositInfo.amount over non-finalized records",
        );
    }
}
