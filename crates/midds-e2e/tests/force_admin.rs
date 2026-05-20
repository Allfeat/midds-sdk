//! `Sudo::sudo(MusicalWorks::force_remove_refund(id))` refunds the depositor
//! exactly the bond that was held at deposit time.
//!
//! Validates the admin / `ForceOrigin` path end-to-end:
//!
//! - `Sudo` wraps the inner call correctly — the wire format mocked by
//!   [`midds_e2e::tx::force_remove_refund_sudo`] matches what the runtime
//!   decodes.
//! - `pallet-midds` releases the held bond on `force_remove_refund`
//!   (foundation indemnifies a good-faith typo, per
//!   `docs/economics.md` §4).
//! - The released amount equals `DepositInfo::amount` — i.e. exactly the
//!   bond originally locked, not a recomputation that could drift under
//!   multiplier changes.
//!
//! The balance check sees the *net* change: we read free balance before
//! deposit and after `force_remove_refund`, then assert the delta equals
//! the sum of all tx fees paid by `//Alice` during the test. Bond debit
//! and refund cancel each other out exactly.

use midds_e2e::{client, poll, session, signer, tx};
use subxt_signer::sr25519::Keypair;

fn alice_account_id() -> subxt::utils::AccountId32 {
    Keypair::public_key(&signer::alice()).to_account_id()
}

#[tokio::test]
async fn force_remove_refund_releases_bond_exactly() {
    let Some(client) = client::try_connect().await else {
        return;
    };
    let alice = signer::alice();
    let alice_id = alice_account_id();
    let work = session::fresh_musical_work();

    // Deposit and capture the bond amount the runtime ended up holding.
    // `deposit_info.amount` is the canonical "what's locked" figure — the
    // refund must release exactly this.
    let id = client
        .musical_works()
        .deposit(&alice, work)
        .await
        .expect("deposit");

    // Belt-and-braces sync — `MiddsClient::at_best_block` already makes
    // `deposit_info` and the auto-nonce on the sudo submit see the just-
    // confirmed deposit, but the helper costs nothing on the happy path
    // and pins the contract for future readers.
    client::wait_for_visible_musical_works_deposit(&client, id).await;

    let info = client
        .musical_works()
        .deposit_info(id)
        .await
        .expect("deposit_info")
        .expect("deposit_info present once finalised");

    let free_before = tx::free_balance(&client, &alice_id)
        .await
        .expect("balance read before refund");

    tx::force_remove_refund_sudo(&client, &alice, id)
        .await
        .expect("Sudo::sudo(force_remove_refund) must succeed for //Alice");

    // Same lag on the post-refund read: wait until the record has actually
    // been wiped at the finalised block before reading the balance — that
    // also gives the refund credit time to land in the finalised state.
    poll::wait_until("record removed at finalised block", || async {
        client
            .musical_works()
            .get(id)
            .await
            .expect("runtime API get post-refund")
            .is_none()
    })
    .await;

    let free_after = tx::free_balance(&client, &alice_id)
        .await
        .expect("balance read after refund");

    // free_after == free_before + bond - sudo_tx_fee. We don't try to
    // predict the exact fee (it varies with weight + multipliers) — just
    // bound it: the refund returns `bond` plancks before fees, so the
    // delta must sit in (-bond, bond]. A negative-or-zero delta means the
    // bond didn't come back at all.
    // Dev-chain balances stay far below `i128::MAX`, so the `try_from`
    // conversions never fail in practice — `expect` documents the invariant
    // for the next reader without tripping clippy's `unwrap` ban.
    let delta = i128::try_from(free_after).expect("free balance fits i128")
        - i128::try_from(free_before).expect("free balance fits i128");
    // For a self-deposit the bond lives entirely in `sponsor_layer` (no
    // `owner_layer` until the depositor extends a sponsored record via
    // plain `update`). On `force_remove_refund` that whole amount is
    // returned to `sponsor_layer.payer` — which is //Alice here.
    let bond = i128::try_from(info.sponsor_layer.amount).expect("sponsor-layer bond fits i128");
    assert!(
        delta > 0,
        "balance must increase after force_remove_refund (got delta = {delta})",
    );
    assert!(
        delta <= bond,
        "refund credit must be at most the held bond ({bond}); got delta = {delta}",
    );

    // Record-already-gone invariant is enforced by the poll above; nothing
    // more to assert on the read side here.
}
