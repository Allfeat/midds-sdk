//! Lock the SCALE wire shape of `DepositInfoOf` after the tuple →
//! struct migration. The migration is intentionally backward-incompatible;
//! this test verifies the new layout round-trips cleanly so an accidental
//! field reordering is caught at CI time, not after a runtime upgrade.

use midds_runtime_api::DepositInfoOf;
use parity_scale_codec::{Decode, Encode};

#[test]
fn deposit_info_round_trips_through_scale() {
    let original = DepositInfoOf::<u64, u128> {
        depositor: 12_345,
        bond_payer: 67_890,
        amount: 1_000_000,
        base_bond: 750_000,
        finalized: false,
    };
    let bytes = original.encode();
    let decoded =
        DepositInfoOf::<u64, u128>::decode(&mut &bytes[..]).expect("SCALE decode round-trip");
    assert_eq!(decoded, original);
}

#[test]
fn deposit_info_field_order_pinned() {
    // SCALE encoding is field-order sensitive: depositor || bond_payer ||
    // amount || base_bond || finalized. Pin the prefix so a reorder breaks
    // the test loudly.
    let info = DepositInfoOf::<u8, u8> {
        depositor: 0xAA,
        bond_payer: 0xAB,
        amount: 0xBB,
        base_bond: 0xCC,
        finalized: true,
    };
    let bytes = info.encode();
    assert_eq!(bytes, vec![0xAA, 0xAB, 0xBB, 0xCC, 0x01]);
}

#[test]
fn deposit_info_finalized_false_encodes_as_zero_byte() {
    let info = DepositInfoOf::<u8, u8> {
        depositor: 0,
        bond_payer: 0,
        amount: 0,
        base_bond: 0,
        finalized: false,
    };
    assert_eq!(info.encode().last().copied(), Some(0));
}
