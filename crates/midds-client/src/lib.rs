// `subxt::Error` is ~216B; boxing it would change the public Error API.
// Tracked: revisit when the client error surface is next refactored.
#![allow(clippy::result_large_err)]

//! High-level Rust client for the Allfeat MIDDS pallets.
//!
//! Wraps `subxt` with a typed façade per pallet instance. V1 covers the
//! `MusicalWorks` instance end-to-end (`deposit`, `update`, runtime-API
//! queries). The same pattern extends to future `Recording` and `Release`
//! instances by adding more sub-APIs.
//!
//! # Quickstart
//!
//! ```no_run
//! use midds_client::MiddsClient;
//! use midds_types::MusicalWork;
//! use subxt_signer::sr25519::dev;
//!
//! # async fn run() -> Result<(), midds_client::Error> {
//! let client = MiddsClient::connect("ws://localhost:9944").await?;
//! let alice = dev::alice();
//!
//! # let work: MusicalWork = unimplemented!();
//! let id = client.musical_works().deposit(&alice, work).await?;
//! println!("registered id = {id}");
//! # Ok(())
//! # }
//! ```

pub mod batch;
pub mod codec_bridge;
pub mod error;
pub mod musical_works;
pub mod tx;

pub use error::Error;
pub use musical_works::{
    DepositInfo, DepositReceipt, FixedU128Raw, MusicalWorksApi, PricingSnapshot, fixed_u128_to_f64,
};
pub use tx::wait_for_in_block;

pub use subxt;
pub use subxt_signer;

use subxt::OnlineClient;

/// Default subxt `Config` used for Allfeat. Allfeat is a Substrate-based chain
/// without Polkadot-specific extensions, so [`subxt::SubstrateConfig`] applies.
pub type ChainConfig = subxt::SubstrateConfig;

/// Balance type carried by the chain. Allfeat uses `u128` like most Substrate
/// runtimes.
pub type Balance = u128;

/// High-level wrapper around a subxt [`OnlineClient`] talking to an Allfeat node.
#[derive(Clone)]
pub struct MiddsClient {
    inner: OnlineClient<ChainConfig>,
}

impl MiddsClient {
    /// Connect to an Allfeat node via WS or HTTP URL.
    pub async fn connect(url: impl AsRef<str>) -> Result<Self, Error> {
        let inner = OnlineClient::from_url(url.as_ref()).await?;
        Ok(Self { inner })
    }

    /// Borrow the underlying subxt client for advanced operations not yet
    /// covered by the typed façade.
    pub fn inner(&self) -> &OnlineClient<ChainConfig> {
        &self.inner
    }

    /// Façade for the `MusicalWorks` pallet instance.
    pub fn musical_works(&self) -> MusicalWorksApi<'_> {
        MusicalWorksApi::new(self)
    }

    /// Read the current nonce for `account_id` at the latest finalised block.
    ///
    /// Used by callers that drive multiple sequential submits per signer
    /// without waiting for finalisation in between (see
    /// [`MusicalWorksApi::deposit_with_receipt_nonce`]). Fetched once per
    /// signer at the start of a run; the caller increments locally on each
    /// successful submit and only re-fetches if a submit fails — that's
    /// strictly cheaper than letting subxt re-resolve the nonce per call,
    /// and the value is correct because subxt also reads from finalised
    /// state (so it would observe the same pre-tx counter).
    pub async fn account_nonce(
        &self,
        account_id: &<ChainConfig as subxt::Config>::AccountId,
    ) -> Result<u64, Error> {
        let at_block = self.inner.at_current_block().await?;
        let nonce = at_block.transactions().account_nonce(account_id).await?;
        Ok(nonce)
    }
}
