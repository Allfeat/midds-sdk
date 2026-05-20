//! Pivot test of the SDK: deposit → identifier lookup → record fetch round-trip.
//!
//! Validates the canonical inter-crate seam:
//!
//! - `midds-types` SCALE-encodes the payload the way `pallet-midds` decodes
//!   it (otherwise the deposit would error out at format validation or codec
//!   level).
//! - `pallet-midds` registers the canonical identifier in the right index
//!   (otherwise `lookup_by_identifier` would miss the freshly deposited id).
//! - `midds-runtime-api` exposes `get` returning bytes that decode back into
//!   the exact same `MusicalWork` value we submitted.
//! - `midds-client` drives all of the above through `subxt::dynamic` and
//!   surfaces the new `MiddsId` from the deposit event.
//!
//! A green run means every one of those crates speaks the same wire-shape.

use midds_e2e::{client, poll, session, signer};
use midds_traits::Midds;

#[tokio::test]
async fn deposit_then_lookup_then_get_roundtrips() {
    let Some(client) = client::try_connect().await else {
        return;
    };
    let alice = signer::alice();
    let work = session::fresh_musical_work();
    let identifier = work.identifier().clone();

    let id = client
        .musical_works()
        .deposit(&alice, work.clone())
        .await
        .expect("deposit must succeed against a clean dev node");

    let ids = poll::wait_until_some("identifier index update", || async {
        let v = client
            .musical_works()
            .lookup_by_identifier(&identifier)
            .await
            .expect("lookup_by_identifier");
        v.contains(&id).then_some(v)
    })
    .await;
    assert!(
        ids.contains(&id),
        "identifier index missing id {id} (got {ids:?})"
    );

    let stored = client
        .musical_works()
        .get(id)
        .await
        .expect("runtime API get")
        .expect("record exists at the id we just deposited");
    assert_eq!(
        stored, work,
        "stored MusicalWork must equal the deposited payload"
    );
}
