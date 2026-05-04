//! Generic high-level handle for a single `pallet-midds` instance.
//!
//! [`PalletApi`] is parametrised on the MIDDS payload type `M: Midds` and
//! carries the runtime-side instance name + runtime-API trait name as plain
//! strings. The names live at the call site (cf. `MiddsClient::musical_works`)
//! rather than as associated constants on `M`, because the same payload could
//! in principle be wired under a different pallet instance name in a future
//! runtime — the namespace belongs to the runtime, not to the type.

use core::marker::PhantomData;

use midds_traits::{Midds, MiddsId};
use parity_scale_codec::{Decode, Encode};
use subxt::extrinsics::ExtrinsicEvents;
use subxt::tx::Signer;

use crate::{
    Balance, ChainConfig, MiddsClient,
    batch::PreEncodedCalls,
    codec_bridge::EncodedCall,
    error::Error,
    pallet::{
        events::collect_deposit_events,
        names::{
            DEPOSIT_BASE_CONST, DEPOSIT_CALL, DEPOSIT_PER_BYTE_CONST, DEPOSITED_EVENT,
            NEXT_MIDDS_ID_STORAGE,
        },
        types::{DepositInfo, DepositReceipt, FixedU128Raw, PricingSnapshot},
    },
    tx::wait_for_in_block,
};

/// High-level handle for a single `pallet-midds` instance, generic over the
/// MIDDS payload type `M`.
pub struct PalletApi<'a, M: Midds> {
    client: &'a MiddsClient,
    pallet_name: &'static str,
    runtime_api_name: &'static str,
    _m: PhantomData<M>,
}

impl<'a, M: Midds> PalletApi<'a, M> {
    /// Build a handle for a specific instance of `pallet-midds`.
    ///
    /// `pallet_name` matches the runtime's `construct_runtime!` entry (e.g.
    /// `"MusicalWorks"`); `runtime_api_name` is the runtime API trait name
    /// implemented per-instance (e.g. `"MiddsApi"`). Subxt addresses runtime
    /// APIs as `<Trait>_<method>` so the latter stays in lock-step with the
    /// runtime impl.
    pub(crate) fn new(
        client: &'a MiddsClient,
        pallet_name: &'static str,
        runtime_api_name: &'static str,
    ) -> Self {
        Self {
            client,
            pallet_name,
            runtime_api_name,
            _m: PhantomData,
        }
    }

    /// Permissionlessly deposit a new MIDDS record. Returns the on-chain id
    /// extracted from the `Deposited` event.
    pub async fn deposit<S>(&self, signer: &S, item: M) -> Result<MiddsId, Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_inner(signer, item, None).await.map(|r| r.id)
    }

    /// Deposit signing with an explicit nonce instead of letting subxt
    /// resolve it from the latest finalised block.
    ///
    /// Required when a single signer is being drained back-to-back without
    /// waiting for GRANDPA finality between submits: subxt's auto-nonce reads
    /// at the latest *finalised* block, which lags 2-3 blocks behind best,
    /// so the next submit picks up a stale nonce and gets rejected as
    /// `Transaction is outdated`. Callers tracking their own monotonic nonce
    /// counter sidestep that race entirely. See `bench fees` for a worked
    /// example; [`Self::deposit`] keeps the auto-nonce path because the race
    /// only manifests under sequential per-signer submission.
    pub async fn deposit_with_receipt_nonce<S>(
        &self,
        signer: &S,
        item: M,
        nonce: u64,
    ) -> Result<DepositReceipt, Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_inner(signer, item, Some(nonce)).await
    }

    async fn deposit_inner<S>(
        &self,
        signer: &S,
        item: M,
        nonce: Option<u64>,
    ) -> Result<DepositReceipt, Error>
    where
        S: Signer<ChainConfig>,
    {
        item.validate_format()?;

        let payload = subxt::dynamic::tx(self.pallet_name, DEPOSIT_CALL, EncodedCall::one(&item));
        let events = self.submit(signer, payload, nonce).await?;

        let (mut deposited, fee) = collect_deposit_events(&events, self.pallet_name)?;
        // Single-deposit extrinsics emit exactly one `Deposited`. Use `pop`
        // to get the last (and only) entry; mirrors the previous "remember
        // the last `Deposited` we saw" behaviour without the explicit loop.
        let (id, bond, base_bond) = deposited.pop().ok_or(Error::EventNotFound {
            pallet: self.pallet_name,
            variant: DEPOSITED_EVENT,
        })?;
        Ok(DepositReceipt {
            id,
            bond,
            base_bond,
            tx_fee: fee,
        })
    }

    /// Deposit a batch of MIDDS records atomically via `pallet_utility::batch_all`.
    ///
    /// All payloads are validated locally before submission. If any inner
    /// `deposit` fails on-chain (e.g. duplicate identifier), `batch_all` reverts
    /// the whole bundle — partial application is impossible. Returns
    /// successfully when the batch is included in a best block; the caller
    /// can read [`Self::next_midds_id`] to recover the post-batch counter
    /// range. We deliberately do not wait for GRANDPA finalisation — see
    /// the rationale in [`Self::submit`].
    ///
    /// Empty input is a no-op. The intended use is mass-seeding (`midds
    /// seed`) where individual ids aren't needed; for single-deposit flows
    /// that need the allocated id back, prefer [`Self::deposit`].
    pub async fn deposit_batch<S>(&self, signer: &S, items: Vec<M>) -> Result<(), Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_batch_inner(signer, items, None).await?;
        Ok(())
    }

    /// Same as [`Self::deposit_batch`], but signs with an explicit nonce.
    ///
    /// Required when a single signer is being drained back-to-back without
    /// waiting for GRANDPA finality between batches: subxt's auto-nonce
    /// reads at the latest *finalised* block, which lags 2-3 blocks behind
    /// best, so the next batch picks up a stale nonce and gets rejected as
    /// `Transaction is outdated`. Callers tracking their own monotonic
    /// nonce sidestep that race entirely. See `bench seed` for a worked
    /// example; [`Self::deposit_batch`] keeps the auto-nonce path because
    /// the race only manifests under sequential per-signer submission.
    pub async fn deposit_batch_with_nonce<S>(
        &self,
        signer: &S,
        items: Vec<M>,
        nonce: u64,
    ) -> Result<(), Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_batch_inner(signer, items, Some(nonce)).await?;
        Ok(())
    }

    /// Deposit a batch of MIDDS records atomically via `pallet_utility::batch_all`,
    /// returning a per-record [`DepositReceipt`].
    ///
    /// Same wire shape as [`Self::deposit_batch_with_nonce`] but iterates the
    /// inclusion events to surface every inner `Deposited` (id + bond +
    /// base_bond) plus the single outer `TransactionFeePaid`. The fee is
    /// amortised across records (`total_fee / batch_size`, integer
    /// division) — see [`DepositReceipt`] for the exact-vs-amortised
    /// trade-off vs [`Self::deposit_with_receipt_nonce`].
    ///
    /// Empty input is a no-op (returns `Ok(vec![])`). Batches must be small
    /// enough to fit a single block's weight budget; on overflow the runtime
    /// rejects the outer extrinsic and `batch_all` reverts atomically — no
    /// partial application.
    pub async fn deposit_batch_with_receipts_nonce<S>(
        &self,
        signer: &S,
        items: Vec<M>,
        nonce: u64,
    ) -> Result<Vec<DepositReceipt>, Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_batch_inner(signer, items, Some(nonce)).await
    }

    async fn deposit_batch_inner<S>(
        &self,
        signer: &S,
        items: Vec<M>,
        nonce: Option<u64>,
    ) -> Result<Vec<DepositReceipt>, Error>
    where
        S: Signer<ChainConfig>,
    {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        for item in &items {
            item.validate_format()?;
        }

        let at_block = self.client.inner.at_current_block().await?;
        let tx_client = at_block.transactions();
        let mut inner_calls: Vec<Vec<u8>> = Vec::with_capacity(items.len());
        for item in &items {
            let inner = subxt::dynamic::tx(self.pallet_name, DEPOSIT_CALL, EncodedCall::one(item));
            // `call_data` resolves pallet/call indices via metadata and
            // returns the canonical SCALE-encoded `RuntimeCall` bytes — the
            // wire shape `Vec<RuntimeCall>` expects per element.
            inner_calls.push(tx_client.call_data(&inner)?);
        }
        let payload = subxt::dynamic::tx(
            "Utility",
            "batch_all",
            EncodedCall::one(&PreEncodedCalls::new(inner_calls)),
        );

        // `submit` provides at-block retry on transient prune errors and the
        // same inclusion-wait policy as the single-deposit path. The event
        // stream contains one `Deposited` per inner deposit plus the single
        // outer `TransactionFeePaid` (event order matches inner-call order,
        // which subxt preserves).
        let events = self.submit(signer, payload, nonce).await?;

        let expected = items.len();
        let (deposited, total_fee) = collect_deposit_events(&events, self.pallet_name)?;
        if deposited.len() != expected {
            return Err(Error::EventNotFound {
                pallet: self.pallet_name,
                variant: DEPOSITED_EVENT,
            });
        }
        // Integer division loses up to (records-1) plancks of total fee per
        // batch — negligible on a 12-decimal chain (sub-pico-token rounding)
        // and identical for every record in the batch by construction.
        let amortised_fee = total_fee.map(|fee| fee / expected as Balance);
        Ok(deposited
            .into_iter()
            .map(|(id, bond, base_bond)| DepositReceipt {
                id,
                bond,
                base_bond,
                tx_fee: amortised_fee,
            })
            .collect())
    }

    /// Read the pallet-config `DepositBase` and `DepositPerByte` constants
    /// straight off the runtime metadata at the current block.
    ///
    /// Returned as `(base, per_byte)`, both denominated in the chain's
    /// `Balance` planck units. Useful for callers that need to mirror the
    /// on-chain bond formula `base + per_byte * encoded_size` without
    /// hardcoding values that vary between testnet (`melodie`) and any
    /// future runtime variants.
    pub async fn deposit_constants(&self) -> Result<(Balance, Balance), Error> {
        let at_block = self.client.inner.at_current_block().await?;
        let consts = at_block.constants();
        // Constants are decoded via `DecodeAsType` against the runtime
        // metadata, so the same code works whether the runtime declares the
        // balance as `u128` or some narrower alias — as long as the wire
        // shape matches `Balance`.
        let base_addr = subxt::dynamic::constant::<Balance>(self.pallet_name, DEPOSIT_BASE_CONST);
        let per_byte_addr =
            subxt::dynamic::constant::<Balance>(self.pallet_name, DEPOSIT_PER_BYTE_CONST);
        let base = consts.entry(base_addr)?;
        let per_byte = consts.entry(per_byte_addr)?;
        Ok((base, per_byte))
    }

    /// Read the per-instance `NextMiddsId` counter via dynamic storage.
    ///
    /// On a fresh chain the storage entry is empty; FRAME's `ValueQuery`
    /// default (encoded `0u64`) is returned by subxt automatically.
    pub async fn next_midds_id(&self) -> Result<MiddsId, Error> {
        let address: subxt::storage::DynamicAddress =
            subxt::dynamic::storage(self.pallet_name, NEXT_MIDDS_ID_STORAGE);
        let at_block = self.client.inner.at_current_block().await?;
        let value = at_block.storage().fetch(address, Vec::new()).await?;
        let bytes = value.bytes();
        Ok(MiddsId::decode(&mut &bytes[..])?)
    }

    /// Quote the bond a fresh `deposit(item)` of `size` SCALE-encoded bytes
    /// would lock at the current block, multipliers included.
    ///
    /// Mirrors `MiddsApi::current_deposit_price`. Use this for pre-flight
    /// quoting and for sizing fund transfers — `deposit_constants()` only
    /// returns the unmultiplied base, which under-prices any block where
    /// `M_fast × M_slow > 1`.
    pub async fn current_deposit_price(&self, size: u32) -> Result<Balance, Error> {
        self.runtime_api("current_deposit_price", &size.encode())
            .await
    }

    /// Read `(M_fast, M_slow)` at the current block as raw [`FixedU128Raw`]
    /// values (`value × 10^18`). Use [`fixed_u128_to_f64`](super::types::fixed_u128_to_f64) for display.
    pub async fn current_multipliers(&self) -> Result<(FixedU128Raw, FixedU128Raw), Error> {
        self.runtime_api("current_multipliers", &[]).await
    }

    /// Static target for the rolling 7-day window — runtime parameter, not
    /// on-chain state, but exposed for symmetry with `weekly_actual` so the
    /// CLI can render a load gauge (`actual / target`).
    pub async fn weekly_target(&self) -> Result<u32, Error> {
        self.runtime_api("weekly_target", &[]).await
    }

    /// Sum of the 7 daily buckets — actual deposits seen in the last 7 days
    /// at day-resolution.
    pub async fn weekly_actual(&self) -> Result<u32, Error> {
        self.runtime_api("weekly_actual", &[]).await
    }

    /// One-shot snapshot of the dynamic-pricing inputs. Two RPC round-trips
    /// (multipliers + weekly load) at a single point in time, useful for
    /// stamping a benchmark report header without coordinating four separate
    /// awaits at the call site.
    pub async fn pricing_snapshot(&self) -> Result<PricingSnapshot, Error> {
        let (fast_multiplier, slow_multiplier) = self.current_multipliers().await?;
        let weekly_target = self.weekly_target().await?;
        let weekly_actual = self.weekly_actual().await?;
        Ok(PricingSnapshot {
            fast_multiplier,
            slow_multiplier,
            weekly_target,
            weekly_actual,
        })
    }

    /// Bond information attached to a stored MIDDS record. `None` if no
    /// record exists at this id.
    pub async fn deposit_info(&self, id: MiddsId) -> Result<Option<DepositInfo>, Error> {
        self.runtime_api::<Option<DepositInfo>>("deposit_info", &id.encode())
            .await
    }

    /// All `MiddsId`s registered against the canonical industry identifier
    /// (multi-claim — several records may share the same identifier with
    /// different payloads). Returns an empty vector when nothing matches.
    pub async fn lookup_by_identifier(
        &self,
        identifier: &M::Identifier,
    ) -> Result<Vec<MiddsId>, Error> {
        self.runtime_api("lookup_by_identifier", &identifier.encode())
            .await
    }

    /// Fetch a stored MIDDS record by its on-chain id.
    pub async fn get(&self, id: MiddsId) -> Result<Option<M>, Error> {
        self.runtime_api("get", &id.encode()).await
    }

    /// Call a `<RuntimeApi>_<method>` runtime API and SCALE-decode the response
    /// into `T`. Runtime API responses are plain SCALE so any `Decode` type
    /// (including tuples and `Option<...>`) round-trips through here.
    async fn runtime_api<T: Decode>(&self, method: &str, args: &[u8]) -> Result<T, Error> {
        let at_block = self.client.inner.at_current_block().await?;
        let function = format!("{}_{method}", self.runtime_api_name);
        let bytes = at_block
            .runtime_apis()
            .call_raw(&function, Some(args))
            .await?;
        Ok(T::decode(&mut &bytes[..])?)
    }

    async fn submit<S, P>(
        &self,
        signer: &S,
        payload: P,
        nonce: Option<u64>,
    ) -> Result<ExtrinsicEvents<ChainConfig>, Error>
    where
        S: Signer<ChainConfig>,
        P: subxt::tx::Payload,
    {
        // Subxt 0.50 routes every tx through `at_current_block()`, which pins
        // the operation to a specific block hash. Under heavy concurrent load
        // the node prunes that block (or replies with a stale handle whose
        // header is no longer in the cache) before we get to sign+submit, and
        // the call fails with a `BlockHeaderNotFound`-style error. The fix is
        // to retry with a fresh `at_current_block()`: re-fetching naturally
        // captures a newer block ref and the second attempt usually goes
        // through. We only retry up to the point of submission — once the tx
        // is in flight, retrying would risk a duplicate. See sibling
        // `transient_at_block_error` for the exact match heuristic.
        const MAX_PREP_RETRIES: u32 = 8;
        let progress = {
            let mut attempt: u32 = 0;
            loop {
                let result = async {
                    let at_block = self.client.inner.at_current_block().await?;
                    let mut tx_client = at_block.transactions();
                    match nonce {
                        Some(n) => {
                            // Caller is tracking the nonce themselves —
                            // override subxt's auto-fetch so back-to-back
                            // sequential submits don't pick up a stale value
                            // from the lagging finalised state.
                            let params =
                                subxt::config::DefaultExtrinsicParamsBuilder::<ChainConfig>::new()
                                    .nonce(n)
                                    .build();
                            tx_client
                                .sign_and_submit_then_watch(&payload, signer, params)
                                .await
                                .map_err(Error::from)
                        }
                        None => tx_client
                            .sign_and_submit_then_watch_default(&payload, signer)
                            .await
                            .map_err(Error::from),
                    }
                }
                .await;
                match result {
                    Ok(progress) => break progress,
                    Err(e) if attempt < MAX_PREP_RETRIES && transient_at_block_error(&e) => {
                        attempt += 1;
                        // Exponential-ish backoff: 50, 100, 200, 400 ms, capped
                        // — prevents a thundering herd from re-stampeding the
                        // same already-pruned block.
                        let backoff_ms = 50_u64 * (1 << attempt.min(3));
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        // Wait for inclusion (best block) instead of finalisation: GRANDPA
        // finality lags 2-3 blocks behind best, so `wait_for_finalized` adds
        // 12-18 s per submit for no measurement gain — `TransactionFeePaid`,
        // `Deposited`, and any pallet error are all emitted at inclusion. The
        // shorter pin window also reduces the chance the in-block hash gets
        // unpinned by the chainHead backend before we fetch events. The
        // tradeoff is theoretical fork resistance, which doesn't apply on
        // single-authority dev nodes (best == finalized in practice) and is
        // tolerable on testnets given that this SDK is operator/test tooling
        // rather than a production submission path.
        let in_block = wait_for_in_block(progress).await?;
        let events = in_block.wait_for_success().await?;
        Ok(events)
    }
}

/// True if the error is a transient `OnlineClientAtBlockError` we know how to
/// recover from by re-fetching the at-block handle.
///
/// Two variants surface in practice under high concurrency:
/// - `BlockHeaderNotFound` — the block hash captured by `at_current_block()`
///   was pruned from the node's cache before we got around to using it.
/// - `CannotGetBlockHeader` — the header fetch RPC returned an error (often a
///   side-effect of the same backend pruning under load).
///
/// Matching on the rendered message keeps us decoupled from subxt's internal
/// error layout, which has multiple wrapping levels (`subxt::Error` →
/// `ExtrinsicError` → `OnlineClientAtBlockError`) and changes between
/// minor releases.
fn transient_at_block_error(e: &Error) -> bool {
    let msg = format!("{e}");
    msg.contains("cannot find the block header") || msg.contains("cannot get the block header")
}

#[cfg(test)]
mod generic_check {
    //! Compile-only proof that [`PalletApi`] is generic over any `Midds`
    //! payload — not just `MusicalWork`. If anyone reintroduces a
    //! `MusicalWork`-specific bound or path on `PalletApi` itself, this stops
    //! compiling. The dummy type doesn't need to be runtime-shaped; it only
    //! needs to satisfy `Midds: Parameter + MaxEncodedLen`.
    use super::*;
    use midds_traits::MiddsFormatError;
    use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
    use scale_info::TypeInfo;

    #[derive(
        Clone, Eq, PartialEq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
    )]
    #[allow(dead_code)]
    struct DummyMidds(u32);

    impl Midds for DummyMidds {
        const KIND: &'static str = "Dummy";
        type Identifier = u32;
        fn identifier(&self) -> &Self::Identifier {
            &self.0
        }
        fn validate_format(&self) -> Result<(), MiddsFormatError> {
            Ok(())
        }
    }

    fn _assert_generic_compiles<'a>(c: &'a MiddsClient) {
        let _: PalletApi<'a, DummyMidds> = PalletApi::new(c, "Dummy", "MiddsApi");
    }
}
