//! Lock the SCALE wire shape of `DepositInfoOf` after the stratified-bond
//! migration. Changing the layout (e.g. reordering fields, dropping
//! `owner_layer`) is intentionally backward-incompatible; this test
//! verifies the new shape round-trips cleanly so an accidental field
//! reordering is caught at CI time, not after a runtime upgrade.

use midds_runtime_api::{BondLayerOf, DepositInfoOf};
use parity_scale_codec::{Decode, Encode};

#[test]
fn deposit_info_round_trips_through_scale() {
    let original = DepositInfoOf::<u64, u128> {
        depositor: 12_345,
        sponsor_layer: BondLayerOf {
            payer: 67_890,
            amount: 1_000_000,
            base: 750_000,
        },
        owner_layer: Some(BondLayerOf {
            payer: 12_345,
            amount: 250_000,
            base: 200_000,
        }),
        finalized: false,
    };
    let bytes = original.encode();
    let decoded =
        DepositInfoOf::<u64, u128>::decode(&mut &bytes[..]).expect("SCALE decode round-trip");
    assert_eq!(decoded, original);
}

#[test]
fn deposit_info_field_order_pinned() {
    // SCALE encoding is field-order sensitive: depositor || sponsor_layer
    // (payer || amount || base) || owner_layer (Option<…>) || finalized.
    // Pin the byte stream so a reorder breaks the test loudly.
    let info = DepositInfoOf::<u8, u8> {
        depositor: 0xAA,
        sponsor_layer: BondLayerOf {
            payer: 0xBB,
            amount: 0xCC,
            base: 0xDD,
        },
        owner_layer: None,
        finalized: true,
    };
    // depositor=0xAA | sponsor.payer=0xBB | sponsor.amount=0xCC |
    // sponsor.base=0xDD | owner_layer=None=0x00 | finalized=true=0x01
    assert_eq!(info.encode(), vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x01]);
}

#[test]
fn deposit_info_with_owner_layer_some_encodes_layer_after_tag() {
    // Option<T> in SCALE: prefix 0x01 then the inner SCALE-encoded T.
    let info = DepositInfoOf::<u8, u8> {
        depositor: 0x00,
        sponsor_layer: BondLayerOf {
            payer: 0x00,
            amount: 0x00,
            base: 0x00,
        },
        owner_layer: Some(BondLayerOf {
            payer: 0xEE,
            amount: 0xFE,
            base: 0xFD,
        }),
        finalized: false,
    };
    let bytes = info.encode();
    // depositor=0x00 | sponsor (3 zero bytes) | Option tag 0x01 | owner
    // (0xEE 0xFE 0xFD) | finalized=0x00.
    assert_eq!(
        bytes,
        vec![0x00, 0x00, 0x00, 0x00, 0x01, 0xEE, 0xFE, 0xFD, 0x00]
    );
}

#[test]
fn deposit_info_finalized_false_encodes_as_zero_byte() {
    let info = DepositInfoOf::<u8, u8> {
        depositor: 0,
        sponsor_layer: BondLayerOf {
            payer: 0,
            amount: 0,
            base: 0,
        },
        owner_layer: None,
        finalized: false,
    };
    assert_eq!(info.encode().last().copied(), Some(0));
}
