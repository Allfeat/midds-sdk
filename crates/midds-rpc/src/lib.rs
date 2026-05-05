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
//! `#[rpc(server)]` hardcodes the method names that the macro emits, so
//! merging two [`MiddsRpc`] instances under the same node (e.g.
//! `MusicalWorks` + `Recordings`) would collide on the canonical
//! `midds_*` names. The default [`MiddsRpcApiServer`] impl emits those
//! names for V1 — a single instance — and the node sidesteps collisions
//! by wrapping each instance with its own thin `#[rpc(server)]` trait
//! that picks a prefixed namespace and delegates to the inherent
//! handlers ([`MiddsRpc::lookup_by_identifier_at`], etc.).
//!
//! ```ignore
//! use std::sync::Arc;
//! use jsonrpsee::{core::RpcResult, proc_macros::rpc};
//! use midds_rpc::{MiddsRpc, DepositInfoOf};
//!
//! #[rpc(server)]
//! pub trait MusicalWorksRpcApi<BlockHash, Identifier, Item, AccountId, Balance> {
//!     #[method(name = "midds_musicalWorks_lookupByIdentifier")]
//!     fn lookup_by_identifier(
//!         &self,
//!         identifier: Identifier,
//!         at: Option<BlockHash>,
//!     ) -> RpcResult<Vec<midds_traits::MiddsId>>;
//!     // ...one method per RPC, prefixed with the instance namespace.
//! }
//!
//! pub struct MusicalWorksRpc<C, B, I, M, A, Bal>(pub Arc<MiddsRpc<C, B, I, M, A, Bal>>);
//!
//! // The trait impl forwards every call to the inherent `*_at` handler —
//! // no logic duplication.
//! ```
//!
//! Recordings / Releases follow the same template. The midds-rpc crate
//! stays agnostic of the pallet-instance topology, and the node controls
//! the namespace exclusively. Inherent handlers are public so wrappers
//! never need to round-trip through trait dispatch.

use std::{marker::PhantomData, sync::Arc};

use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
    types::{ErrorObject, ErrorObjectOwned},
};
use midds_runtime_api::MiddsApi;
pub use midds_runtime_api::{BondLayerOf, DepositInfoOf};
use midds_traits::MiddsId;
use parity_scale_codec::Codec;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_runtime::{FixedU128, traits::Block as BlockT};

/// JSON-RPC surface for one MIDDS pallet instance.
#[rpc(server)]
pub trait MiddsRpcApi<BlockHash, Identifier, Item, AccountId, Balance> {
    /// First page of `MiddsId`s registered against the canonical
    /// industry identifier, sorted ascending and capped at the pallet's
    /// `MAX_LOOKUP_LIMIT`. Use `midds_lookupByIdentifierPaged` to walk
    /// past the cap.
    #[method(name = "midds_lookupByIdentifier")]
    fn lookup_by_identifier(
        &self,
        identifier: Identifier,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<MiddsId>>;

    /// Paginated variant of `lookup_by_identifier`: returns ids strictly
    /// greater than `after` (or the head of the list when `after` is
    /// `None`), sorted ascending, capped at the pallet's lookup limit.
    /// The next page resumes with the last id of the returned vector.
    #[method(name = "midds_lookupByIdentifierPaged")]
    fn lookup_by_identifier_paged(
        &self,
        identifier: Identifier,
        after: Option<MiddsId>,
        limit: u32,
        at: Option<BlockHash>,
    ) -> RpcResult<Vec<MiddsId>>;

    /// Total number of `MiddsId`s registered against the canonical
    /// industry identifier — useful for UIs that need to surface "X
    /// claims" alongside a paginated list.
    #[method(name = "midds_countByIdentifier")]
    fn count_by_identifier(&self, identifier: Identifier, at: Option<BlockHash>) -> RpcResult<u32>;

    /// Fetch a stored MIDDS record by its on-chain id.
    #[method(name = "midds_get")]
    fn get(&self, id: MiddsId, at: Option<BlockHash>) -> RpcResult<Option<Item>>;

    /// Bond information attached to a stored record. See
    /// [`midds_runtime_api::DepositInfoOf`] for the field layout.
    #[method(name = "midds_depositInfo")]
    fn deposit_info(
        &self,
        id: MiddsId,
        at: Option<BlockHash>,
    ) -> RpcResult<Option<DepositInfoOf<AccountId, Balance>>>;

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

/// Inherent (unprefixed) handlers — node-side wrappers can call these
/// directly to wire a per-instance RPC namespace without going through
/// the canonical `MiddsRpcApiServer` trait. Each method mirrors a
/// runtime-API call; the only adjustment is the friendly error message
/// surfaced to clients.
impl<Client, Block, Identifier, Item, AccountId, Balance>
    MiddsRpc<Client, Block, Identifier, Item, AccountId, Balance>
where
    Block: BlockT,
    Client: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
    Client::Api: MiddsApi<Block, Identifier, Item, AccountId, Balance>,
    Identifier: Codec + Send + Sync + 'static,
    Item: Codec + Send + Sync + 'static,
    AccountId: Codec + Send + Sync + 'static,
    Balance: Codec + Send + Sync + 'static,
{
    fn at_hash(&self, at: Option<<Block as BlockT>::Hash>) -> <Block as BlockT>::Hash {
        at.unwrap_or_else(|| self.client.info().best_hash)
    }

    pub fn lookup_by_identifier_at(
        &self,
        identifier: Identifier,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<MiddsId>> {
        self.client
            .runtime_api()
            .lookup_by_identifier(self.at_hash(at), identifier)
            .map_err(|e| runtime_err(e, "Unable to resolve identifier."))
    }

    pub fn lookup_by_identifier_paged_at(
        &self,
        identifier: Identifier,
        after: Option<MiddsId>,
        limit: u32,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<MiddsId>> {
        self.client
            .runtime_api()
            .lookup_by_identifier_paged(self.at_hash(at), identifier, after, limit)
            .map_err(|e| runtime_err(e, "Unable to resolve identifier page."))
    }

    pub fn count_by_identifier_at(
        &self,
        identifier: Identifier,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<u32> {
        self.client
            .runtime_api()
            .count_by_identifier(self.at_hash(at), identifier)
            .map_err(|e| runtime_err(e, "Unable to count claims for identifier."))
    }

    pub fn get_at(
        &self,
        id: MiddsId,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Option<Item>> {
        self.client
            .runtime_api()
            .get(self.at_hash(at), id)
            .map_err(|e| runtime_err(e, "Unable to fetch MIDDS record."))
    }

    pub fn deposit_info_at(
        &self,
        id: MiddsId,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Option<DepositInfoOf<AccountId, Balance>>> {
        self.client
            .runtime_api()
            .deposit_info(self.at_hash(at), id)
            .map_err(|e| runtime_err(e, "Unable to fetch deposit info."))
    }

    pub fn current_deposit_price_at(
        &self,
        size: u32,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Balance> {
        self.client
            .runtime_api()
            .current_deposit_price(self.at_hash(at), size)
            .map_err(|e| runtime_err(e, "Unable to compute deposit price."))
    }

    pub fn current_multipliers_at(
        &self,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<(FixedU128, FixedU128)> {
        self.client
            .runtime_api()
            .current_multipliers(self.at_hash(at))
            .map_err(|e| runtime_err(e, "Unable to fetch current multipliers."))
    }

    pub fn weekly_target_at(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<u32> {
        self.client
            .runtime_api()
            .weekly_target(self.at_hash(at))
            .map_err(|e| runtime_err(e, "Unable to fetch weekly target."))
    }

    pub fn weekly_actual_at(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<u32> {
        self.client
            .runtime_api()
            .weekly_actual(self.at_hash(at))
            .map_err(|e| runtime_err(e, "Unable to fetch weekly actual."))
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
        self.lookup_by_identifier_at(identifier, at)
    }

    fn lookup_by_identifier_paged(
        &self,
        identifier: Identifier,
        after: Option<MiddsId>,
        limit: u32,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Vec<MiddsId>> {
        self.lookup_by_identifier_paged_at(identifier, after, limit, at)
    }

    fn count_by_identifier(
        &self,
        identifier: Identifier,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<u32> {
        self.count_by_identifier_at(identifier, at)
    }

    fn get(&self, id: MiddsId, at: Option<<Block as BlockT>::Hash>) -> RpcResult<Option<Item>> {
        self.get_at(id, at)
    }

    fn deposit_info(
        &self,
        id: MiddsId,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Option<DepositInfoOf<AccountId, Balance>>> {
        self.deposit_info_at(id, at)
    }

    fn current_deposit_price(
        &self,
        size: u32,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<Balance> {
        self.current_deposit_price_at(size, at)
    }

    fn current_multipliers(
        &self,
        at: Option<<Block as BlockT>::Hash>,
    ) -> RpcResult<(FixedU128, FixedU128)> {
        self.current_multipliers_at(at)
    }

    fn weekly_target(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<u32> {
        self.weekly_target_at(at)
    }

    fn weekly_actual(&self, at: Option<<Block as BlockT>::Hash>) -> RpcResult<u32> {
        self.weekly_actual_at(at)
    }
}

fn runtime_err(e: impl ToString, msg: &'static str) -> ErrorObjectOwned {
    ErrorObject::owned(Error::RuntimeError.into(), msg, Some(e.to_string()))
}
