//! Raw subxt helpers for chain interactions not (yet) wrapped by
//! [`midds_client::MiddsClient`].
//!
//! `midds-client` v0.1 only exposes `deposit` / `deposit_batch` and the
//! read-side runtime API. The e2e suite still needs `update`, `force_*` and
//! balance reads to exercise the full inter-crate seam, so we drive those
//! through `subxt::dynamic` here — same pattern as `midds-client`'s
//! `pallet::api`, no static codegen, names resolved through metadata.
//!
//! When `midds-client` grows typed wrappers for these extrinsics this module
//! collapses to a thin re-export.

use anyhow::{Result, anyhow};
use midds_client::{MiddsClient, codec_bridge::EncodedCall, wait_for_in_block};
use midds_traits::MiddsId;
use midds_types::MusicalWork;
use parity_scale_codec::{Decode, Encode, Output};
use subxt::utils::AccountId32;
use subxt_signer::sr25519::Keypair;

/// SCALE-encode shim that writes its inner bytes verbatim.
///
/// `Sudo::sudo(call: RuntimeCall)` carries a nested call in canonical
/// `pallet_index ++ call_index ++ args` form. `tx_client.call_data()` gives
/// us those bytes already; wrapping them in [`RawCall`] feeds them into
/// [`EncodedCall::one`] without `parity_scale_codec` adding a length prefix.
struct RawCall(Vec<u8>);

impl Encode for RawCall {
    fn encode_to<O: Output + ?Sized>(&self, out: &mut O) {
        out.write(&self.0);
    }
}

/// `update(id, item)` on the `MusicalWorks` instance. Caller must be the
/// original depositor and still within the pallet's `CommitmentWindow`.
pub async fn update_musical_work(
    client: &MiddsClient,
    signer: &Keypair,
    id: MiddsId,
    item: &MusicalWork,
) -> Result<()> {
    let payload = subxt::dynamic::tx(
        midds_client::musical_works::PALLET_NAME,
        "update",
        EncodedCall::two(&id, item),
    );
    let at = client.at_best_block().await?;
    let progress = at
        .transactions()
        .sign_and_submit_then_watch_default(&payload, signer)
        .await?;
    let in_block = wait_for_in_block(progress).await?;
    in_block.wait_for_success().await?;
    Ok(())
}

/// `Sudo::sudo(MusicalWorks::force_remove_refund(id))` — `ForceOrigin`
/// removal that refunds the held bond in full.
pub async fn force_remove_refund_sudo(
    client: &MiddsClient,
    sudo: &Keypair,
    id: MiddsId,
) -> Result<()> {
    let at = client.at_best_block().await?;
    let mut tx_client = at.transactions();

    let inner = subxt::dynamic::tx(
        midds_client::musical_works::PALLET_NAME,
        "force_remove_refund",
        EncodedCall::one(&id),
    );
    let inner_bytes = tx_client
        .call_data(&inner)
        .map_err(|e| anyhow!("encode inner force_remove_refund: {e}"))?;

    let payload = subxt::dynamic::tx("Sudo", "sudo", EncodedCall::one(&RawCall(inner_bytes)));
    let progress = tx_client
        .sign_and_submit_then_watch_default(&payload, sudo)
        .await?;
    let in_block = wait_for_in_block(progress).await?;
    in_block.wait_for_success().await?;
    Ok(())
}

/// Pin of `frame_system::AccountInfo<Nonce=u32, AccountData<Balance=u128>>`
/// as the melodie runtime stores it. The fields we don't read are still
/// decoded so the cursor advances — but they get an underscore prefix so the
/// `dead_code` lint stays quiet.
#[derive(Decode)]
struct AccountInfoPin {
    _nonce: u32,
    _consumers: u32,
    _providers: u32,
    _sufficients: u32,
    data: AccountDataPin,
}

#[derive(Decode)]
struct AccountDataPin {
    free: u128,
    _reserved: u128,
    _frozen: u128,
    _flags: u128,
}

/// Free balance of `account` at the latest block — the amount the owner can
/// freely spend or have held against new operations.
///
/// Decodes the crate-internal `AccountInfoPin` from the raw storage bytes.
/// If the runtime ever
/// migrates `AccountData` to a different layout the decode fails loudly,
/// surfacing the regression in the e2e suite rather than silently returning
/// a wrong number.
pub async fn free_balance(client: &MiddsClient, account: &AccountId32) -> Result<u128> {
    let addr: subxt::storage::DynamicAddress = subxt::dynamic::storage("System", "Account");
    let at = client.at_best_block().await?;
    let value = at
        .storage()
        .fetch(addr, vec![subxt::dynamic::Value::from_bytes(account.0)])
        .await?;
    let bytes = value.bytes();
    let info =
        AccountInfoPin::decode(&mut &bytes[..]).map_err(|e| anyhow!("decode AccountInfo: {e}"))?;
    Ok(info.data.free)
}
