//! Lifecycle tests for `pallet-midds` over the mock runtime.
//!
//! Mirrors the matrix of `docs/economics.md`: deposit (with multipliers),
//! update (bond up/down, identifier immuable, premium preserved),
//! `remove_own` (base refund + premium to Treasury), `finalize` (eager via
//! hook + permissionless catch-up), the two `force_remove_*` variants, and
//! batch removal.

use crate as pallet_midds;
use crate::mock::{test_helpers::*, *};
use crate::types::{
    DepositOnBehalfPayload, OnBehalfAction, RemovalKind, RemovalRequest, RemoveOnBehalfPayload,
    UpdateOnBehalfPayload,
};
use frame_support::{BoundedVec, assert_noop, assert_ok, traits::Get};
use midds_traits::Midds as _;
use parity_scale_codec::Encode;
use sp_runtime::{
    FixedU128,
    traits::{One, Saturating},
};

type Instance = ();

// -----------------------------------------------------------------------------
// deposit
// -----------------------------------------------------------------------------

#[test]
fn deposit_works() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let identifier = item.identifier().clone();
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
        assert_eq!(info.sponsor_layer.payer, ALICE);
        assert_eq!(info.sponsor_layer.amount, bond);
        assert_eq!(info.sponsor_layer.base, base);
        assert!(info.owner_layer.is_none());
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
            bond_payer: ALICE,
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

        // Last in-window block: window is strictly < CommitmentWindow, so the
        // last accepted offset is `CommitmentWindow - 1`. At `+ window` exactly
        // the record is already finalizable.
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW - 1);
        assert_ok!(Midds::update(
            RuntimeOrigin::signed(ALICE),
            0,
            updated.clone()
        ));

        let info = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("deposit info");
        assert_eq!(pallet_midds::Items::<Test, Instance>::get(0), Some(updated));
        // Self-deposit: delta lands on sponsor_layer (no owner layer needed).
        assert_eq!(info.sponsor_layer.base, new_base);
        // Multipliers unchanged across the test → premium = 0, amount = base.
        assert_eq!(info.sponsor_layer.amount, new_base);
        assert!(info.owner_layer.is_none());

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Updated {
            id: 0,
            sponsor_bond: new_base,
            owner_bond: 0,
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
        assert_eq!(info.sponsor_layer.amount, total);
        assert_eq!(info.sponsor_layer.base, base);
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

/// Pin the residual `DuplicatePayload` branch inside `do_apply_edit`
/// (`lib.rs:670+`). The first-line `IdentifierClaims` check guards against
/// most collisions, but it can't catch the multi-claim case where two
/// records share an identifier and a third update lands on the second's
/// exact payload. Without this branch, the operation would silently
/// re-key `PayloadHashes`, leaking record 1's hash entry.
#[test]
fn update_rejects_payload_collision_under_multi_claim() {
    new_test_ext().execute_with(|| {
        // Two records share identifier "abc" with different payloads —
        // multi-claim is legal at deposit time.
        let r0 = mock_midds(b"abc", 5);
        let mut r1 = mock_midds(b"abc", 5);
        r1.data.iter_mut().for_each(|byte| *byte = 1);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), r0.clone()));
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), r1.clone()));

        // Updating r0 with r1's exact payload would byte-equal record 1.
        // Identifier check passes (still "abc"), but the payload hash now
        // maps to id 1, not 0 — `do_apply_edit` rejects with
        // `DuplicatePayload`.
        assert_noop!(
            Midds::update(RuntimeOrigin::signed(ALICE), 0, r1.clone()),
            pallet_midds::Error::<Test, Instance>::DuplicatePayload,
        );

        // Storage left untouched.
        assert_eq!(pallet_midds::Items::<Test, Instance>::get(0), Some(r0));
        assert_eq!(pallet_midds::Items::<Test, Instance>::get(1), Some(r1));
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
            sponsor: ALICE,
            sponsor_refund: base,
            owner_refund: 0,
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

/// Counterpart to [`remove_own_refunds_base_and_sends_premium_to_treasury`]:
/// when the deposit-time multiplier sits *below* 1 the layer was banked at a
/// discount (`amount < base`, no premium ever existed). `do_remove_own` must
/// then emit `sponsor_refund = amount`, not raw `base` — otherwise the event
/// overstates the on-chain balance movement by the `M < 1` discount and
/// off-chain consumers crediting on the event value end up over-paying.
#[test]
fn remove_own_at_sub_unit_multiplier_refunds_amount_not_base() {
    new_test_ext().execute_with(|| {
        // Pin M_fast at the configured floor (mock = 0.1×).
        let floor = <Test as pallet_midds::Config>::FastMultiplierMin::get();
        pallet_midds::FastMultiplier::<Test, Instance>::put(floor);

        let item = mock_midds(b"abc", 5);
        let base = expected_base_bond_for(&item);
        let total = expected_total_bond_for(&item);
        assert!(total < base, "M < 1 should price the deposit under base");

        let free_before = free(ALICE);
        let treasury_before = treasury_balance();

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        assert_ok!(Midds::remove_own(RuntimeOrigin::signed(ALICE), 0));

        // ALICE paid `total`, got `total` back: net zero balance movement.
        assert_eq!(held(ALICE), 0);
        assert_eq!(free(ALICE), free_before);
        // No premium ever existed → Treasury sees nothing.
        assert_eq!(treasury_balance(), treasury_before);

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Refunded {
            id: 0,
            depositor: ALICE,
            sponsor: ALICE,
            sponsor_refund: total,
            owner_refund: 0,
            premium_to_treasury: 0,
        }));
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
            sponsor: ALICE,
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

/// At the exact expiry block (`deposited_at + CommitmentWindow`):
/// - `update` and `remove_own` reject with `CommitmentWindowClosed`,
/// - `finalize` succeeds.
///
/// Pinning this rules out the previous off-by-one ambiguity where `update`
/// (using `<=`) and `finalize` (using `>`) both refused to act at expiry.
#[test]
fn window_boundary_at_expiry_is_finalizable_only() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        let deposited_at = System::block_number();

        // Land exactly on the expiry block.
        System::set_block_number(deposited_at + COMMITMENT_WINDOW);

        // `update` and `remove_own` no longer accept this depositor's call.
        let other = mock_midds(b"abc", 6);
        assert_noop!(
            Midds::update(RuntimeOrigin::signed(ALICE), 0, other),
            pallet_midds::Error::<Test, Instance>::CommitmentWindowClosed,
        );
        assert_noop!(
            Midds::remove_own(RuntimeOrigin::signed(ALICE), 0),
            pallet_midds::Error::<Test, Instance>::CommitmentWindowClosed,
        );

        // `finalize` works at the same block.
        assert_ok!(Midds::finalize(RuntimeOrigin::signed(BOB), 0));
    });
}

/// One block before expiry the deal is the inverse: `update` /
/// `remove_own` succeed, `finalize` rejects with `CommitmentWindowOpen`.
#[test]
fn window_boundary_one_before_expiry_blocks_finalize() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        let deposited_at = System::block_number();
        System::set_block_number(deposited_at + COMMITMENT_WINDOW - 1);

        assert_noop!(
            Midds::finalize(RuntimeOrigin::signed(BOB), 0),
            pallet_midds::Error::<Test, Instance>::CommitmentWindowOpen,
        );
        assert_ok!(Midds::remove_own(RuntimeOrigin::signed(ALICE), 0));
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

        // Window is strictly less than `CommitmentWindow` so the expiry
        // block itself is finalizable. The hook drains `PendingFinalization`
        // at exactly that block.
        advance_to_block(expiry);

        assert_eq!(treasury_balance(), bond);
        let after = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info kept");
        assert!(after.finalized);
    });
}

/// When more records expire on the same block than `MaxFinalizationsPerBlock`,
/// the leftover entries must roll forward and finalize on subsequent blocks
/// via the rolling cursor. Without that rollover they would stay stuck under
/// their original expiry prefix and only the permissionless `finalize` crank
/// could clear them.
#[test]
fn on_initialize_rolls_pending_finalizations_forward() {
    new_test_ext().execute_with(|| {
        // Deposit `cap + overflow` records at block 1 so they all share the
        // same expiry prefix. The fast multiplier will spike under this
        // burst — expected, irrelevant to what we exercise here.
        let cap: u64 =
            <<Test as pallet_midds::Config<Instance>>::MaxFinalizationsPerBlock as Get<u32>>::get()
                .into();
        let overflow = 5u64;
        let total = cap + overflow;
        for i in 0..total {
            let mut id = [0u8; 4];
            id[0] = b'a';
            id[1..4].copy_from_slice(&[
                (i / 100) as u8 + b'0',
                ((i / 10) % 10) as u8 + b'0',
                (i % 10) as u8 + b'0',
            ]);
            assert_ok!(Midds::deposit(
                RuntimeOrigin::signed(ALICE),
                mock_midds(&id, 1)
            ));
        }
        let info = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("first info");
        let expiry = info.deposited_at + COMMITMENT_WINDOW;

        // First hook at the expiry block can only drain `cap` entries.
        advance_to_block(expiry);
        let finalized_first_pass = (0..total)
            .filter(|i| {
                pallet_midds::DepositInfo::<Test, Instance>::get(*i)
                    .map(|d| d.finalized)
                    .unwrap_or(false)
            })
            .count() as u64;
        assert_eq!(
            finalized_first_pass, cap,
            "first hook should finalize exactly `cap` records"
        );
        // Cursor must still point at the overflowed prefix so the next hook
        // resumes the drain instead of skipping ahead.
        assert_eq!(
            pallet_midds::NextFinalizationScan::<Test, Instance>::get(),
            expiry,
        );

        // One more block clears the leftover and advances the cursor past
        // the prefix.
        advance_to_block(expiry + 1);
        let finalized_total = (0..total)
            .filter(|i| {
                pallet_midds::DepositInfo::<Test, Instance>::get(*i)
                    .map(|d| d.finalized)
                    .unwrap_or(false)
            })
            .count() as u64;
        assert_eq!(
            finalized_total, total,
            "rollover must catch up the entire backlog on the next block"
        );
        assert_eq!(
            pallet_midds::NextFinalizationScan::<Test, Instance>::get(),
            expiry + 2,
        );
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

/// Post-finalization `force_edit` (cf. `docs/economics.md` §4) updates
/// the on-chain payload + reverse hash index without touching the bond
/// — which already lives in the Treasury at this point. Lets governance
/// fix a typo on a permanent record without losing attribution.
#[test]
fn force_edit_post_finalization_updates_payload_only() {
    use sp_runtime::traits::Hash as _;

    new_test_ext().execute_with(|| {
        let item = mock_midds(b"abc", 5);
        let original_hash = <Test as frame_system::Config>::Hashing::hash_of(&item);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);
        assert_ok!(Midds::finalize(RuntimeOrigin::signed(BOB), 0));

        let treasury_at_finalize = treasury_balance();
        let info_before = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        assert!(info_before.finalized);
        assert_eq!(info_before.payload_hash, original_hash);

        let edited = mock_midds(b"abc", 6);
        let new_hash = <Test as frame_system::Config>::Hashing::hash_of(&edited);
        assert_ne!(new_hash, original_hash);

        assert_ok!(Midds::force_edit(RuntimeOrigin::root(), 0, edited.clone()));

        // Payload + hash index swapped; bond accounting untouched.
        assert_eq!(pallet_midds::Items::<Test, Instance>::get(0), Some(edited));
        let info_after = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        assert!(info_after.finalized);
        assert_eq!(info_after.payload_hash, new_hash);
        assert_eq!(info_after.sponsor_layer, info_before.sponsor_layer);
        assert_eq!(info_after.owner_layer, info_before.owner_layer);
        assert_eq!(treasury_balance(), treasury_at_finalize);
        assert!(pallet_midds::PayloadHashes::<Test, Instance>::get(new_hash).is_some());
        assert!(
            pallet_midds::PayloadHashes::<Test, Instance>::get(original_hash).is_none(),
            "stale payload hash must be evicted"
        );

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::ForceEdited {
            id: 0,
        }));
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
                sponsor: ALICE,
                sponsor_refund: total,
                owner_refund: 0,
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
            BoundedVec::try_from(vec![
                RemovalRequest {
                    id: 0,
                    kind: RemovalKind::Refund
                },
                RemovalRequest {
                    id: 1,
                    kind: RemovalKind::Refund
                },
                RemovalRequest {
                    id: 2,
                    kind: RemovalKind::Refund
                },
            ])
            .expect("3 ≤ MaxRemovalsPerCall"),
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
            BoundedVec::try_from(vec![
                RemovalRequest {
                    id: 0,
                    kind: RemovalKind::Slash
                },
                RemovalRequest {
                    id: 1,
                    kind: RemovalKind::Slash
                },
            ])
            .expect("2 ≤ MaxRemovalsPerCall"),
        ));

        assert_eq!(held(ALICE), 0);
        assert_eq!(treasury_balance(), total);
    });
}

/// `BoundedVec::try_from` rejects request lists longer than
/// `MaxRemovalsPerCall`. The bound is the gateway preventing a single
/// `force_remove_many` call from blowing the per-block weight budget.
#[test]
fn force_remove_many_rejects_oversized_input() {
    let oversized: Vec<RemovalRequest> = (0..(MAX_REMOVALS_PER_CALL as u64 + 1))
        .map(|id| RemovalRequest {
            id,
            kind: RemovalKind::Refund,
        })
        .collect();
    assert!(
        BoundedVec::<RemovalRequest, frame_support::traits::ConstU32<MAX_REMOVALS_PER_CALL>>::try_from(oversized).is_err(),
        "MaxRemovalsPerCall bound should reject lists longer than the limit",
    );
}

/// `force_remove_many` may interleave refunds and slashes in a single
/// call — the per-id `RemovalKind` makes mixed batches a one-extrinsic
/// operation instead of two separate force calls.
#[test]
fn force_remove_many_mixed_batch_per_id_kind() {
    new_test_ext().execute_with(|| {
        let a = mock_midds(b"a", 5);
        let b = mock_midds(b"b", 6);
        let total_b = expected_total_bond_for(&b);
        let free_before = free(ALICE);

        for it in [&a, &b] {
            assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), it.clone()));
        }

        assert_ok!(Midds::force_remove_many(
            RuntimeOrigin::root(),
            BoundedVec::try_from(vec![
                RemovalRequest {
                    id: 0,
                    kind: RemovalKind::Refund
                },
                RemovalRequest {
                    id: 1,
                    kind: RemovalKind::Slash
                },
            ])
            .expect("2 ≤ MaxRemovalsPerCall"),
        ));

        assert_eq!(held(ALICE), 0);
        // id 0 refunded → ALICE balance restored, id 1 slashed → treasury keeps `total_b`.
        assert_eq!(free(ALICE), free_before - total_b);
        assert_eq!(treasury_balance(), total_b);
    });
}

/// `#[transactional]` on `force_remove_many` must roll the entire batch
/// back if any single id fails — without atomicity the operator would
/// observe an unstable middle state where some ids were settled and
/// others were not.
#[test]
fn force_remove_many_rolls_back_partial_batch_on_failure() {
    new_test_ext().execute_with(|| {
        let a = mock_midds(b"a", 5);
        let b = mock_midds(b"b", 6);
        let total_a = expected_total_bond_for(&a);
        let total_b = expected_total_bond_for(&b);

        for it in [&a, &b] {
            assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), it.clone()));
        }
        let held_after_deposit = held(ALICE);
        assert_eq!(held_after_deposit, total_a + total_b);

        // Submit a batch where the second id (=42) does not exist. Without
        // `#[transactional]`, the loop would settle id=0 then bail at id=42,
        // leaving ALICE's hold = total_b. With it, the failure rolls back
        // both layers and ALICE's hold is unchanged.
        assert_noop!(
            Midds::force_remove_many(
                RuntimeOrigin::root(),
                BoundedVec::try_from(vec![
                    RemovalRequest {
                        id: 0,
                        kind: RemovalKind::Refund,
                    },
                    RemovalRequest {
                        id: 42, // never deposited
                        kind: RemovalKind::Refund,
                    },
                ])
                .expect("2 ≤ MaxRemovalsPerCall"),
            ),
            pallet_midds::Error::<Test, Instance>::MiddsNotFound,
        );

        assert_eq!(
            held(ALICE),
            held_after_deposit,
            "partial batch must roll back both layers' settlement"
        );
        assert!(pallet_midds::DepositInfo::<Test, Instance>::contains_key(0));
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

/// Growth via `update` must feed the multiplier demand counters — the
/// pallet should not let an actor deposit at a tiny size while
/// multipliers are low and then grow the payload without the network
/// ever observing the storage increase. The sticky-premium rule keeps
/// the *growth itself* cheap (anti-arbitrage §5.5), but the demand
/// signal is what feeds `M_fast` / `M_slow`.
#[test]
fn update_grow_records_deposit_demand() {
    new_test_ext().execute_with(|| {
        // Fresh chain: nothing recorded yet.
        assert_eq!(pallet_midds::DepositsThisBlock::<Test, Instance>::get(), 0);

        let small = mock_midds(b"grow1", 1);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), small));
        let after_deposit = pallet_midds::DepositsThisBlock::<Test, Instance>::get();
        assert_eq!(after_deposit, 1, "deposit always counts");

        // Same-block grow via `update` must increment the counter — the
        // network observes a storage extension just like a fresh deposit.
        let bigger = mock_midds(b"grow1", 8);
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, bigger));
        assert_eq!(
            pallet_midds::DepositsThisBlock::<Test, Instance>::get(),
            after_deposit + 1,
            "update grow must bump per-block demand"
        );

        // A subsequent shrink must NOT count — freeing storage is the
        // opposite of demand.
        let smaller = mock_midds(b"grow1", 3);
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, smaller));
        assert_eq!(
            pallet_midds::DepositsThisBlock::<Test, Instance>::get(),
            after_deposit + 1,
            "shrink must not feed the demand signal"
        );
    });
}

// -----------------------------------------------------------------------------
// reverse lookup
// -----------------------------------------------------------------------------

/// Pin the cap surfaced by `lookup_by_identifier`: a popular identifier
/// must not produce an unbounded `Vec<MiddsId>`, otherwise the runtime
/// API / RPC would OOM the node. Pagination via
/// `lookup_by_identifier_paged` is the supported way past the cap.
#[test]
fn lookup_by_identifier_paginates_and_caps() {
    new_test_ext().execute_with(|| {
        // Six different payloads sharing the same identifier "shared".
        let total = 6u8;
        for i in 0..total {
            let mut item = mock_midds(b"shared", 5);
            item.data.iter_mut().for_each(|byte| *byte = i);
            assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        }
        let identifier = mock_midds(b"shared", 5).id.clone();

        // Unpaginated call returns every id sorted ascending (under cap).
        let all = pallet_midds::Pallet::<Test, Instance>::lookup_by_identifier(identifier.clone());
        assert_eq!(all, (0..total as u64).collect::<Vec<_>>());
        assert_eq!(
            pallet_midds::Pallet::<Test, Instance>::count_by_identifier(identifier.clone()),
            total as u32,
        );

        // Page through with limit=2 and walk the cursor.
        let page1 = pallet_midds::Pallet::<Test, Instance>::lookup_by_identifier_paged(
            identifier.clone(),
            None,
            2,
        );
        assert_eq!(page1, vec![0, 1]);
        let page2 = pallet_midds::Pallet::<Test, Instance>::lookup_by_identifier_paged(
            identifier.clone(),
            Some(*page1.last().expect("page1 not empty")),
            2,
        );
        assert_eq!(page2, vec![2, 3]);
        let page3 = pallet_midds::Pallet::<Test, Instance>::lookup_by_identifier_paged(
            identifier.clone(),
            Some(*page2.last().expect("page2 not empty")),
            2,
        );
        assert_eq!(page3, vec![4, 5]);
        let page4 = pallet_midds::Pallet::<Test, Instance>::lookup_by_identifier_paged(
            identifier.clone(),
            Some(*page3.last().expect("page3 not empty")),
            2,
        );
        assert!(page4.is_empty(), "exhausted cursor must yield empty page");

        // Limit beyond the cap clamps silently.
        let cap = pallet_midds::Pallet::<Test, Instance>::MAX_LOOKUP_LIMIT;
        let oversize = pallet_midds::Pallet::<Test, Instance>::lookup_by_identifier_paged(
            identifier,
            None,
            cap + 100,
        );
        assert_eq!(oversize.len(), total as usize);
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

/// The proportional EIP-1559 form means the per-block move scales with
/// `(observed − target) / target`. A 10× burst must therefore lift `M_fast`
/// substantially more than a 2× burst over the same starting value — the
/// previous binary "above/below" code clamped both cases to a single
/// `+rate` step and missed the spec's "spike proportional to demand"
/// claim (`docs/economics.md` §5.1).
#[test]
fn fast_multiplier_step_scales_with_deviation() {
    use frame_support::traits::Hooks;

    fn observe(deposits: u32) -> FixedU128 {
        new_test_ext().execute_with(|| {
            // Genesis multipliers are 1.0×; we feed `deposits` directly into
            // `DepositsThisBlock` instead of going through `Midds::deposit`
            // so the test isolates the multiplier math from the rest of the
            // pallet (bonds, format checks, etc.).
            pallet_midds::DepositsThisBlock::<Test, Instance>::put(deposits);
            let n = System::block_number() + 1;
            System::set_block_number(n);
            <crate::Pallet<Test, ()> as Hooks<BlockNumber>>::on_initialize(n);
            pallet_midds::FastMultiplier::<Test, Instance>::get()
        })
    }

    let target = FAST_TARGET_PER_BLOCK;
    let m_at_2x = observe(2 * target);
    let m_at_10x = observe(10 * target);

    // Both should be > 1.0× (above target).
    assert!(m_at_2x > FixedU128::one());
    assert!(m_at_10x > FixedU128::one());
    // 10× burst must lift the multiplier strictly more than a 2× burst —
    // proves the step is not a constant `+rate` regardless of amplitude.
    assert!(
        m_at_10x > m_at_2x,
        "10× burst ({m_at_10x:?}) must outpace 2× burst ({m_at_2x:?})"
    );
    // And the gap must be material: the 10× step should be at least double
    // the 2× step (in linear-step terms `9·rate` vs `1·rate`).
    let step_2x = m_at_2x - FixedU128::one();
    let step_10x = m_at_10x - FixedU128::one();
    assert!(
        step_10x > step_2x.saturating_mul(FixedU128::from_u32(2)),
        "step at 10× ({step_10x:?}) must be > 2× the step at 2× ({step_2x:?})"
    );
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

/// `current_deposit_price(size)` is the contract the runtime API exposes to
/// off-chain consumers (CLI quoting, dashboard wallet UX). Pin that the
/// quote matches the bond effectively held by a same-block deposit, even
/// once the multipliers have moved off `1.0×`.
#[test]
fn current_deposit_price_matches_actual_bond() {
    new_test_ext().execute_with(|| {
        // Move M_fast off the unit baseline so the test would catch a
        // missing multiplier in the quote path.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::from_rational(7, 4));
        let item = mock_midds(b"abc", 5);
        let size = item.encoded_size() as u32;

        let quoted = pallet_midds::Pallet::<Test, Instance>::current_deposit_price(size);
        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), item));
        let info =
            pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("deposit info present");

        assert_eq!(
            info.sponsor_layer.amount, quoted,
            "runtime-API quote must equal the bond actually placed on hold"
        );
        assert_eq!(held(ALICE), quoted, "held bond must match the quote");
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
        check_invariants(&[(0, ALICE, first.identifier().clone())]);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(BOB), second.clone()));
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 2);
        check_invariants(&[
            (0, ALICE, first.identifier().clone()),
            (1, BOB, second.identifier().clone()),
        ]);

        assert_ok!(Midds::deposit(RuntimeOrigin::signed(ALICE), third.clone()));
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 3);
        check_invariants(&[
            (0, ALICE, first.identifier().clone()),
            (1, BOB, second.identifier().clone()),
            (2, ALICE, third.identifier().clone()),
        ]);

        let mut grown = mock_midds(b"first", 25);
        grown.data.iter_mut().for_each(|b| *b = 1);
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, grown));
        check_invariants(&[
            (0, ALICE, first.identifier().clone()),
            (1, BOB, second.identifier().clone()),
            (2, ALICE, third.identifier().clone()),
        ]);

        assert_ok!(Midds::force_remove_refund(RuntimeOrigin::root(), 1));
        assert_eq!(pallet_midds::NextMiddsId::<Test, Instance>::get(), 3);
        assert!(
            !pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(
                second.identifier().clone(),
                1
            )
        );
        check_invariants(&[
            (0, ALICE, first.identifier().clone()),
            (2, ALICE, third.identifier().clone()),
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
        assert_eq!(item.identifier(), identifier, "identifier drift at id={id}");
        assert!(
            pallet_midds::IdentifierClaims::<Test, Instance>::contains_key(identifier, *id),
            "IdentifierClaims missing entry for id={id}"
        );
        let total_base = info.sponsor_layer.base
            + info
                .owner_layer
                .as_ref()
                .map(|l| l.base)
                .unwrap_or_default();
        assert_eq!(
            total_base,
            expected_base_bond_for(&item),
            "Σ layer.base drifted from the bond formula at id={id}"
        );
        // Each layer: amount = base + premium ≥ base.
        assert!(info.sponsor_layer.amount >= info.sponsor_layer.base);
        if let Some(layer) = info.owner_layer.as_ref() {
            assert!(layer.amount >= layer.base);
        }

        if !info.finalized {
            *total_held.entry(info.sponsor_layer.payer).or_default() += info.sponsor_layer.amount;
            if let Some(layer) = info.owner_layer.as_ref() {
                *total_held.entry(layer.payer).or_default() += layer.amount;
            }
        }
    }

    for (account, expected_held) in total_held {
        assert_eq!(
            held(account),
            expected_held,
            "hold(account={account}) ≠ Σ layer.amount over non-finalized records",
        );
    }
}

// -----------------------------------------------------------------------------
// deposit_on_behalf / update_on_behalf
// -----------------------------------------------------------------------------

/// Convenience `valid_until` for tests that don't specifically exercise
/// signature expiry — pinned far enough in the future that no realistic
/// `advance_to_block` ever exceeds it.
const FAR_FUTURE: BlockNumber = BlockNumber::MAX;

fn test_genesis_hash() -> sp_core::H256 {
    pallet_midds::Pallet::<Test, Instance>::genesis_hash()
}

fn test_kind_bytes() -> Vec<u8> {
    pallet_midds::Pallet::<Test, Instance>::kind_bytes()
}

/// Build the canonical SCALE payload an owner signs to authorize a sponsored
/// deposit. Mirrors the on-chain construction so a test signs exactly what the
/// pallet will check.
fn deposit_payload(
    item: &MockMidds,
    operator: AccountId,
    nonce: u64,
    valid_until: BlockNumber,
) -> DepositOnBehalfPayload<AccountId, BlockNumber, sp_core::H256, MockMidds> {
    DepositOnBehalfPayload {
        kind: test_kind_bytes(),
        genesis_hash: test_genesis_hash(),
        action: OnBehalfAction::Deposit,
        item: item.clone(),
        operator,
        nonce,
        valid_until,
    }
}

fn update_payload(
    id: midds_traits::MiddsId,
    item: &MockMidds,
    operator: AccountId,
    nonce: u64,
    valid_until: BlockNumber,
) -> UpdateOnBehalfPayload<AccountId, BlockNumber, sp_core::H256, MockMidds> {
    UpdateOnBehalfPayload {
        kind: test_kind_bytes(),
        genesis_hash: test_genesis_hash(),
        action: OnBehalfAction::Update,
        id,
        item: item.clone(),
        operator,
        nonce,
        valid_until,
    }
}

fn remove_payload(
    id: midds_traits::MiddsId,
    operator: AccountId,
    nonce: u64,
    valid_until: BlockNumber,
) -> RemoveOnBehalfPayload<AccountId, BlockNumber, sp_core::H256> {
    RemoveOnBehalfPayload {
        kind: test_kind_bytes(),
        genesis_hash: test_genesis_hash(),
        action: OnBehalfAction::Remove,
        id,
        operator,
        nonce,
        valid_until,
    }
}

#[test]
fn deposit_on_behalf_attributes_owner_holds_on_operator() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"sponsor1", 5);
        let bond = expected_total_bond_for(&item);

        let payload = deposit_payload(&item, BOB, 0, FAR_FUTURE);
        let sig = sign(ALICE, &payload);

        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            item.clone(),
            0,
            FAR_FUTURE,
            sig,
        ));

        let info =
            pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("deposit info present");
        assert_eq!(info.depositor, ALICE);
        assert_eq!(info.sponsor_layer.payer, BOB);
        assert_eq!(info.sponsor_layer.amount, bond);
        assert!(info.owner_layer.is_none());

        // Bond is held against the sponsor, not the owner.
        assert_eq!(held(BOB), bond);
        assert_eq!(held(ALICE), 0);
        // Per-owner nonce bumps.
        assert_eq!(pallet_midds::OnBehalfNonce::<Test, Instance>::get(ALICE), 1);

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Deposited {
            id: 0,
            depositor: ALICE,
            bond_payer: BOB,
            bond,
            base_bond: info.sponsor_layer.base,
        }));
    });
}

#[test]
fn deposit_on_behalf_rejects_invalid_signature() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"sponsor2", 5);
        // Sign a payload claiming a *different* operator than the one calling.
        let payload = deposit_payload(&item, CHARLIE, 0, FAR_FUTURE);
        let sig = sign(ALICE, &payload);

        assert_noop!(
            Midds::deposit_on_behalf(RuntimeOrigin::signed(BOB), ALICE, item, 0, FAR_FUTURE, sig),
            pallet_midds::Error::<Test, Instance>::InvalidSignature
        );
    });
}

#[test]
fn deposit_on_behalf_rejects_replay() {
    new_test_ext().execute_with(|| {
        let first = mock_midds(b"replay1", 5);
        let payload = deposit_payload(&first, BOB, 0, FAR_FUTURE);
        let sig = sign(ALICE, &payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            first,
            0,
            FAR_FUTURE,
            sig.clone(),
        ));

        // Same nonce used twice — the on-chain counter is now 1, replay
        // attempt at nonce=0 is refused.
        let second = mock_midds(b"replay2", 5);
        let stale_payload = deposit_payload(&second, BOB, 0, FAR_FUTURE);
        let stale_sig = sign(ALICE, &stale_payload);
        assert_noop!(
            Midds::deposit_on_behalf(
                RuntimeOrigin::signed(BOB),
                ALICE,
                second,
                0,
                FAR_FUTURE,
                stale_sig,
            ),
            pallet_midds::Error::<Test, Instance>::InvalidNonce
        );
    });
}

/// At `OnBehalfNonce[owner] == u64::MAX` the next bump must surface
/// `NonceOverflow` rather than silently saturating — otherwise the same
/// `nonce = u64::MAX` signature would remain accepted forever (replay).
#[test]
fn deposit_on_behalf_rejects_nonce_overflow() {
    new_test_ext().execute_with(|| {
        // Pin the on-chain counter at the boundary.
        pallet_midds::OnBehalfNonce::<Test, Instance>::insert(ALICE, u64::MAX);

        let item = mock_midds(b"maxnonce", 5);
        let payload = deposit_payload(&item, BOB, u64::MAX, FAR_FUTURE);
        let sig = sign(ALICE, &payload);

        assert_noop!(
            Midds::deposit_on_behalf(
                RuntimeOrigin::signed(BOB),
                ALICE,
                item,
                u64::MAX,
                FAR_FUTURE,
                sig
            ),
            pallet_midds::Error::<Test, Instance>::NonceOverflow
        );
    });
}

/// A signature whose `valid_until` has elapsed must be rejected with
/// `SignatureExpired` — even if the nonce still matches. The owner
/// pinned a freshness window and the chain enforces it.
#[test]
fn deposit_on_behalf_rejects_expired_signature() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"expires", 5);
        // Sign with `valid_until = current_block`, then advance past it.
        let now = System::block_number();
        let payload = deposit_payload(&item, BOB, 0, now);
        let sig = sign(ALICE, &payload);

        System::set_block_number(now + 1);

        assert_noop!(
            Midds::deposit_on_behalf(RuntimeOrigin::signed(BOB), ALICE, item, 0, now, sig),
            pallet_midds::Error::<Test, Instance>::SignatureExpired
        );
    });
}

/// A payload whose `genesis_hash` does not match the running chain must be
/// rejected — the on-chain side rebuilds the canonical encoding with the
/// real genesis hash, so a forged hash produces a different SCALE blob and
/// the signature fails verification.
#[test]
fn deposit_on_behalf_rejects_wrong_genesis_hash() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"otherchn", 5);
        // Forge a payload pretending to come from a different chain (any
        // non-real H256 will do; test_genesis_hash() returns the real one).
        let mut payload = deposit_payload(&item, BOB, 0, FAR_FUTURE);
        payload.genesis_hash = sp_core::H256::from([0xAB; 32]);
        let sig = sign(ALICE, &payload);

        assert_noop!(
            Midds::deposit_on_behalf(RuntimeOrigin::signed(BOB), ALICE, item, 0, FAR_FUTURE, sig),
            pallet_midds::Error::<Test, Instance>::InvalidSignature
        );
    });
}

#[test]
fn remove_own_on_sponsored_record_refunds_sponsor() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"refund", 5);
        let base = expected_base_bond_for(&item);
        let total = expected_total_bond_for(&item);
        let premium = total - base;

        let bob_free_before = free(BOB);
        let alice_free_before = free(ALICE);
        let treasury_before = treasury_balance();

        let payload = deposit_payload(&item, BOB, 0, FAR_FUTURE);
        let sig = sign(ALICE, &payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            item.clone(),
            0,
            FAR_FUTURE,
            sig,
        ));
        // Owner keeps authority — they call remove_own.
        assert_ok!(Midds::remove_own(RuntimeOrigin::signed(ALICE), 0));

        // Sponsor's hold released; net loss is exactly the multiplier
        // premium that flowed to the Treasury. Owner's balance is untouched.
        assert_eq!(held(BOB), 0);
        assert_eq!(free(BOB), bob_free_before - premium);
        assert_eq!(free(ALICE), alice_free_before);
        assert_eq!(treasury_balance(), treasury_before + premium);

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Refunded {
            id: 0,
            depositor: ALICE,
            sponsor: BOB,
            sponsor_refund: base,
            owner_refund: 0,
            premium_to_treasury: premium,
        }));
    });
}

/// Web3 escape hatch: the owner of a sponsored record can extend it via
/// plain `update`, paying the delta out of their own balance. The
/// `sponsor_layer` is left untouched; an `owner_layer` materializes and
/// banks the current multiplier premium against the depositor.
#[test]
fn update_grows_sponsored_record_via_escape_hatch() {
    new_test_ext().execute_with(|| {
        let initial = mock_midds(b"plainup", 5);
        let initial_base = expected_base_bond_for(&initial);
        let payload = deposit_payload(&initial, BOB, 0, FAR_FUTURE);
        let sig = sign(ALICE, &payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            initial,
            0,
            FAR_FUTURE,
            sig,
        ));
        let sponsor_held_before = held(BOB);

        // Force a 2× M_fast so we can observe the owner's premium getting
        // banked at *current* multipliers when the owner_layer is created.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::from_u32(2));

        let bigger = mock_midds(b"plainup", 10);
        let bigger_base = expected_base_bond_for(&bigger);
        let delta_base = bigger_base - initial_base;
        let owner_amount = delta_base * 2; // current M = 2.0×

        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, bigger));

        let info = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        assert_eq!(info.sponsor_layer.payer, BOB);
        assert_eq!(info.sponsor_layer.amount, sponsor_held_before);
        let owner_layer = info
            .owner_layer
            .expect("escape-hatch owner_layer materialized");
        assert_eq!(owner_layer.payer, ALICE);
        assert_eq!(owner_layer.base, delta_base);
        assert_eq!(owner_layer.amount, owner_amount);

        // Sponsor's hold is unchanged; owner's hold equals the multiplied
        // delta. (We don't assert on `free(ALICE)` here because the first
        // hold on a previously-untouched account also kicks in the ED
        // accounting in `pallet_balances::reducible_balance`.)
        assert_eq!(held(BOB), sponsor_held_before);
        assert_eq!(held(ALICE), owner_amount);

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Updated {
            id: 0,
            sponsor_bond: sponsor_held_before,
            owner_bond: owner_amount,
        }));
    });
}

/// Owner extends, then the *original sponsor* still extends their own
/// layer through `update_on_behalf`. Both layers grow independently — the
/// stratified model lets the SaaS sponsor and the autonomous owner
/// co-exist on the same record (cf. design Q2.b).
#[test]
fn update_on_behalf_coexists_with_owner_layer() {
    new_test_ext().execute_with(|| {
        let initial = mock_midds(b"coexist", 5);
        let dep_payload = deposit_payload(&initial, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            initial,
            0,
            FAR_FUTURE,
            dep_sig,
        ));

        // Step 1 — owner extends in solo, materializes owner_layer.
        let mid = mock_midds(b"coexist", 10);
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, mid));
        let after_owner = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        let owner_after_step1 = after_owner.owner_layer.as_ref().expect("layer").amount;
        let sponsor_after_step1 = after_owner.sponsor_layer.amount;

        // Step 2 — sponsor extends further on top, both layers must coexist.
        let bigger = mock_midds(b"coexist", 18);
        let upd_payload = update_payload(0, &bigger, BOB, 1, FAR_FUTURE);
        let upd_sig = sign(ALICE, &upd_payload);
        assert_ok!(Midds::update_on_behalf(
            RuntimeOrigin::signed(BOB),
            0,
            bigger,
            1,
            FAR_FUTURE,
            upd_sig,
        ));

        let after_sponsor = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        // Sponsor layer grew; owner layer untouched.
        assert!(after_sponsor.sponsor_layer.amount > sponsor_after_step1);
        let owner_layer = after_sponsor
            .owner_layer
            .as_ref()
            .expect("owner_layer survives sponsor extend");
        assert_eq!(owner_layer.amount, owner_after_step1);
        assert_eq!(owner_layer.payer, ALICE);
    });
}

/// Owner shrinks below their own layer's base — the overflow refunds the
/// sponsor (LIFO release across layers).
#[test]
fn update_shrink_drains_owner_then_overflows_to_sponsor() {
    new_test_ext().execute_with(|| {
        let initial = mock_midds(b"shrink", 5);
        let dep_payload = deposit_payload(&initial, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            initial,
            0,
            FAR_FUTURE,
            dep_sig,
        ));
        let bob_held_at_deposit = held(BOB);

        // Owner extends so owner_layer.base = 25 - 5 = 20 (unit multipliers).
        let bigger = mock_midds(b"shrink", 25);
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, bigger));
        let alice_held_after_extend = held(ALICE);

        // Owner shrinks back to size 0: total base = DEPOSIT_BASE only,
        // delta = -(25 unit bytes). LIFO drains owner first (20), then
        // overflows 5 into sponsor's base.
        let smaller = mock_midds(b"shrink", 0);
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, smaller));

        let info = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        // Owner layer kept (premium=0 at unit multipliers, but the layer
        // survives) with base = 0.
        let owner_layer = info.owner_layer.as_ref().expect("layer survives");
        assert_eq!(owner_layer.base, 0);
        assert_eq!(owner_layer.amount, 0);
        // Sponsor base shrunk by the overflow (5 bytes).
        assert!(info.sponsor_layer.base < bob_held_at_deposit);
        // Owner fully released.
        assert_eq!(held(ALICE), 0);
        // Sponsor partially released.
        assert!(held(BOB) < bob_held_at_deposit);
        assert!(alice_held_after_extend > 0); // sanity for the helper variable
    });
}

#[test]
fn update_on_behalf_extends_sponsor_hold() {
    new_test_ext().execute_with(|| {
        let initial = mock_midds(b"upbehalf", 5);
        let dep_payload = deposit_payload(&initial, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            initial,
            0,
            FAR_FUTURE,
            dep_sig,
        ));
        let initial_held = held(BOB);

        let updated = mock_midds(b"upbehalf", 12);
        let upd_payload = update_payload(0, &updated, BOB, 1, FAR_FUTURE);
        let upd_sig = sign(ALICE, &upd_payload);
        assert_ok!(Midds::update_on_behalf(
            RuntimeOrigin::signed(BOB),
            0,
            updated.clone(),
            1,
            FAR_FUTURE,
            upd_sig,
        ));

        let info = pallet_midds::DepositInfo::<Test, Instance>::get(0).expect("info");
        assert_eq!(info.depositor, ALICE);
        assert_eq!(info.sponsor_layer.payer, BOB);
        // Hold against the sponsor grew, not against the owner.
        assert!(held(BOB) > initial_held);
        assert_eq!(held(ALICE), 0);
        // No owner contribution yet — sponsor is the sole payer.
        assert!(info.owner_layer.is_none());
        assert_eq!(pallet_midds::OnBehalfNonce::<Test, Instance>::get(ALICE), 2);
    });
}

#[test]
fn update_on_behalf_rejects_wrong_sponsor() {
    new_test_ext().execute_with(|| {
        let initial = mock_midds(b"wrongsp", 5);
        let dep_payload = deposit_payload(&initial, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            initial,
            0,
            FAR_FUTURE,
            dep_sig,
        ));

        // Charlie tries to extend the hold even with a valid owner signature
        // co-authorizing the update. Refused: only the original sponsor may
        // grow their own hold.
        let updated = mock_midds(b"wrongsp", 8);
        let upd_payload = update_payload(0, &updated, CHARLIE, 1, FAR_FUTURE);
        let upd_sig = sign(ALICE, &upd_payload);
        assert_noop!(
            Midds::update_on_behalf(
                RuntimeOrigin::signed(CHARLIE),
                0,
                updated,
                1,
                FAR_FUTURE,
                upd_sig
            ),
            pallet_midds::Error::<Test, Instance>::WrongSponsor
        );
    });
}

#[test]
fn update_on_behalf_rejects_invalid_signature() {
    new_test_ext().execute_with(|| {
        let initial = mock_midds(b"badsig", 5);
        let dep_payload = deposit_payload(&initial, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            initial,
            0,
            FAR_FUTURE,
            dep_sig,
        ));

        let updated = mock_midds(b"badsig", 8);
        let upd_payload = update_payload(0, &updated, BOB, 1, FAR_FUTURE);
        // Charlie (not the owner) signs — verification keys to ALICE, not CHARLIE.
        let bad_sig = sign(CHARLIE, &upd_payload);
        assert_noop!(
            Midds::update_on_behalf(
                RuntimeOrigin::signed(BOB),
                0,
                updated,
                1,
                FAR_FUTURE,
                bad_sig
            ),
            pallet_midds::Error::<Test, Instance>::InvalidSignature
        );
    });
}

// -----------------------------------------------------------------------------
// remove_own_on_behalf
// -----------------------------------------------------------------------------

/// Closes the meta-tx loop: an owner with **no AFT at all** retracts a
/// sponsored record by signing a payload off-chain, while a third-party
/// relayer (≠ sponsor, ≠ owner) submits the extrinsic and pays the
/// fees. Each layer's base returns to its respective payer; the owner's
/// balance never moves.
#[test]
fn remove_own_on_behalf_routes_refunds_per_layer_with_third_party_relayer() {
    new_test_ext().execute_with(|| {
        // 2× M_fast so we can verify the per-layer premium splits cleanly.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::from_u32(2));

        let item = mock_midds(b"meta", 5);
        let base = expected_base_bond_for(&item);
        let total = expected_total_bond_for(&item);
        let premium = total - base;
        assert!(premium > 0);

        // Capture balances BEFORE deposit so the ED/provider-bookkeeping
        // around the first hold is symmetric in both snapshots (the
        // assertions hold equally well whichever way `reducible_balance`
        // accounts for ED — same shape on both sides).
        let bob_free_before = free(BOB);
        let alice_free_before = free(ALICE);
        let charlie_free_before = free(CHARLIE);
        let treasury_before = treasury_balance();

        // Sponsor (BOB) deposits on behalf of owner (ALICE).
        let dep_payload = deposit_payload(&item, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            item.clone(),
            0,
            FAR_FUTURE,
            dep_sig,
        ));

        // Third-party relayer = CHARLIE (different from BOB the sponsor and
        // ALICE the owner). ALICE signs the payload pinning CHARLIE as the
        // submitter.
        let payload = remove_payload(0, CHARLIE, 1, FAR_FUTURE);
        let sig = sign(ALICE, &payload);
        assert_ok!(Midds::remove_own_on_behalf(
            RuntimeOrigin::signed(CHARLIE),
            0,
            1,
            FAR_FUTURE,
            sig,
        ));

        // Sponsor recovered base; their net loss is the multiplier premium
        // they banked at deposit time. Owner balance untouched (they signed
        // off-chain, paid nothing). Relayer balance untouched (mock has no
        // tx fees).
        assert_eq!(held(BOB), 0);
        assert_eq!(free(BOB), bob_free_before - premium);
        assert_eq!(free(ALICE), alice_free_before);
        assert_eq!(free(CHARLIE), charlie_free_before);
        assert_eq!(treasury_balance(), treasury_before + premium);
        // Storage cleared.
        assert!(pallet_midds::Items::<Test, Instance>::get(0).is_none());
        // Nonce bumped on the owner.
        assert_eq!(pallet_midds::OnBehalfNonce::<Test, Instance>::get(ALICE), 2);

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Refunded {
            id: 0,
            depositor: ALICE,
            sponsor: BOB,
            sponsor_refund: base,
            owner_refund: 0,
            premium_to_treasury: premium,
        }));
    });
}

/// After the owner has materialised an `owner_layer` via plain `update`,
/// `remove_own_on_behalf` settles **both** layers — sponsor base back to
/// sponsor, owner base back to owner, premiums to Treasury.
#[test]
fn remove_own_on_behalf_settles_owner_layer_when_present() {
    new_test_ext().execute_with(|| {
        // Capture starting balances so the ED/consumer accounting is
        // symmetric in both snapshots (cf. the relayer test for the same
        // motivation).
        let bob_free_before = free(BOB);
        let alice_free_before = free(ALICE);
        let treasury_before = treasury_balance();

        // Sponsor deposits at unit M (so sponsor_premium = 0 for clarity).
        let initial = mock_midds(b"twolayer", 5);
        let sponsor_base = expected_base_bond_for(&initial);
        let dep_payload = deposit_payload(&initial, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            initial,
            0,
            FAR_FUTURE,
            dep_sig,
        ));

        // Owner extends in solo at 2× M, banking an owner-side premium.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::from_u32(2));
        let bigger = mock_midds(b"twolayer", 12);
        let bigger_total_base = expected_base_bond_for(&bigger);
        let owner_delta_base = bigger_total_base - sponsor_base;
        let owner_amount = owner_delta_base * 2;
        let owner_premium = owner_amount - owner_delta_base;
        assert_ok!(Midds::update(RuntimeOrigin::signed(ALICE), 0, bigger));

        // Reset multipliers to 1× to make sure remove math reads the
        // *banked* per-layer premiums, not the current `M`.
        pallet_midds::FastMultiplier::<Test, Instance>::put(FixedU128::one());

        let payload = remove_payload(0, BOB, 1, FAR_FUTURE);
        let sig = sign(ALICE, &payload);
        assert_ok!(Midds::remove_own_on_behalf(
            RuntimeOrigin::signed(BOB),
            0,
            1,
            FAR_FUTURE,
            sig,
        ));

        // Sponsor: deposit_on_behalf paid `sponsor_base` (unit M, premium=0)
        // and remove returned all of it. Net delta = 0.
        assert_eq!(free(BOB), bob_free_before);
        // Owner: paid `owner_amount` for the solo extend, got back
        // `owner_delta_base`. Net loss = `owner_premium`.
        assert_eq!(free(ALICE), alice_free_before - owner_premium);
        // Treasury collects only the owner-side premium.
        assert_eq!(treasury_balance(), treasury_before + owner_premium);
        // Both holds drained.
        assert_eq!(held(BOB), 0);
        assert_eq!(held(ALICE), 0);

        System::assert_has_event(RuntimeEvent::Midds(pallet_midds::Event::Refunded {
            id: 0,
            depositor: ALICE,
            sponsor: BOB,
            sponsor_refund: sponsor_base,
            owner_refund: owner_delta_base,
            premium_to_treasury: owner_premium,
        }));
    });
}

#[test]
fn remove_own_on_behalf_rejects_invalid_signature() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"badsig", 5);
        let dep_payload = deposit_payload(&item, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            item,
            0,
            FAR_FUTURE,
            dep_sig,
        ));

        // Charlie (not the owner) signs — verification keys to ALICE.
        let payload = remove_payload(0, CHARLIE, 1, FAR_FUTURE);
        let bad_sig = sign(CHARLIE, &payload);
        assert_noop!(
            Midds::remove_own_on_behalf(RuntimeOrigin::signed(CHARLIE), 0, 1, FAR_FUTURE, bad_sig),
            pallet_midds::Error::<Test, Instance>::InvalidSignature,
        );
    });
}

/// `OnBehalfAction::Remove` is part of the SCALE-encoded payload, so a
/// `Deposit`-tagged signature cannot be replayed against a `Remove`
/// extrinsic — the verification recomputes the canonical bytes from the
/// `Remove` discriminant.
#[test]
fn remove_own_on_behalf_rejects_wrong_action_tag() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"crosstag", 5);
        let dep_payload = deposit_payload(&item, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            item.clone(),
            0,
            FAR_FUTURE,
            dep_sig,
        ));

        // Forge a payload that *looks* like a remove (right id/operator/
        // nonce) but is signed under the `Update` action discriminant —
        // which would never validate against the chain's `Remove` payload.
        let forged = update_payload(0, &item, BOB, 1, FAR_FUTURE);
        let sig = sign(ALICE, &forged);
        assert_noop!(
            Midds::remove_own_on_behalf(RuntimeOrigin::signed(BOB), 0, 1, FAR_FUTURE, sig),
            pallet_midds::Error::<Test, Instance>::InvalidSignature,
        );
    });
}

#[test]
fn remove_own_on_behalf_rejects_replay_with_stale_nonce() {
    new_test_ext().execute_with(|| {
        // Two records so the second remove has a target.
        let first = mock_midds(b"replay1", 5);
        let second = mock_midds(b"replay2", 5);
        for (item, n) in [(&first, 0), (&second, 1)] {
            let dep_payload = deposit_payload(item, BOB, n, FAR_FUTURE);
            let dep_sig = sign(ALICE, &dep_payload);
            assert_ok!(Midds::deposit_on_behalf(
                RuntimeOrigin::signed(BOB),
                ALICE,
                item.clone(),
                n,
                FAR_FUTURE,
                dep_sig,
            ));
        }

        // Burn nonce 2 on the first remove.
        let p1 = remove_payload(0, BOB, 2, FAR_FUTURE);
        let s1 = sign(ALICE, &p1);
        assert_ok!(Midds::remove_own_on_behalf(
            RuntimeOrigin::signed(BOB),
            0,
            2,
            FAR_FUTURE,
            s1,
        ));

        // Replay-attempt the *same* nonce on the second record — refused.
        let p2_stale = remove_payload(1, BOB, 2, FAR_FUTURE);
        let s2_stale = sign(ALICE, &p2_stale);
        assert_noop!(
            Midds::remove_own_on_behalf(RuntimeOrigin::signed(BOB), 1, 2, FAR_FUTURE, s2_stale),
            pallet_midds::Error::<Test, Instance>::InvalidNonce,
        );
    });
}

#[test]
fn remove_own_on_behalf_rejects_after_window_closed() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"latebird", 5);
        let dep_payload = deposit_payload(&item, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            item,
            0,
            FAR_FUTURE,
            dep_sig,
        ));

        // Drift past the window so settlement is finalize-only.
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);

        let payload = remove_payload(0, BOB, 1, FAR_FUTURE);
        let sig = sign(ALICE, &payload);
        assert_noop!(
            Midds::remove_own_on_behalf(RuntimeOrigin::signed(BOB), 0, 1, FAR_FUTURE, sig),
            pallet_midds::Error::<Test, Instance>::CommitmentWindowClosed,
        );
        // Nonce must NOT bump on a rejected call.
        assert_eq!(pallet_midds::OnBehalfNonce::<Test, Instance>::get(ALICE), 1);
    });
}

#[test]
fn remove_own_on_behalf_rejects_finalized_record() {
    new_test_ext().execute_with(|| {
        let item = mock_midds(b"final", 5);
        let dep_payload = deposit_payload(&item, BOB, 0, FAR_FUTURE);
        let dep_sig = sign(ALICE, &dep_payload);
        assert_ok!(Midds::deposit_on_behalf(
            RuntimeOrigin::signed(BOB),
            ALICE,
            item,
            0,
            FAR_FUTURE,
            dep_sig,
        ));
        System::set_block_number(System::block_number() + COMMITMENT_WINDOW + 1);
        assert_ok!(Midds::finalize(RuntimeOrigin::signed(BOB), 0));

        let payload = remove_payload(0, BOB, 1, FAR_FUTURE);
        let sig = sign(ALICE, &payload);
        // `!info.finalized` is checked before `ensure_in_window` in the
        // extrinsic, so a finalized post-window record surfaces
        // `AlreadyFinalized` rather than `CommitmentWindowClosed` — even
        // though both conditions hold.
        assert_noop!(
            Midds::remove_own_on_behalf(RuntimeOrigin::signed(BOB), 0, 1, FAR_FUTURE, sig),
            pallet_midds::Error::<Test, Instance>::AlreadyFinalized,
        );
    });
}
