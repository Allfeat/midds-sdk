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
    assert_eq!(info.encode(), vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x01]);
}

#[test]
fn deposit_info_with_owner_layer_some_encodes_layer_after_tag() {
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
