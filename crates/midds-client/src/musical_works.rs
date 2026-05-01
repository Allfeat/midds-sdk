//! Typed façade for the `MusicalWorks` instance of `pallet-midds`.
//!
//! Wraps the on-chain operations the SDK actually consumes today (`deposit`
//! single + batch, plus reads of the `NextMiddsId` storage and the
//! `DepositBase` / `DepositPerByte` constants used by `midds-cli bench`).

use midds_traits::{Midds, MiddsId};
use midds_types::MusicalWork;
use parity_scale_codec::{Decode, Encode};
use subxt::extrinsics::ExtrinsicEvents;
use subxt::tx::Signer;

use crate::{
    Balance, ChainConfig, MiddsClient, batch::PreEncodedCalls, codec_bridge::EncodedCall,
    error::Error, tx::wait_for_in_block,
};

/// Name of the `pallet-midds` instance dedicated to musical works.
pub const PALLET_NAME: &str = "MusicalWorks";

/// Runtime API trait name implemented for this instance in `melodie-runtime`
/// (`impl midds_runtime_api::MiddsApi<...> for Runtime`). Subxt addresses
/// runtime APIs as `<Trait>_<method>` so this stays in lock-step with the
/// runtime impl.
pub const RUNTIME_API_NAME: &str = "MiddsApi";

const DEPOSITED_EVENT: &str = "Deposited";
const TX_PAYMENT_PALLET: &str = "TransactionPayment";
const TX_FEE_PAID_EVENT: &str = "TransactionFeePaid";

/// Inner representation of `sp_runtime::FixedU128`: a `u128` with 18 decimal
/// places of fixed-point precision. We surface the raw integer because
/// `midds-client` does not depend on `sp-runtime`; consumers convert via
/// [`fixed_u128_to_f64`] when display precision is enough.
pub type FixedU128Raw = u128;

/// FixedU128 accuracy — `10^18`. Matches `sp_runtime::FixedU128::DIV`.
const FIXED_U128_ACCURACY: u128 = 1_000_000_000_000_000_000;

/// Convert a raw FixedU128 (`value * 10^18`) into the floating-point ratio it
/// represents. Lossy for values needing more than ~15 decimal digits, but
/// sufficient for displaying multiplier ratios on a CLI dashboard.
pub fn fixed_u128_to_f64(raw: FixedU128Raw) -> f64 {
    raw as f64 / FIXED_U128_ACCURACY as f64
}

/// Receipt for a single deposited MIDDS record.
///
/// Bundles the allocated id with the on-chain bond breakdown (extracted
/// from the `Deposited` event so callers don't re-derive it from runtime
/// constants and current multipliers) and the inclusion fee paid.
///
/// The `tx_fee` semantics depend on the producing call:
/// - [`MusicalWorksApi::deposit_with_receipt_nonce`] — exact fee, taken
///   straight from the single `TransactionFeePaid` event.
/// - [`MusicalWorksApi::deposit_batch_with_receipts_nonce`] — the
///   per-record share of the outer batch's `TransactionFeePaid` value
///   (`total_batch_fee / batch_size`, integer division). Identical for
///   every record in the batch by construction;
///   `TransactionPayment` only emits one event per outer extrinsic.
///
/// The other fields (`bond`, `base_bond`) are taken from per-inner
/// `Deposited` events and are exact in both cases.
#[derive(Debug, Clone, Copy)]
pub struct DepositReceipt {
    /// On-chain id allocated by the pallet.
    pub id: MiddsId,
    /// Total bond placed on hold against the depositor (`base_bond ×
    /// M_fast × M_slow`). This is the amount [`MusicalWorksApi`] callers
    /// should use when reasoning about the user-facing cost of a deposit.
    pub bond: Balance,
    /// Unmultiplied portion of the bond. `remove_own` refunds this exact
    /// value; the difference `bond − base_bond` is the multiplier premium
    /// transferred to the Treasury on remove or finalization.
    pub base_bond: Balance,
    /// Inclusion fee paid by the depositor — see the type-level doc for
    /// the exact-vs-amortised distinction. `None` if the runtime does not
    /// emit `TransactionPayment::TransactionFeePaid` (older runtimes).
    pub tx_fee: Option<Balance>,
}

/// Bond information attached to a stored MIDDS record, mirroring the
/// `MiddsApi::deposit_info` runtime tuple `(depositor, total_held,
/// base_bond, finalized)`.
#[derive(Debug, Clone)]
pub struct DepositInfo {
    /// Account that paid the original bond.
    pub depositor: <ChainConfig as subxt::Config>::AccountId,
    /// Total amount currently on hold (or moved to the Treasury if
    /// `finalized`).
    pub total_held: Balance,
    /// Unmultiplied portion of the bond — what `remove_own` would refund.
    pub base_bond: Balance,
    /// `true` once the commitment window has elapsed and the bond has been
    /// transferred to the Treasury.
    pub finalized: bool,
}

/// Snapshot of the dynamic pricing inputs at the queried block.
#[derive(Debug, Clone, Copy)]
pub struct PricingSnapshot {
    /// Anti-DoS multiplier (per-block reactivity).
    pub fast_multiplier: FixedU128Raw,
    /// Anti-flood multiplier (rolling 7-day window).
    pub slow_multiplier: FixedU128Raw,
    /// Static target deposits per rolling 7-day window — by spec, runtime
    /// parameter, not on-chain state.
    pub weekly_target: u32,
    /// Sum of the 7 daily buckets — actual deposits seen in the last 7 days
    /// at day-resolution.
    pub weekly_actual: u32,
}

/// High-level handle for the MusicalWorks pallet instance.
pub struct MusicalWorksApi<'a> {
    client: &'a MiddsClient,
}

impl<'a> MusicalWorksApi<'a> {
    pub(crate) fn new(client: &'a MiddsClient) -> Self {
        Self { client }
    }

    /// Permissionlessly deposit a new MusicalWork. Returns the on-chain id
    /// extracted from the `Deposited` event.
    pub async fn deposit<S>(&self, signer: &S, work: MusicalWork) -> Result<MiddsId, Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_inner(signer, work, None).await.map(|r| r.id)
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
        work: MusicalWork,
        nonce: u64,
    ) -> Result<DepositReceipt, Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_inner(signer, work, Some(nonce)).await
    }

    async fn deposit_inner<S>(
        &self,
        signer: &S,
        work: MusicalWork,
        nonce: Option<u64>,
    ) -> Result<DepositReceipt, Error>
    where
        S: Signer<ChainConfig>,
    {
        work.validate_format()?;

        let payload = subxt::dynamic::tx(PALLET_NAME, "deposit", EncodedCall::one(&work));
        let events = self.submit(signer, payload, nonce).await?;

        let (mut deposited, fee) = collect_deposit_events(&events)?;
        // Single-deposit extrinsics emit exactly one `Deposited`. Use `pop`
        // to get the last (and only) entry; mirrors the previous "remember
        // the last `Deposited` we saw" behaviour without the explicit loop.
        let (id, bond, base_bond) = deposited.pop().ok_or(Error::EventNotFound {
            pallet: PALLET_NAME,
            variant: DEPOSITED_EVENT,
        })?;
        Ok(DepositReceipt {
            id,
            bond,
            base_bond,
            tx_fee: fee,
        })
    }

    /// Deposit a batch of MusicalWorks atomically via `pallet_utility::batch_all`.
    ///
    /// All payloads are validated locally before submission. If any inner
    /// `deposit` fails on-chain (e.g. duplicate ISWC), `batch_all` reverts
    /// the whole bundle — partial application is impossible. Returns
    /// successfully when the batch is included in a best block; the caller
    /// can read [`Self::next_midds_id`] to recover the post-batch counter
    /// range. We deliberately do not wait for GRANDPA finalisation — see
    /// the rationale in [`Self::submit`].
    ///
    /// Empty input is a no-op. The intended use is mass-seeding (`midds
    /// seed`) where individual ids aren't needed; for single-deposit flows
    /// that need the allocated id back, prefer [`Self::deposit`].
    pub async fn deposit_batch<S>(&self, signer: &S, works: Vec<MusicalWork>) -> Result<(), Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_batch_inner(signer, works, None).await
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
        works: Vec<MusicalWork>,
        nonce: u64,
    ) -> Result<(), Error>
    where
        S: Signer<ChainConfig>,
    {
        self.deposit_batch_inner(signer, works, Some(nonce)).await
    }

    async fn deposit_batch_inner<S>(
        &self,
        signer: &S,
        works: Vec<MusicalWork>,
        nonce: Option<u64>,
    ) -> Result<(), Error>
    where
        S: Signer<ChainConfig>,
    {
        if works.is_empty() {
            return Ok(());
        }
        for work in &works {
            work.validate_format()?;
        }

        let at_block = self.client.inner.at_current_block().await?;
        let mut tx_client = at_block.transactions();

        let mut inner_calls: Vec<Vec<u8>> = Vec::with_capacity(works.len());
        for work in &works {
            let inner = subxt::dynamic::tx(PALLET_NAME, "deposit", EncodedCall::one(work));
            // `call_data` resolves pallet/call indices via metadata and
            // returns the canonical SCALE-encoded `RuntimeCall` bytes — the
            // wire shape `Vec<RuntimeCall>` expects per element.
            let bytes = tx_client.call_data(&inner)?;
            inner_calls.push(bytes);
        }

        let payload = subxt::dynamic::tx(
            "Utility",
            "batch_all",
            EncodedCall::one(&PreEncodedCalls::new(inner_calls)),
        );
        let progress = match nonce {
            Some(n) => {
                let params = subxt::config::DefaultExtrinsicParamsBuilder::<ChainConfig>::new()
                    .nonce(n)
                    .build();
                tx_client
                    .sign_and_submit_then_watch(&payload, signer, params)
                    .await?
            }
            None => {
                tx_client
                    .sign_and_submit_then_watch_default(&payload, signer)
                    .await?
            }
        };
        let in_block = wait_for_in_block(progress).await?;
        in_block.wait_for_success().await?;
        Ok(())
    }

    /// Deposit a batch of MusicalWorks atomically via `pallet_utility::batch_all`,
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
        works: Vec<MusicalWork>,
        nonce: u64,
    ) -> Result<Vec<DepositReceipt>, Error>
    where
        S: Signer<ChainConfig>,
    {
        if works.is_empty() {
            return Ok(Vec::new());
        }
        for work in &works {
            work.validate_format()?;
        }

        let at_block = self.client.inner.at_current_block().await?;
        let tx_client = at_block.transactions();

        let mut inner_calls: Vec<Vec<u8>> = Vec::with_capacity(works.len());
        for work in &works {
            let inner = subxt::dynamic::tx(PALLET_NAME, "deposit", EncodedCall::one(work));
            let bytes = tx_client.call_data(&inner)?;
            inner_calls.push(bytes);
        }

        let payload = subxt::dynamic::tx(
            "Utility",
            "batch_all",
            EncodedCall::one(&PreEncodedCalls::new(inner_calls)),
        );

        // Reuse `submit` so the at-block retry / inclusion-wait policy stays
        // consistent with the single-deposit path. The events stream contains
        // one `Deposited` per inner deposit plus the single outer
        // `TransactionFeePaid` (event order matches inner-call order, which
        // subxt preserves).
        let events = self.submit(signer, payload, Some(nonce)).await?;

        let expected_records = works.len();
        let (deposited, total_fee) = collect_deposit_events(&events)?;
        if deposited.len() != expected_records {
            return Err(Error::EventNotFound {
                pallet: PALLET_NAME,
                variant: DEPOSITED_EVENT,
            });
        }
        // Integer division loses up to (records-1) plancks of total fee per
        // batch — negligible on a 12-decimal chain (sub-pico-token rounding)
        // and identical for every record in the batch by construction.
        let amortised_fee = total_fee.map(|fee| fee / expected_records as Balance);
        let receipts = deposited
            .into_iter()
            .map(|(id, bond, base_bond)| DepositReceipt {
                id,
                bond,
                base_bond,
                tx_fee: amortised_fee,
            })
            .collect();
        Ok(receipts)
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
        let base_addr = subxt::dynamic::constant::<Balance>(PALLET_NAME, "DepositBase");
        let per_byte_addr = subxt::dynamic::constant::<Balance>(PALLET_NAME, "DepositPerByte");
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
            subxt::dynamic::storage(PALLET_NAME, "NextMiddsId");
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
    /// values (`value × 10^18`). Use [`fixed_u128_to_f64`] for display.
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
        // Wire shape is `Option<(AccountId, Balance, Balance, bool)>`.
        let raw = self
            .runtime_api::<Option<(
                <ChainConfig as subxt::Config>::AccountId,
                Balance,
                Balance,
                bool,
            )>>("deposit_info", &id.encode())
            .await?;
        Ok(raw.map(
            |(depositor, total_held, base_bond, finalized)| DepositInfo {
                depositor,
                total_held,
                base_bond,
                finalized,
            },
        ))
    }

    /// Call a `MiddsApi_<method>` runtime API and SCALE-decode the response
    /// into `T`. Runtime API responses are plain SCALE so any `Decode` type
    /// (including tuples and `Option<...>`) round-trips through here.
    async fn runtime_api<T: Decode>(&self, method: &str, args: &[u8]) -> Result<T, Error> {
        let at_block = self.client.inner.at_current_block().await?;
        let function = format!("{RUNTIME_API_NAME}_{method}");
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

/// `(id, bond, base_bond)` extracted from a single `Deposited` event.
type DepositedEvent = (MiddsId, Balance, Balance);

/// Walk an extrinsic's event stream and surface every inner `Deposited`
/// plus the optional outer `TransactionFeePaid`.
///
/// Shared by single-deposit and batch paths — the only thing they disagree
/// on is the expected `Deposited` count, so that check stays at the call
/// site. Decodes that fail to match the expected event shape are ignored
/// (rather than propagated) so a future runtime extension that adds an
/// extra trailing field doesn't poison the receipt path.
fn collect_deposit_events(
    events: &ExtrinsicEvents<ChainConfig>,
) -> Result<(Vec<DepositedEvent>, Option<Balance>), Error> {
    let mut deposited: Vec<DepositedEvent> = Vec::new();
    let mut fee: Option<Balance> = None;
    for event in events.iter() {
        let event = event?;
        match (event.pallet_name(), event.event_name()) {
            (PALLET_NAME, DEPOSITED_EVENT) => {
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

/// Decode the `Deposited { id, depositor, bond, base_bond }` event payload.
/// Wire shape is `(MiddsId, AccountId, Balance, Balance)`. Depositor is
/// dropped on the floor — the caller is the submitting signer and already
/// knows it — but still `Decode`d to advance the cursor (cf.
/// [`decode_fee_paid`] for the rationale).
fn decode_deposited(bytes: &[u8]) -> Option<(MiddsId, Balance, Balance)> {
    let mut cursor = bytes;
    let id = MiddsId::decode(&mut cursor).ok()?;
    <ChainConfig as subxt::Config>::AccountId::decode(&mut cursor).ok()?;
    let bond = Balance::decode(&mut cursor).ok()?;
    let base_bond = Balance::decode(&mut cursor).ok()?;
    Some((id, bond, base_bond))
}
