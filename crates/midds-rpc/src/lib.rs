//! JSON-RPC bridges exposing the `pallet-midds` runtime APIs to clients.
//!
//! # One handler per MIDDS kind, one macro body
//!
//! Each MIDDS instance has its own runtime-API trait
//! ([`midds_runtime_api::MusicalWorkApi`], [`midds_runtime_api::RecordingApi`],
//! …) because Substrate keys runtime-API dispatch on the trait name. The
//! JSON-RPC surface mirrors that one-trait-per-kind shape: the
//! [`midds_rpc_instance!`] macro stamps a generic handler + a
//! `#[rpc(server)]` trait per kind from a single body, so there is exactly
//! one copy of the bridging logic.
//!
//! # Symmetric, namespaced method names
//!
//! `#[rpc(server)]` hardcodes the emitted method names, so two handlers
//! merged under one node must not collide. Rather than leave one kind on the
//! bare `midds_*` names and prefix only the others, every kind is namespaced
//! **symmetrically** via jsonrpsee's `namespace`:
//!
//! - `MusicalWorkRpc` → `midds_musicalWorks_lookupByIdentifier`, `_get`, …
//! - `RecordingRpc`   → `midds_recordings_lookupByIdentifier`, `_get`, …
//!
//! This is a deliberate break from the pre-multi-instance `midds_*` names:
//! consumers (the explorer, wallets) move to the prefixed names in lockstep.
//! The node merges the per-kind modules; the namespace keeps them disjoint.
//!
//! Inherent `*_at` handlers are public so a node that wants a bespoke
//! namespace can wire its own thin `#[rpc(server)]` trait without
//! round-tripping through trait dispatch.

use std::{marker::PhantomData, sync::Arc};

use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
    types::{ErrorObject, ErrorObjectOwned},
};
pub use midds_runtime_api::{BondLayerOf, DepositInfoOf};
use midds_runtime_api::{MusicalWorkApi, RecordingApi};
use midds_traits::MiddsId;
use parity_scale_codec::Codec;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::HeaderBackend;
use sp_runtime::{FixedU128, traits::Block as BlockT};

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

fn runtime_err(e: impl ToString, msg: &'static str) -> ErrorObjectOwned {
    ErrorObject::owned(Error::RuntimeError.into(), msg, Some(e.to_string()))
}

/// Stamp a complete JSON-RPC bridge for one `pallet-midds` instance.
///
/// `$Handler` is the generic handler struct; `$Api` is the `#[rpc(server)]`
/// trait and `$ApiServer` the trait jsonrpsee derives from it (by
/// convention `${Api}Server` — spelled explicitly since `macro_rules!`
/// cannot concatenate idents and a `paste` dep would be overkill for two
/// call sites); `$RtApi` is the per-kind runtime-API trait the handler
/// calls into; `$ns` is the jsonrpsee namespace, so every method is
/// published as `${ns}_<camelCaseName>`.
///
/// Generated per kind: the namespaced server trait, the handler struct +
/// `new`, the public inherent `*_at` methods (so node-side wrappers can
/// bypass trait dispatch), and the `$ApiServer` impl delegating to them.
/// The bridging logic exists once, here.
macro_rules! midds_rpc_instance {
    (
        $(#[$hattr:meta])*
        handler: $Handler:ident,
        server: $Api:ident,
        server_trait: $ApiServer:ident,
        runtime_api: $RtApi:ident,
        namespace: $ns:literal $(,)?
    ) => {
        #[doc = concat!(
            "JSON-RPC surface for the `", stringify!($Handler),
            "` instance — methods published under the `", $ns, "_` namespace."
        )]
        #[rpc(server, namespace = $ns)]
        pub trait $Api<BlockHash, Identifier, Item, AccountId, Balance> {
            /// First page of `MiddsId`s registered against the canonical
            /// industry identifier, sorted ascending and capped at the
            /// pallet's `MAX_LOOKUP_LIMIT`. Use `lookupByIdentifierPaged`
            /// to walk past the cap.
            #[method(name = "lookupByIdentifier")]
            fn lookup_by_identifier(
                &self,
                identifier: Identifier,
                at: Option<BlockHash>,
            ) -> RpcResult<Vec<MiddsId>>;

            /// Paginated variant of `lookupByIdentifier`: ids strictly
            /// greater than `after` (or the head when `after` is `None`),
            /// sorted ascending, capped at the pallet's lookup limit. The
            /// next page resumes with the last id of the returned vector.
            #[method(name = "lookupByIdentifierPaged")]
            fn lookup_by_identifier_paged(
                &self,
                identifier: Identifier,
                after: Option<MiddsId>,
                limit: u32,
                at: Option<BlockHash>,
            ) -> RpcResult<Vec<MiddsId>>;

            /// Total number of `MiddsId`s registered against the canonical
            /// industry identifier.
            #[method(name = "countByIdentifier")]
            fn count_by_identifier(
                &self,
                identifier: Identifier,
                at: Option<BlockHash>,
            ) -> RpcResult<u32>;

            /// Fetch a stored MIDDS record by its on-chain id.
            #[method(name = "get")]
            fn get(&self, id: MiddsId, at: Option<BlockHash>) -> RpcResult<Option<Item>>;

            /// Bond information attached to a stored record. See
            /// [`midds_runtime_api::DepositInfoOf`] for the field layout.
            #[method(name = "depositInfo")]
            fn deposit_info(
                &self,
                id: MiddsId,
                at: Option<BlockHash>,
            ) -> RpcResult<Option<DepositInfoOf<AccountId, Balance>>>;

            /// Quote the bond a fresh `deposit(item)` of `size`
            /// SCALE-encoded bytes would lock at the queried block.
            #[method(name = "currentDepositPrice")]
            fn current_deposit_price(
                &self,
                size: u32,
                at: Option<BlockHash>,
            ) -> RpcResult<Balance>;

            /// `(M_fast, M_slow)` at the queried block.
            #[method(name = "currentMultipliers")]
            fn current_multipliers(
                &self,
                at: Option<BlockHash>,
            ) -> RpcResult<(FixedU128, FixedU128)>;

            /// Static target for the rolling 7-day window.
            #[method(name = "weeklyTarget")]
            fn weekly_target(&self, at: Option<BlockHash>) -> RpcResult<u32>;

            /// Sum of the 7 daily buckets — actual deposits seen in the
            /// last 7 days at day-resolution.
            #[method(name = "weeklyActual")]
            fn weekly_actual(&self, at: Option<BlockHash>) -> RpcResult<u32>;
        }

        $(#[$hattr])*
        pub struct $Handler<Client, Block, Identifier, Item, AccountId, Balance> {
            client: Arc<Client>,
            _marker: PhantomData<(Block, Identifier, Item, AccountId, Balance)>,
        }

        impl<Client, Block, Identifier, Item, AccountId, Balance>
            $Handler<Client, Block, Identifier, Item, AccountId, Balance>
        {
            pub fn new(client: Arc<Client>) -> Self {
                Self {
                    client,
                    _marker: PhantomData,
                }
            }
        }

        /// Inherent (un-prefixed) handlers — node-side wrappers can call
        /// these directly to wire a bespoke namespace without going through
        /// the generated server trait.
        impl<Client, Block, Identifier, Item, AccountId, Balance>
            $Handler<Client, Block, Identifier, Item, AccountId, Balance>
        where
            Block: BlockT,
            Client: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
            Client::Api: $RtApi<Block, Identifier, Item, AccountId, Balance>,
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

            pub fn weekly_target_at(
                &self,
                at: Option<<Block as BlockT>::Hash>,
            ) -> RpcResult<u32> {
                self.client
                    .runtime_api()
                    .weekly_target(self.at_hash(at))
                    .map_err(|e| runtime_err(e, "Unable to fetch weekly target."))
            }

            pub fn weekly_actual_at(
                &self,
                at: Option<<Block as BlockT>::Hash>,
            ) -> RpcResult<u32> {
                self.client
                    .runtime_api()
                    .weekly_actual(self.at_hash(at))
                    .map_err(|e| runtime_err(e, "Unable to fetch weekly actual."))
            }
        }

        impl<Client, Block, Identifier, Item, AccountId, Balance>
            $ApiServer<<Block as BlockT>::Hash, Identifier, Item, AccountId, Balance>
            for $Handler<Client, Block, Identifier, Item, AccountId, Balance>
        where
            Block: BlockT,
            Client: ProvideRuntimeApi<Block> + HeaderBackend<Block> + Send + Sync + 'static,
            Client::Api: $RtApi<Block, Identifier, Item, AccountId, Balance>,
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

            fn get(
                &self,
                id: MiddsId,
                at: Option<<Block as BlockT>::Hash>,
            ) -> RpcResult<Option<Item>> {
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
    };
}

midds_rpc_instance! {
    /// JSON-RPC handler for the `MusicalWork` `pallet-midds` instance.
    /// Methods are published under the `midds_musicalWorks_` namespace.
    handler: MusicalWorkRpc,
    server: MusicalWorkRpcApi,
    server_trait: MusicalWorkRpcApiServer,
    runtime_api: MusicalWorkApi,
    namespace: "midds_musicalWorks",
}

midds_rpc_instance! {
    /// JSON-RPC handler for the `Recording` `pallet-midds` instance.
    /// Methods are published under the `midds_recordings_` namespace.
    handler: RecordingRpc,
    server: RecordingRpcApi,
    server_trait: RecordingRpcApiServer,
    runtime_api: RecordingApi,
    namespace: "midds_recordings",
}
