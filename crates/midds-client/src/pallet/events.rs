//! Event-stream helpers for the deposit path. Shared by single-deposit and
//! batch flows; parametrised on the pallet name so the same code applies to
//! every `pallet-midds` instance.

use midds_traits::MiddsId;
use parity_scale_codec::Decode;
use subxt::extrinsics::ExtrinsicEvents;

use crate::{
    Balance, ChainConfig,
    error::Error,
    pallet::{
        names::{DEPOSITED_EVENT, TX_FEE_PAID_EVENT, TX_PAYMENT_PALLET},
        types::DepositedEvent,
    },
};

/// Walk an extrinsic's event stream and surface every inner `Deposited`
/// plus the optional outer `TransactionFeePaid`.
///
/// Shared by single-deposit and batch paths — the only thing they disagree
/// on is the expected `Deposited` count, so that check stays at the call
/// site. Decodes that fail to match the expected event shape are ignored
/// (rather than propagated) so a future runtime extension that adds an
/// extra trailing field doesn't poison the receipt path.
pub(crate) fn collect_deposit_events(
    events: &ExtrinsicEvents<ChainConfig>,
    pallet_name: &str,
) -> Result<(Vec<DepositedEvent>, Option<Balance>), Error> {
    let mut deposited: Vec<DepositedEvent> = Vec::new();
    let mut fee: Option<Balance> = None;
    for event in events.iter() {
        let event = event?;
        match (event.pallet_name(), event.event_name()) {
            (p, DEPOSITED_EVENT) if p == pallet_name => {
                if let Some(parsed) = decode_deposited(event.field_bytes()) {
                    deposited.push(parsed);
                }
            }
            (TX_PAYMENT_PALLET, TX_FEE_PAID_EVENT) => {
                if let Some(paid) = decode_fee_paid(event.field_bytes()) {
                    fee = Some(paid);
                }
            }
            _ => {}
        }
    }
    Ok((deposited, fee))
}

/// Decode the actual fee out of a `TransactionPayment::TransactionFeePaid`
/// event payload. Wire shape is `(AccountId, Balance, Balance)` — payer
/// account, `actual_fee`, then `tip`. We `Decode` the account properly
/// (rather than skipping a hardcoded byte stride) so a future
/// `ChainConfig::AccountId` change surfaces as a decode error instead of a
/// silent misalignment of the following balance.
fn decode_fee_paid(bytes: &[u8]) -> Option<Balance> {
    let mut cursor = bytes;
    <ChainConfig as subxt::Config>::AccountId::decode(&mut cursor).ok()?;
    Balance::decode(&mut cursor).ok()
}

/// Decode the `Deposited { id, depositor, bond_payer, bond, base_bond }`
/// event payload. Wire shape is `(MiddsId, AccountId, AccountId, Balance,
/// Balance)`. Both account fields are dropped on the floor (the caller is
/// the submitting signer, the sponsor is the same for self-deposits and
/// already known by the operator for sponsored deposits) but still
/// `Decode`d to advance the cursor (cf. [`decode_fee_paid`] for the
/// rationale).
fn decode_deposited(bytes: &[u8]) -> Option<(MiddsId, Balance, Balance)> {
    let mut cursor = bytes;
    let id = MiddsId::decode(&mut cursor).ok()?;
    <ChainConfig as subxt::Config>::AccountId::decode(&mut cursor).ok()?;
    <ChainConfig as subxt::Config>::AccountId::decode(&mut cursor).ok()?;
    let bond = Balance::decode(&mut cursor).ok()?;
    let base_bond = Balance::decode(&mut cursor).ok()?;
    Some((id, bond, base_bond))
}
