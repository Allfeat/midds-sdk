//! Internal funding helpers used by `seed --auto-fund` and
//! `bench fees --auto-fund`.
//!
//! These are not part of the MIDDS protocol — they exist purely so that
//! `seed` / `bench` runs against a freshly-booted dev node have funded
//! signers to sign deposits with. Do not point them at a public chain.

use anyhow::{Context, Result, anyhow};
use midds_client::{
    Balance, MiddsClient, PricingSnapshot,
    batch::PreEncodedCalls,
    codec_bridge::EncodedCall,
    fixed_u128_to_f64,
    subxt::{
        self,
        utils::{AccountId32, MultiAddress},
    },
    subxt_signer::sr25519::Keypair,
    wait_for_in_block,
};
use midds_traits::Midds;
use parity_scale_codec::Compact;

use crate::bench::worker::ApiOf;

/// Worst-case multiplier ramp the seed/bench loop can ever produce, taken
/// straight from `docs/economics.md` §5.1 / §5.2:
///
/// - `FastMultiplierMax = 20×` (anti-DoS ceiling on per-block ramp)
/// - `SlowMultiplierMax = 50×` (anti-flood ceiling on rolling 7-day window)
///
/// We size every auto-fund as if the signer would be charged `1000×` the
/// base bond on every record. That's a massive over-fund relative to the
/// realistic case (multipliers rarely exceed `~5×` even under sustained
/// load), but the alternative is mid-run `BondHoldFailed` cascades when a
/// burst pushes `M_fast` above the snapshot used at fund time. The cost
/// is paid in dev-node plancks moved between accounts owned by the same
/// operator — strictly cosmetic on a chain we control.
///
/// If the runtime constants ever change, bump these to match. The pallet
/// declares them as `#[pallet::constant]` (`FastMultiplierMax`,
/// `SlowMultiplierMax`) — read-from-chain plumbing is deliberately omitted
/// to keep the auto-fund path simple.
const FAST_MULTIPLIER_MAX_FACTOR: Balance = 20;
const SLOW_MULTIPLIER_MAX_FACTOR: Balance = 50;
const BOND_SAFETY_FACTOR: Balance = FAST_MULTIPLIER_MAX_FACTOR * SLOW_MULTIPLIER_MAX_FACTOR;

/// Substrate's address type for `SubstrateConfig`. The runtime expects this
/// shape on the `dest` argument of `Balances::transfer_keep_alive` —
/// keep the `u32` index width in lock-step with `midds_client::ChainConfig`.
type Address = MultiAddress<AccountId32, u32>;

/// Send `amount` to each signer, bundling `Balances::transfer_keep_alive`
/// calls into `pallet_utility::batch_all` extrinsics of `batch_size` inner
/// transfers. Funding N signers costs `ceil(N / batch_size)` blocks instead
/// of N. `batch_all` is atomic per chunk — a failure inside the batch reverts
/// the whole chunk, never partially applied.
pub(crate) async fn fund_signers_in_batches(
    client: &MiddsClient,
    funder: &Keypair,
    signers: &[Keypair],
    amount: u128,
    batch_size: u32,
) -> Result<()> {
    if signers.is_empty() {
        return Ok(());
    }
    let batch_size = batch_size.max(1) as usize;
    let total_batches = signers.len().div_ceil(batch_size);

    println!(
        "  funding in {total_batches} batch(es) of up to {batch_size} ({} signer(s) total)",
        signers.len()
    );
    for (batch_idx, chunk) in signers.chunks(batch_size).enumerate() {
        submit_batch(client, funder, chunk, amount)
            .await
            .with_context(|| {
                format!(
                    "submit batch #{} ({} transfers)",
                    batch_idx + 1,
                    chunk.len()
                )
            })?;
        println!(
            "  batch {}/{} funded {} signer(s)",
            batch_idx + 1,
            total_batches,
            chunk.len()
        );
    }
    Ok(())
}

/// Build a `Utility::batch_all` containing one `Balances::transfer_keep_alive`
/// per target, sign with `funder`, submit, and wait for finalisation.
async fn submit_batch(
    client: &MiddsClient,
    funder: &Keypair,
    targets: &[Keypair],
    amount: u128,
) -> Result<()> {
    // Best-block pin so the funder's auto-nonce reflects pending transfers
    // — same rationale as `MiddsClient::at_best_block`. Finalised would
    // reject back-to-back batches as "Transaction is outdated".
    let at_block = client.at_best_block().await?;
    let mut tx_client = at_block.transactions();

    let mut inner_calls: Vec<Vec<u8>> = Vec::with_capacity(targets.len());
    for target in targets {
        let dest: Address = target.public_key().to_address();
        let inner = subxt::dynamic::tx(
            "Balances",
            "transfer_keep_alive",
            EncodedCall::two(&dest, &Compact(amount)),
        );
        // `call_data` resolves pallet/call indices via metadata and returns
        // the canonical SCALE-encoded `RuntimeCall` bytes. That's exactly the
        // wire shape `Vec<RuntimeCall>` expects per element, so we can drop
        // each blob into [`PreEncodedCalls`] verbatim.
        let bytes = tx_client
            .call_data(&inner)
            .map_err(|e| anyhow!("encode inner transfer: {e}"))?;
        inner_calls.push(bytes);
    }

    let payload = subxt::dynamic::tx(
        "Utility",
        "batch_all",
        EncodedCall::one(&PreEncodedCalls::new(inner_calls)),
    );

    let progress = tx_client
        .sign_and_submit_then_watch_default(&payload, funder)
        .await
        .map_err(|e| anyhow!("submit batch: {e}"))?;
    // Inclusion is enough to make the funded balance visible to subsequent
    // submits — `wait_for_in_block` returns once the transfers are applied.
    // GRANDPA finality would tack on 2-3 blocks of pure waiting before the
    // deposit phase even starts, with no behavioural gain on a dev node.
    let in_block = wait_for_in_block(progress)
        .await
        .map_err(|e| anyhow!("batch not included: {e}"))?;
    in_block
        .wait_for_success()
        .await
        .map_err(|e| anyhow!("batch failed in block: {e}"))?;
    Ok(())
}

/// Inputs for [`announce_and_fund`]. Bundled into a struct because the helper
/// needs seven parameters and a positional list invites footguns at call
/// sites — same shape as [`crate::bench::seed::Args`] / `fees::Args`.
pub(crate) struct AnnounceAndFundArgs<'a, M: Midds> {
    pub client: &'a MiddsClient,
    /// Per-kind pallet-instance accessor — selects which `pallet-midds`
    /// instance's `DepositBase` / `DepositPerByte` constants to read.
    pub api_of: ApiOf<M>,
    pub partitions: &'a [Vec<M>],
    pub signers: &'a [Keypair],
    pub funder_uri: &'a str,
    pub fund_margin: f64,
    pub fund_batch_size: u32,
    /// Pricing snapshot taken by the caller. Surfaced in the announce line
    /// for diagnostics — *not* used in the fund computation, which always
    /// assumes worst-case multipliers (cf. [`BOND_SAFETY_FACTOR`]).
    pub pricing: PricingSnapshot,
}

/// Compute the per-signer fund amount, print a summary line, and pre-fund
/// every signer in batches. Returns the amount sent to each signer so callers
/// can record it in their report.
///
/// Reads the pallet's `DepositBase` / `DepositPerByte` constants from the
/// chain itself — callers only need to pass the rest of the configuration.
pub(crate) async fn announce_and_fund<M: Midds>(
    args: AnnounceAndFundArgs<'_, M>,
) -> Result<Balance> {
    let (deposit_base, deposit_per_byte) = (args.api_of)(args.client)
        .deposit_constants()
        .await
        .context("read pallet-midds DepositBase / DepositPerByte from chain")?;
    let amount = compute_per_signer_funding(
        args.partitions,
        deposit_base,
        deposit_per_byte,
        args.fund_margin.max(1.0),
    );
    println!(
        "auto-fund: DepositBase={base} DepositPerByte={per_byte} \
         M_fast={fast:.4}× M_slow={slow:.4}× (snapshot, fund covers worst-case \
         {worst}× = {fast_max}× × {slow_max}×) margin={margin:.2} → {amount}/signer",
        base = crate::format::plancks(deposit_base),
        per_byte = crate::format::plancks(deposit_per_byte),
        fast = fixed_u128_to_f64(args.pricing.fast_multiplier),
        slow = fixed_u128_to_f64(args.pricing.slow_multiplier),
        worst = BOND_SAFETY_FACTOR,
        fast_max = FAST_MULTIPLIER_MAX_FACTOR,
        slow_max = SLOW_MULTIPLIER_MAX_FACTOR,
        margin = args.fund_margin,
        amount = crate::format::plancks(amount),
    );
    let funder_keypair =
        crate::signers::build_signer(args.funder_uri).context("build funder signer")?;
    fund_signers_in_batches(
        args.client,
        &funder_keypair,
        args.signers,
        amount,
        args.fund_batch_size,
    )
    .await
    .context("auto-fund signers")?;
    Ok(amount)
}

/// Determine a uniform per-signer funding amount that covers the heaviest
/// signer's cumulative *worst-case* bond, scaled by `margin`.
///
/// We use the heaviest signer (rather than per-signer amounts) so the
/// funding step itself can be a single `batch_all` of identical transfers
/// — same wire shape from every angle. With round-robin partitioning the
/// variance is at most one record per signer, so the over-funding is
/// negligible. Shared between `seed --auto-fund` and `bench fees --auto-fund`,
/// which both partition the same way.
///
/// The "worst-case bond" is the base bond multiplied by [`BOND_SAFETY_FACTOR`]
/// (= `FastMultiplierMax × SlowMultiplierMax`). This intentionally ignores the
/// live multiplier snapshot: a snapshot below 1× would under-fund as soon as
/// the seed loop pushes `M_fast` back toward target, and a snapshot near 1×
/// would still under-fund if a sustained burst spikes `M_fast` mid-run.
/// Funding for the ceiling sidesteps the whole issue at the cost of moving
/// more plancks than strictly needed — fine on dev nodes.
pub(crate) fn compute_per_signer_funding<M: Midds>(
    partitions: &[Vec<M>],
    deposit_base: Balance,
    deposit_per_byte: Balance,
    margin: f64,
) -> Balance {
    let max_per_signer_base = partitions
        .iter()
        .map(|slice| {
            slice
                .iter()
                .map(|w| {
                    deposit_base.saturating_add(
                        deposit_per_byte.saturating_mul(w.encoded_size() as Balance),
                    )
                })
                .sum::<Balance>()
        })
        .max()
        .unwrap_or(0);
    let max_per_signer_bond = max_per_signer_base.saturating_mul(BOND_SAFETY_FACTOR);
    // f64 multiplication is fine here: we're scaling by a small factor
    // (1.0–2.0 typically) and overshooting by a few plancks is preferred
    // over arithmetic subtlety.
    ((max_per_signer_bond as f64) * margin).ceil() as Balance
}

#[cfg(test)]
mod tests {
    //! Unit tests for the auto-fund math. The interesting things to pin:
    //! - We use `max(per_signer_bond)` × margin, *not* `total / N`, because
    //!   round-robin partitioning can leave some signers with one more
    //!   record than others — those signers must be funded for their actual
    //!   load, not the average.
    //! - Funding always sizes for the worst-case multiplier ramp
    //!   (`BOND_SAFETY_FACTOR = 1000×`) so a single-shot snapshot at fund
    //!   time can never under-finance a signer mid-seed.
    use super::*;
    use midds_fixtures::pathological;
    use midds_types::MusicalWork;
    // `encoded_size` on the concrete `MusicalWork` needs the trait in scope;
    // the generic lib code reaches it through the `M: Midds` bound instead.
    use parity_scale_codec::Encode;

    #[test]
    fn compute_per_signer_funding_uses_heaviest_signer() {
        // Two signers, one with 2 records, one with 1 record. The fair
        // funding is sized by the 2-record signer, not the average.
        let work_a = pathological::min_size_musical_work();
        let work_b = pathological::min_size_musical_work();
        let work_c = pathological::min_size_musical_work();
        let partitions = vec![vec![work_a, work_b], vec![work_c]];

        let funding = compute_per_signer_funding(&partitions, 10, 1, 1.0);
        let single_bond = 10 + (pathological::min_size_musical_work().encoded_size() as Balance);
        // Heaviest signer holds two records' worth of bond — × the safety
        // factor.
        assert_eq!(funding, 2 * single_bond * BOND_SAFETY_FACTOR);
    }

    #[test]
    fn margin_is_applied_after_max() {
        let work = pathological::min_size_musical_work();
        let partitions = vec![vec![work]];
        let single_bond = 10 + (pathological::min_size_musical_work().encoded_size() as Balance);

        let funding = compute_per_signer_funding(&partitions, 10, 1, 1.5);
        let safe_bond = single_bond * BOND_SAFETY_FACTOR;
        assert_eq!(funding, ((safe_bond as f64) * 1.5).ceil() as Balance);
    }

    #[test]
    fn empty_partitions_fund_zero() {
        let partitions: Vec<Vec<MusicalWork>> = vec![];
        assert_eq!(compute_per_signer_funding(&partitions, 10, 1, 1.3), 0);
    }

    #[test]
    fn safety_factor_matches_spec_ceilings() {
        // Locks the worst-case factor against the spec values. Anyone
        // changing `FAST_MULTIPLIER_MAX_FACTOR` / `SLOW_MULTIPLIER_MAX_FACTOR`
        // (because the runtime ceilings drifted) needs to also adjust this.
        // Catches accidental edits against the hard-coded values in
        // `docs/economics.md` §5.
        assert_eq!(FAST_MULTIPLIER_MAX_FACTOR, 20);
        assert_eq!(SLOW_MULTIPLIER_MAX_FACTOR, 50);
        assert_eq!(BOND_SAFETY_FACTOR, 1000);
    }
}
