//! Dynamic pricing — anti-DoS (`M_fast`) and anti-flood (`M_slow`) multipliers.
//!
//! Per `docs/economics.md` §5: a fresh chain starts at 1.0× and each
//! multiplier shifts toward `[Min, Max]` based on observed deposit demand.
//! `M_fast` reacts per-block to bursts; `M_slow` tracks a rolling 7-day
//! window through daily buckets rotated by `on_initialize`.

use crate::{BalanceOf, pallet::*};
use alloc::vec;
// `frame::prelude` covers `BoundedVec`, the storage pallet prelude, and the
// arithmetic traits/types (`FixedPointNumber`, `FixedU128`, `One`, `Saturating`).
use frame::prelude::*;

/// Number of daily buckets composing the slow rolling window. The window is
/// **7 days exactly** by spec — change this and the `M_slow` semantics change.
pub const SLOW_WINDOW_DAYS: u32 = 7;

/// Default value for the multipliers when first read — both start at 1.0×
/// (neutral) so a fresh chain prices deposits at the unmultiplied bond.
pub struct DefaultUnitMultiplier;
impl Get<FixedU128> for DefaultUnitMultiplier {
    fn get() -> FixedU128 {
        FixedU128::one()
    }
}

/// Default value for [`SlowWindowBuckets`] — a fully-zero 7-day buffer.
pub struct DefaultSlowBuckets;
impl Get<BoundedVec<u32, ConstU32<SLOW_WINDOW_DAYS>>> for DefaultSlowBuckets {
    fn get() -> BoundedVec<u32, ConstU32<SLOW_WINDOW_DAYS>> {
        BoundedVec::try_from(vec![0u32; SLOW_WINDOW_DAYS as usize])
            .expect("SLOW_WINDOW_DAYS u32s ≤ SLOW_WINDOW_DAYS bound; qed")
    }
}

impl<T: Config<I>, I: 'static> Pallet<T, I> {
    /// Apply the current `M_fast × M_slow` multipliers to a base bond.
    ///
    /// Read once at deposit time and stored as the bond `amount`; the
    /// multiplier premium banked here is *not* re-priced on `update` /
    /// `force_edit` (only the base portion is). This is the spec's
    /// anti-arbitrage guarantee — see `docs/economics.md` §5.5.
    pub(crate) fn apply_multipliers(base: BalanceOf<T, I>) -> BalanceOf<T, I> {
        let fast = FastMultiplier::<T, I>::get();
        let slow = SlowMultiplier::<T, I>::get();
        let combined = fast.saturating_mul(slow);
        combined.saturating_mul_int(base)
    }

    /// Bump the per-block and current-bucket demand counters. Called once per
    /// successful `deposit`.
    pub(crate) fn record_deposit_demand() {
        DepositsThisBlock::<T, I>::mutate(|c| *c = c.saturating_add(1));
        let head = SlowWindowHead::<T, I>::get() as usize;
        SlowWindowBuckets::<T, I>::mutate(|buckets| {
            if let Some(bucket) = buckets.get_mut(head) {
                *bucket = bucket.saturating_add(1);
            }
        });
    }

    /// EIP-1559-style adjustment: per-block step proportional to the
    /// observed/target deviation, clamped to `[FastMultiplierMin,
    /// FastMultiplierMax]`. A bloc landing at 10× the target therefore moves
    /// `M_fast` by `10 × FastAdjustmentRate`, not just `FastAdjustmentRate`.
    /// Concrete numbers per `docs/economics.md` §5.1.
    pub(crate) fn adjust_fast_multiplier(observed: u32) {
        let new = Self::adjust_multiplier(
            FastMultiplier::<T, I>::get(),
            observed,
            T::FastTargetPerBlock::get(),
            T::FastAdjustmentRate::get(),
            T::FastMultiplierMin::get(),
            T::FastMultiplierMax::get(),
        );
        FastMultiplier::<T, I>::put(new);
    }

    /// Advance the `(head + 1) mod 7` daily bucket and zero it out so the
    /// next day's deposits accumulate in a fresh slot.
    pub(crate) fn rotate_slow_bucket() {
        let len = SLOW_WINDOW_DAYS as u8;
        let next_head = SlowWindowHead::<T, I>::get().wrapping_add(1) % len;
        SlowWindowBuckets::<T, I>::mutate(|buckets| {
            if let Some(slot) = buckets.get_mut(next_head as usize) {
                *slot = 0;
            }
        });
        SlowWindowHead::<T, I>::put(next_head);
    }

    /// EIP-1559-style adjustment over the 7-day rolling sum: per-day step
    /// proportional to the deviation between the bucket sum and
    /// `SlowTargetPerWindow`, clamped to `[SlowMultiplierMin,
    /// SlowMultiplierMax]`. Concrete numbers per `docs/economics.md` §5.2.
    pub(crate) fn adjust_slow_multiplier() {
        let new = Self::adjust_multiplier(
            SlowMultiplier::<T, I>::get(),
            Self::bucket_sum(),
            T::SlowTargetPerWindow::get(),
            T::SlowAdjustmentRate::get(),
            T::SlowMultiplierMin::get(),
            T::SlowMultiplierMax::get(),
        );
        SlowMultiplier::<T, I>::put(new);
    }

    /// Shift `current` toward the observed/target deviation, clamped to
    /// `[min, max]`. EIP-1559 form: the up step is
    /// `current × (1 + rate × δ)` with `δ = (observed − target) / target`,
    /// so a 10× burst yields a `10 × rate` move instead of the constant 1×
    /// step the previous binary version did. The down step is the reciprocal
    /// `current / (1 + rate × δ)`.
    ///
    /// The two sides are **deliberately asymmetric**: the up-side `δ` is
    /// unbounded (a burst can land at many multiples of target), but the
    /// down-side `δ = (target − observed) / target` is capped at 1 because
    /// `observed ≥ 0`. So a spike ratchets up fast and then decays by at most
    /// `÷(1 + rate)` per fully-idle period — the intended anti-DoS shape
    /// (quick to react, slow to forgive). A clean reciprocal round-trip
    /// therefore only holds for deviations of magnitude `≤ 1× target`; a
    /// larger burst unwinds over several periods rather than in one.
    fn adjust_multiplier(
        current: FixedU128,
        observed: u32,
        target: u32,
        rate: FixedU128,
        min: FixedU128,
        max: FixedU128,
    ) -> FixedU128 {
        use core::cmp::Ordering;
        if target == 0 {
            return current;
        }
        let one = FixedU128::one();
        let candidate = match observed.cmp(&target) {
            Ordering::Equal => current,
            Ordering::Greater => {
                let excess = observed.saturating_sub(target);
                let delta_ratio = FixedU128::saturating_from_rational(excess, target);
                let factor_up = one.saturating_add(rate.saturating_mul(delta_ratio));
                current.saturating_mul(factor_up)
            }
            Ordering::Less => {
                let shortfall = target.saturating_sub(observed);
                let delta_ratio = FixedU128::saturating_from_rational(shortfall, target);
                let scaled = rate.saturating_mul(delta_ratio);
                let factor_down = one
                    .const_checked_div(one.saturating_add(scaled))
                    .unwrap_or(one);
                current.saturating_mul(factor_down)
            }
        };
        if candidate < min {
            min
        } else if candidate > max {
            max
        } else {
            candidate
        }
    }

    /// Sum of the 7 daily buckets — approximates "deposits seen in the last
    /// 7 days" with day-resolution.
    fn bucket_sum() -> u32 {
        SlowWindowBuckets::<T, I>::get()
            .iter()
            .copied()
            .fold(0u32, u32::saturating_add)
    }

    /// Current `(M_fast, M_slow)`.
    pub fn current_multipliers() -> (FixedU128, FixedU128) {
        (FastMultiplier::<T, I>::get(), SlowMultiplier::<T, I>::get())
    }

    /// Static target for the rolling 7-day window — by spec a runtime
    /// parameter, not on-chain state.
    pub fn weekly_target() -> u32 {
        T::SlowTargetPerWindow::get()
    }

    /// Day-resolution rolling sum over the last 7 daily buckets.
    pub fn weekly_actual() -> u32 {
        Self::bucket_sum()
    }
}
