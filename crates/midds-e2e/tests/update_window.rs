//! `update` inside the commitment window changes the payload in place,
//! preserves the canonical identifier, and the runtime API sees the new
//! bytes.
//!
//! Validates the `update` extrinsic path that `midds-client` does not yet
//! wrap — the test drives it via `subxt::dynamic` through our `tx`
//! helper, so a regression in the extrinsic's metadata signature (a new
//! argument, a renamed field) trips the helper instead of going unnoticed.

use midds_e2e::{client, poll, session, signer, tx};
use midds_traits::Midds;

#[tokio::test]
async fn update_within_window_rewrites_payload() {
    let Some(client) = client::try_connect().await else {
        return;
    };
    let alice = signer::alice();

    let initial = session::fresh_musical_work();
    let identifier = initial.identifier().clone();
    let mut replacement = session::fresh_musical_work();
    {
        let midds_types::MusicalWork::V1(v1) = &mut replacement;
        v1.iswc = identifier.clone();
    }
    assert_ne!(initial, replacement, "fixture must produce distinct bodies");

    let id = client
        .musical_works()
        .deposit(&alice, initial.clone())
        .await
        .expect("deposit");

    client::wait_for_visible_musical_works_deposit(&client, id).await;

    tx::update_musical_work(&client, &alice, id, &replacement)
        .await
        .expect("update inside commitment window must succeed");

    let stored = poll::wait_until_some("update visible at finalised block", || async {
        client
            .musical_works()
            .get(id)
            .await
            .expect("runtime API get")
            .filter(|v| v == &replacement)
    })
    .await;
    assert_eq!(stored, replacement, "update must overwrite the payload");
    assert_eq!(
        stored.identifier(),
        &identifier,
        "update must preserve the canonical identifier",
    );
}
