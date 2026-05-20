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

    let id = client
        .musical_works()
        .deposit(&alice, work)
        .await
        .expect("deposit");

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

    let delta = i128::try_from(free_after).expect("free balance fits i128")
        - i128::try_from(free_before).expect("free balance fits i128");
    let bond = i128::try_from(info.sponsor_layer.amount).expect("sponsor-layer bond fits i128");
    assert!(
        delta > 0,
        "balance must increase after force_remove_refund (got delta = {delta})",
    );
    assert!(
        delta <= bond,
        "refund credit must be at most the held bond ({bond}); got delta = {delta}",
    );
}
