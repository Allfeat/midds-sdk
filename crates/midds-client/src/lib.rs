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
pub mod pallet;
pub mod recordings;
pub mod releases;
pub mod tx;

pub use error::Error;
pub use musical_works::MusicalWorksApi;
pub use pallet::{
    DepositInfo, DepositReceipt, FixedU128Raw, PalletApi, PricingSnapshot, fixed_u128_to_f64,
};
pub use recordings::RecordingsApi;
pub use releases::ReleasesApi;
pub use tx::wait_for_in_block;

pub use subxt;
pub use subxt_signer;

use subxt::OnlineClient;
use subxt::client::ClientAtBlock;
use subxt::config::HashFor;
use subxt::rpcs::RpcClient;
use subxt::rpcs::client::rpc_params;

/// Default subxt `Config` used for Allfeat. Allfeat is a Substrate-based chain
/// without Polkadot-specific extensions, so [`subxt::SubstrateConfig`] applies.
pub type ChainConfig = subxt::SubstrateConfig;

/// Balance type carried by the chain. Allfeat uses `u128` like most Substrate
/// runtimes.
pub type Balance = u128;

/// High-level wrapper around a subxt [`OnlineClient`] talking to an Allfeat node.
///
/// Internally pairs an [`OnlineClient`] with the raw [`RpcClient`] it was
/// built from, so we can resolve the latest **best** block hash (one
/// `chain_getBlockHash` round-trip) and pin reads/submits to it via
/// [`Self::at_best_block`]. Subxt's own `at_current_block` always returns
/// the **finalised** block, which lags 2-3 blocks behind best on Substrate
/// chains and produces a stale view of nonces and storage after a
/// `wait_for_in_block`-style deposit — see the rationale on
/// [`Self::at_best_block`].
#[derive(Clone)]
pub struct MiddsClient {
    inner: OnlineClient<ChainConfig>,
    rpc: RpcClient,
}

impl MiddsClient {
    /// Connect to an Allfeat node via WS or HTTP URL.
    pub async fn connect(url: impl AsRef<str>) -> Result<Self, Error> {
        // Build the raw RPC client first so we can share it between the
        // typed [`OnlineClient`] and our own best-block helper. The clone
        // is cheap — `RpcClient` wraps an `Arc<dyn RpcClientT>`.
        let rpc = RpcClient::from_insecure_url(url.as_ref()).await?;
        let inner = OnlineClient::from_rpc_client(rpc.clone()).await?;
        Ok(Self { inner, rpc })
    }

    /// Borrow the underlying subxt client for advanced operations not yet
    /// covered by the typed façade.
    pub fn inner(&self) -> &OnlineClient<ChainConfig> {
        &self.inner
    }

    /// Pin a [`ClientAtBlock`] to the node's latest **best** block — the
    /// canonical read point inside the SDK.
    ///
    /// `at_current_block` on subxt's [`OnlineClient`] pins to the latest
    /// **finalised** block, which lags ~2-3 blocks behind best on
    /// Substrate chains. That produces two symmetric foot-guns when
    /// pairing reads with `wait_for_in_block`-style deposits (the SDK's
    /// default — see `wait_for_in_block` for the rationale):
    ///
    /// - **Storage reads miss the deposit.** A `lookup_by_identifier` or
    ///   `get` issued right after `deposit` would query at a block that
    ///   doesn't yet contain the new record and return an empty/`None`
    ///   answer.
    /// - **Auto-nonce reads a stale value.** Back-to-back submits from
    ///   the same signer would pick up the depositor's pre-deposit nonce
    ///   and the second tx would be rejected as
    ///   `Transaction is outdated`.
    ///
    /// Reading at best (computed via a single `chain_getBlockHash`
    /// round-trip + `at_block`) fixes both: nonces and storage reflect
    /// every extrinsic that has hit a block, finalised or not. On dev
    /// chains there are no reorgs; on production chains the worst case
    /// is a one-block view that gets corrected on the next read.
    pub async fn at_best_block(
        &self,
    ) -> Result<
        ClientAtBlock<ChainConfig, subxt::client::OnlineClientAtBlockImpl<ChainConfig>>,
        Error,
    > {
        // `chain_getBlockHash(None)` returns the hash of the latest best
        // block (see `subxt_rpcs::methods::legacy::chain_get_block_hash`).
        // We deserialise directly into the chain's hash type and skip the
        // typed `LegacyRpcMethods` wrapper to avoid pulling in its config
        // generic at every call site.
        let best_hash: Option<HashFor<ChainConfig>> = self
            .rpc
            .request("chain_getBlockHash", rpc_params![])
            .await?;
        let best_hash = best_hash.ok_or(Error::NoBestBlock)?;
        Ok(self.inner.at_block(best_hash).await?)
    }

    /// Façade for the `MusicalWorks` pallet instance.
    pub fn musical_works(&self) -> MusicalWorksApi<'_> {
        PalletApi::new(
            self,
            musical_works::PALLET_NAME,
            musical_works::RUNTIME_API_NAME,
        )
    }

    /// Façade for the `Recordings` pallet instance.
    ///
    /// Type-checked and ready, but the `Recordings` instance is not yet wired
    /// in the `../Allfeat` runtime — calls will fail at runtime against a
    /// node that lacks it. See [`crate::recordings`] for the rationale.
    pub fn recordings(&self) -> RecordingsApi<'_> {
        PalletApi::new(self, recordings::PALLET_NAME, recordings::RUNTIME_API_NAME)
    }

    /// Façade for the `Releases` pallet instance.
    ///
    /// Type-checked and ready, but the `Releases` instance is not yet wired
    /// in the `../Allfeat` runtime — calls will fail at runtime against a
    /// node that lacks it. See [`crate::releases`] for the rationale.
    pub fn releases(&self) -> ReleasesApi<'_> {
        PalletApi::new(self, releases::PALLET_NAME, releases::RUNTIME_API_NAME)
    }

    /// Read the current nonce for `account_id` at the latest best block.
    ///
    /// Used by callers that drive multiple sequential submits per signer
    /// without waiting for finalisation in between (see
    /// [`MusicalWorksApi::deposit_with_receipt_nonce`]). The read is pinned
    /// to the best block via [`Self::at_best_block`] so callers see a
    /// nonce that already accounts for any deposit `wait_for_in_block` has
    /// just confirmed — finalised reads would lag by 2-3 blocks and trip
    /// `Transaction is outdated` on the next submit.
    pub async fn account_nonce(
        &self,
        account_id: &<ChainConfig as subxt::Config>::AccountId,
    ) -> Result<u64, Error> {
        let at_block = self.at_best_block().await?;
        let nonce = at_block.transactions().account_nonce(account_id).await?;
        Ok(nonce)
    }
}
