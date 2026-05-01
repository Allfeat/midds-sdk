//! JSON-RPC bridge exposing [`midds_runtime_api::MiddsApi`] to clients.
//!
//! A single generic handler serves any `pallet-midds` instance — node code
//! instantiates [`MiddsRpc`] once per instance and registers the resulting
//! module under the namespace it wants.
//!
//! V2 surface (cf. `docs/economics.md` §12.2) adds live pricing endpoints
//! (`midds_currentDepositPrice`, `midds_currentMultipliers`, weekly load
//! gauge) on top of the V1 lookups.
//!
//! # Multi-instance namespacing
//!
//! `#[rpc(server)]` hardcodes the method names that the macro emits. With
//! one instance this is fine; once the node hosts several instances
//! (`MusicalWorks`, `Recordings`, `Releases`, …) every instance would emit
//! the same `midds_*` method names and `RpcModule::merge` would refuse the
//! collision. The integrating node is expected to renamespace each
//! instance's module before merging — for example by extracting the methods
//! and re-registering them as `midds_<instance>_*`. V1 ships a single
//! instance so the renaming is intentionally left to the node.

use std::{marker::PhantomData, sync::Arc};

use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
    types::{ErrorObject, ErrorObjectOwned},
};
use midds_runtime_api::MiddsApi;
use midds_traits::MiddsId;
use parity_scale_codec::Codec;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_runtime::{FixedU128, traits::Block as BlockT};

/// `(depositor, total_held, base_bond, finalized)` — wire shape for
/// `midds_depositInfo`. Mirrors the runtime API tuple exactly so the bridge
/// stays a pure passthrough.
pub type DepositInfoView<AccountId, Balance> = (AccountId, Balance, Balance, bool);

/// JSON-RPC surface for one MIDDS pallet instance.
#[rpc(server)]
pub trait MiddsRpcApi<BlockHash, Identifier, Item, AccountId, Balance> {
    /// All `MiddsId`s registered against the canonical industry identifier.
    /// Returns an empty list when nothing matches.
    #[method(name = "midds_lookupByIdentifier")]
    fn lookup_by_identifier(
        &self,
        identifier: Identifier,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<MiddsId>>;

    /// Fetch a stored MIDDS record by its on-chain id.
    #[method(name = "midds_get")]
    fn get(&self, id: MiddsId, at: Option<BlockHash>) -> RpcResult<Option<Item>>;

    /// Bond information `(depositor, total_held, base_bond, finalized)`
    /// attached to a stored record.
    #[method(name = "midds_depositInfo")]
    fn deposit_info(
        &self,
        id: MiddsId,
        at: Option<BlockHash>,
    ) -> RpcResult<Option<DepositInfoView<AccountId, Balance>>>;

    /// Quote the bond a fresh `deposit(item)` of `size` SCALE-encoded bytes
    /// would lock at the queried block.
    #[method(name = "midds_currentDepositPrice")]
    fn current_deposit_price(&self, size: u32, at: Option<BlockHash>) -> RpcResult<Balance>;

    /// `(M_fast, M_slow)` at the queried block.
    #[method(name = "midds_currentMultipliers")]
    fn current_multipliers(&self, at: Option<BlockHash>) -> RpcResult<(FixedU128, FixedU128)>;

    /// Static target for the rolling 7-day window.
    #[method(name = "midds_weeklyTarget")]
    fn weekly_target(&self, at: Option<BlockHash>) -> RpcResult<u32>;

    /// Sum of the 7 daily buckets — actual deposits seen in the last 7 days
    /// at day-resolution.
    #[method(name = "midds_weeklyActual")]
    fn weekly_actual(&self, at: Option<BlockHash>) -> RpcResult<u32>;
}

/// Numeric error codes surfaced to JSON-RPC clients.
#[repr(i32)]
pub enum Error {
    /// The runtime call returned an error.
    RuntimeError = 1,
}

impl From<Error> for i32 {
    fn from(e: Error) -> i32 {
        e as i32
    }
}

/// Generic RPC handler bridging a single `pallet-midds` instance to clients.
pub struct MiddsRpc<Client, Block, Identifier, Item, AccountId, Balance> {
    client: Arc<Client>,
    _marker: PhantomData<(Block, Identifier, Item, AccountId, Balance)>,
}

impl<Client, Block, Identifier, Item, AccountId, Balance>
    MiddsRpc<Client, Block, Identifier, Item, AccountId, Balance>
{
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            _marker: PhantomData,
        }
    }
}

impl<Client, Block, Identifier, Item, AccountId, Balance>
    MiddsRpcApiServer<<Block as BlockT>::Hash, Identifier, Item, AccountId, Balance>
    for MiddsRpc<Client, Block, Identifier, Item, AccountId, Balance>
where
    Block: BlockT,
    Client: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
    Client::Api: MiddsApi<Block, Identifier, Item, AccountId, Balance>,
    Identifier: Codec + Send + Sync + 'static,
    Item: Codec + Send + Sync + 'static,
    AccountId: Codec + Send + Sync + 'static,
    Balance: Codec + Send + Sync + 'static,
{
    fn lookup_by_identifier(
        &self,
        identifier: Identifier,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<MiddsId>> {
        let api = self.client.runtime_api();
        let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);
        api.lookup_by_identifier(at_hash, identifier)
            .map_err(|e| runtime_err(e, "Unable to resolve identifier."))
    }

    fn get(&self, id: MiddsId, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Option<Item>> {
        let api = self.client.runtime_api();
        let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);
        api.get(at_hash, id)
            .map_err(|e| runtime_err(e, "Unable to fetch MIDDS record."))
    }

    fn deposit_info(
        &self,
        id: MiddsId,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Option<DepositInfoView<AccountId, Balance>>> {
        let api = self.client.runtime_api();
        let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);
        api.deposit_info(at_hash, id)
            .map_err(|e| runtime_err(e, "Unable to fetch deposit info."))
    }

    fn current_deposit_price(
        &self,
        size: u32,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Balance> {
        let api = self.client.runtime_api();
        let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);
        api.current_deposit_price(at_hash, size)
            .map_err(|e| runtime_err(e, "Unable to compute deposit price."))
    }

    fn current_multipliers(
        &self,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<(FixedU128, FixedU128)> {
        let api = self.client.runtime_api();
        let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);
        api.current_multipliers(at_hash)
            .map_err(|e| runtime_err(e, "Unable to fetch current multipliers."))
    }

    fn weekly_target(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<u32> {
        let api = self.client.runtime_api();
        let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);
        api.weekly_target(at_hash)
            .map_err(|e| runtime_err(e, "Unable to fetch weekly target."))
    }

    fn weekly_actual(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<u32> {
        let api = self.client.runtime_api();
        let at_hash = at.unwrap_or_else(|| self.client.info().best_hash);
        api.weekly_actual(at_hash)
            .map_err(|e| runtime_err(e, "Unable to fetch weekly actual."))
    }
}

fn runtime_err(e: impl ToString, msg: &'static str) -> ErrorObjectOwned {
    ErrorObject::owned(Error::RuntimeError.into(), msg, Some(e.to_string()))
}
