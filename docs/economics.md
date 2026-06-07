# MIDDS SDK — Economic model

> Reference document for the `pallet-midds` economics: bond, dynamic
> pricing, finalization window, destination of funds. Counterpart to
> `docs/plan.md` (architecture) and `docs/testing.md` (tests).
> Target: a sound model, sybil-resistant by construction, aligned with the
> Allfeat tokenomics (supply capped at 1 B AFT, never any issuance beyond).

---

## 1. Context and objectives

The pallet-midds must serve a product that has **three structuring
characteristics**:

1. **Permissionless from day 0** — no whitelist, anyone can deposit.
2. **No on-chain quality filter for several months** — the PoM (Proof of
   Metadata) mechanism will come later; in standalone, the format is the
   only check.
3. **Marketing promise**: *"minimal cost per million metadata records"*
   (whitepaper §1.1) — large-scale ingestion must stay affordable.

Three additional economic constraints frame the design:

- **Supply capped at 1 000 000 000 AFT, never reissued.** No mechanism
  should destroy supply irreversibly (no burn).
- **Web3 = no per-account anti-spam** — an attacker creates 10 K wallets
  for < 1 K AFT (just the ED). Any defense indexed on the account's
  identity is defeated by sybil.
- **Identifier duplicates allowed** — several parties may deposit their
  version of the same ISWC. Only exact duplicates (same payload) are
  rejected. Namespace squatting does not exist as a threat.

→ The economic design must therefore work **at the community level**
(never per-account), be **honest about what it does not do** (no absolute
anti-sybil without curation), and **preserve the supply** by recycling
any AFT collected rather than burning it.

---

## 2. Locked decisions

| # | Topic | Decision |
|---|---|---|
| 1 | Payment model | **Bond with a 7-day refundable window**, then transfer to the Foundation Treasury |
| 2 | Supply preservation | **No burn**. Finalized bond → Foundation Treasury (recycled via governance) |
| 3 | Dynamic pricing | **Two superposed multipliers**: `M_fast` (per block, anti-DoS) + `M_slow` (rolling 7d window, anti-flood) |
| 4 | Finalization cycle | 7 days after deposit, bond auto-converted via bounded `on_initialize`, fallback permissionless `finalize(id)` |
| 5 | Non-unique identifier | `IdentifierClaims: DoubleMap<Identifier, MiddsId, ()>` — native multi-claim |
| 6 | Exact anti-duplicate | `PayloadHashes: Map<H256, MiddsId>` — SCALE-encoded hash of the payload |
| 7 | Sudo remove distinction | **Two separate extrinsics**: `force_remove_refund` (typo) and `force_remove_slash` (abuse) |
| 8 | 7d window | Aligned with the weekly music release cycle (Friday IFPI Global Release Day) |
| 9 | V1 throughput target | `SlowTargetPerWindow = 200 000` deposits/week (~30 K/day). Conservative calibration, adjustable by governance |
| 10 | Runtime tx fees | `TransactionByteFee` ÷10 vs current (1 µAFT/B), `WeightFeeFactor` unchanged |
| 11 | Migration | **None** — pallet not on mainnet, melodie testnet reset on deployment |
| 12 | Anti-sybil | **Not guaranteed at the protocol level**. Quality filter = off-chain (indexers, front-end) until PoM |

The previous decisions from `plan.md` § 2 (#4 simple bond, #5 static formula, #10 uniqueness per identifier) are **superseded** by those above.

---

## 3. Principle in one sentence

> **Everyone pays a bond at deposit. The bond is fully refundable for 7
> days, then transferred to the Foundation Treasury at finalization. The
> bond amount is multiplied by two dynamic multipliers that push the whole
> community to spread its deposits over time rather than bursting them.**

Three properties emerge:

1. **Real refund** (not vaporware) — the user has 7 days to cancel a
   mistake or to back out, with a full refund.
2. **No permanent supply lock** — beyond 7 days, the bond leaves the
   user's account toward the Treasury, which can redistribute it (rewards
   top-up, grants, operations).
3. **Self-regulation** — dynamic pricing mechanically aligns honest
   parties and attackers on the same incentive: **spread over time**.
   An actor who bursts pays more, whether they come from one wallet or a
   thousand.

---

## 4. Lifecycle of a MIDDS

```
t0       deposit(item)
         → HOLD bond = base × M_fast(t0) × M_slow(t0)
         → MIDDS published, readable immediately
         → push (t0 + 7d, MiddsId) into PendingFinalization

[t0, t0 + 7d]  — refundable window
         remove_own(id)  → 100 % bond refund, MIDDS deleted
         update(id, payload) → modifies the payload, window unchanged (anchored on t0)

t0 + 7d  — automatic finalization
         on_initialize consumes PendingFinalization[t0 + 7d]
         → release of the HOLD to the Treasury
         → DepositInfo.finalized = true
         → MIDDS becomes permanent

> t0 + 7d  — sudo management only
         force_remove_refund(id) → removed, dispossessed party indemnified (typo)
         force_remove_slash(id)  → removed, no refund (abuse)
         force_edit(id, payload) → modified without touching the economics
```

### 4.1 Anchoring of the window

The window is anchored on `deposited_at` (original timestamp of the
deposit). **`update` does not reset the window.** Otherwise a user could
update every 6 days to keep their bond eligible for refund in perpetuity.

### 4.2 Finalization trigger

**Eager + fallback** architecture:

- **`on_initialize(n)`**: consumes up to `MaxFinalizationsPerBlock`
  entries of `PendingFinalization::iter_prefix(n)`. Guarantees automatic
  finalization in the nominal case.
- **`finalize(id)` permissionless**: fallback if the queue overflows
  (manual catch-up, or via a third-party crank / oracle).

A queue indexed by block allows an O(1) read of the items to finalize at
the current block. The `on_initialize` weight is bounded by
`MaxFinalizationsPerBlock × W::finalize_one()`.

---

## 5. Dynamic pricing model

The effective bond at deposit time is:

```
final_bond(t) = MiddsBondBase
              + MiddsBondPerByte × encoded_size(item)
final_bond(t) ×= M_fast(t)
final_bond(t) ×= M_slow(t)
```

with two multipliers with different dynamics:

### 5.1 `M_fast` — instantaneous anti-DoS

| Parameter | Value |
|---|---|
| Target | 100 deposits per block (≈ 17/s at 6s/block) |
| Adjustment step | ±12.5 % per block |
| Floor | 0.1× |
| Ceiling | 20× |
| Reactivity | Multiplier doubles / halves in ~6 blocks (~36 s) |

**Effect**: a burst of 1000 deposits in a block spikes `M_fast` to
~5–10×; the next block without load brings it back toward 1× within a few
minutes. Prevents block stuffing without penalizing the nominal regime.

### 5.2 `M_slow` — anti-flood and spreading incentive

| Parameter | Value |
|---|---|
| Window | 7 rolling days (rolling window) |
| Target | 200 000 deposits / week (~30 K/day) |
| Adjustment step | ±5 % per day |
| Floor | 0.1× |
| Ceiling | 50× |
| Reactivity | Reaches 5× in ~7 days under sustained flood, comes back down in ~7 days afterward |

**Effect**: the window sees the whole community, regardless of the number
of wallets used. A patient attacker with 1000 wallets and an honest user
with 1 wallet are treated exactly the same.

### 5.3 Expected cost profiles

Effective bond for a typical MusicalWork (~150 B encoded) with the
**hybrid payload-aware** calibration (base 100 mAFT, per-byte 250 µAFT/B,
cf §6):

| Profile | M_fast | M_slow | Bond paid |
|---|---|---|---|
| Nominal regime | 1× | 1× | ~138 mAFT (~$0.003) |
| Indie artist, 5 spread deposits | 1× | 1× | ~138 mAFT |
| Label, 10K records over 1 month | 1× avg | ~1× | ~138 mAFT avg |
| CMO mass ingest, 1M over 60d | 1× | ~1.5× | ~207 mAFT avg |
| Burst of 1 full block | ~10× | 1× | ~1.4 AFT (on that block) |
| Patient flood 1M in 7d | 1× avg | ~10× | ~1.4 AFT for 7d |
| Very patient flood 1M in 60d | 1× | ~1.5× | ~207 mAFT (≈ honest) |

The last case is interesting: a **truly patient** attacker ends up
looking like a legitimate user, economically speaking. At that point, the
defense shifts to quality filtering (off-chain, or future PoM). That is
the right transfer of the problem.

### 5.3.1 Payload-aware effect

Unlike a flat bond, calibration B makes the cost diverge appreciably
depending on the payload size. For the same 1× multiplier, at $0.02/AFT:

| Type | Typical encoded | Max encoded | Typical bond | Max bond |
|---|---:|---:|---:|---:|
| MusicalWork | 137 B | 1 416 B | ~$0.003 | ~$0.009 |
| Recording | 222 B | 4 722 B | ~$0.003 | ~$0.026 |
| Release | 197 B | 9 030 B | ~$0.003 | ~$0.047 |

A saturated Release (long tracklist, rich metadata) pays ~17× the price
of a typical MusicalWork — that is the anti-stuffing incentive. Healthy
MIDDS stay around $0.003, regardless of the type.

### 5.4 Why 7 days

Aligned with the business rhythm of the music industry:

- **Friday = Global Release Day** (IFPI standard since 2015)
- Labels prepare their releases during the week, deposit in advance
- A 7d window captures exactly the weekly release cycle
- The mechanism **educates** the community to register its MIDDS *before*
  release day — an operationally sound behavior

### 5.5 Refund and multiplier

The refund during the window (`remove_own`) returns the **base of each
layer** to its respective payer. The multiplier premium (beyond the base
bond) banked in each layer is transferred to the Treasury at
finalization, even if the item is subsequently refunded.

Rationale: prevent the *expensive-burst-deposit → remove → cheap-re-deposit* arbitrage. Without this rule, a user would wait for `M_slow` to drop and resubmit at lower cost while recovering the entirety of their initial bond. With this rule, the premium paid to burst stays lost even after refund — which aligns the incentive with "don't burst in the first place".

Implementation: `remove_own` returns `sponsor.base` to the sponsor + `owner.base` to the depositor (where applicable). The `(M_fast × M_slow − 1) × base` delta captured by each layer is transferred to the Treasury immediately, **per layer** — so a sponsor and a depositor who each posted their share don't share each other's burst penalty.

### 5.6 Stratified bond and web3 escape hatch

A MIDDS carries two bond layers, independent of each other:

- **`sponsor_layer`** (always present) — the base + premium paid at the
  initial `deposit`. On a self-deposit, the payer is the depositor; on a
  `deposit_on_behalf`, it is the sponsor operator.
- **`owner_layer`** (`Option`) — appears only when the **depositor of a
  sponsored record** extends the data via a plain `update`. On that
  occasion, the depositor pays the `Δbase × M_current` from their own
  funds; their layer banks its own premium.

This stratification implements the **web3 escape hatch**: an artist
onboarded via a SaaS platform (sponsor) can at any time take back control
of their data without depending on the sponsor's balance. The rule:

- `update` (caller = depositor) on a sponsored record → the target layer
  is `owner_layer` (created on the first solo extension, then extended
  without a new premium).
- `update_on_behalf` (caller = sponsor) → the target layer is
  `sponsor_layer`. Compatible with the existence of an `owner_layer` —
  sponsor and owner co-exist (Q2.b).
- On a shrink where the `Δbase` to release exceeds the caller's layer, the
  reduction overflows LIFO onto the other layer, which is then refunded.
- `remove_own_on_behalf` closes the meta-tx loop: free caller (third-party
  relayer, sponsor, or whatever), the owner signature suffices to authorize
  the retraction. An owner who has *never* held any AFT can thus take back
  all their funds without worrying about on-chain fees.

On `remove_own` / `remove_own_on_behalf` / `force_remove_refund`, each
layer reconciles independently with its `payer`; on `finalize` /
`force_remove_slash`, each layer transfers its full amount to the
Treasury. The financial separation is complete: no funds from one layer
pay for the other's penalty.

Per-layer premium preservation: on `update`, the base of an existing layer
evolves by the formula `new_amount = new_base + max(0, old_amount −
old_base)`. The premium banked at creation is sticky and is not re-priced
at the current multiplier (anti-arbitrage §5.5). A layer created under
`M < 1` (degenerate case, multipliers at the floor) is underpaid at
creation — its subsequent extension rebases the layer to the new base
total, which may grow the hold; this behavior mirrors the
pre-stratification.

### 5.6.1 Sponsor exposure to the premium (assumed risk)

On a sponsored record, the retraction authority belongs to the **owner**
(`remove_own` / `remove_own_on_behalf`). If the owner cancels during a
period of `M > 1`, the sponsor's base is returned to them, but **the
multiplier premium that the sponsor paid goes to the Treasury**
(anti-arbitrage rule §5.5, applied per layer). An owner can therefore, at
zero cost to themselves (via a relayer in `remove_own_on_behalf`), make
the sponsor lose the premium paid at `deposit_on_behalf`.

This is the **assumed counterpart** of the web3 escape hatch: returning
the premium to the sponsor would reopen burst arbitrage by sponsor↔owner
collusion (expensive burst → owner cancels → premium recovered). The loss
is zero at `M = 1` (nominal regime) and only appears under load.

**Mitigation, on the operator side**: do not auto-sponsor when `M` is
high. The `current_multipliers` / `current_deposit_price` RPCs (§12.2)
are exposed precisely to gate `deposit_on_behalf` on the current
multiplier before signing — the equivalent of a *slippage limit*.

---

## 6. Runtime constants

> **Note** — `MiddsDepositBase` and `MiddsDepositPerByte` are being
> migrated to `StorageValue`s adjustable by sudo (cf §13.4). The values
> below remain those initialized at genesis on Melodie; once §13.4 is
> delivered, they become mutable parameters without a runtime upgrade. The
> structure of the pallet's Config trait `parameter_types!` changes
> accordingly (breaking change `0.1 → 0.2`).

### 6.1 Hybrid payload-aware calibration (variant B)

```rust
// runtime/melodie/src/pallets/midds.rs (values initialized at genesis)

parameter_types! {
    // Bond formula (unmultiplied) — hybrid payload-aware: `DepositBase`
    // pinned to the ExistentialDeposit (0.1 AFT) for minimal anti-sybil
    // cost, weight shifted onto `DepositPerByte` so saturated payloads
    // pay materially more than minimal ones. At $0.02/AFT a typical
    // MusicalWork (~137 B) costs ~$0.003, a maxed-out Release (~9 KB)
    // ~$0.05 — 17× ratio that creates the anti-stuffing incentive
    // absent from the prior flat 0.5 AFT calibration.
    pub const MiddsDepositBase: Balance = 100 * MILLIAFT;
    pub const MiddsDepositPerByte: Balance = 250 * MICROAFT;

    // Refundable window
    pub const MiddsCommitmentWindow: BlockNumber = 7 * DAYS;
    pub const MiddsMaxFinalizationsPerBlock: u32 = 100;

    // Fast multiplier
    pub const FastTargetPerBlock: u32 = 100;
    pub const FastAdjustmentRate: Perbill = Perbill::from_parts(125_000_000);
    pub const FastMultiplierMin: FixedU128 = FixedU128::from_rational(1, 10);
    pub const FastMultiplierMax: FixedU128 = FixedU128::from_u32(20);

    // Slow multiplier
    pub const SlowWindow: BlockNumber = 7 * DAYS;
    pub const SlowTargetPerWindow: u32 = 200_000;
    pub const SlowAdjustmentRate: Perbill = Perbill::from_percent(5);
    pub const SlowMultiplierMin: FixedU128 = FixedU128::from_rational(1, 10);
    pub const SlowMultiplierMax: FixedU128 = FixedU128::from_u32(50);

    // Destination of the finalized bond
    pub MiddsTreasuryAccount: AccountId =
        PalletId(*b"af/midds").into_account_truncating();
}
```

Standard runtime tx fees (to adjust in `transaction_payment.rs`):

```rust
parameter_types! {
    pub const TransactionByteFee: Balance = MICROAFT;          // ÷10 vs current
    pub const OperationalFeeMultiplier: u8 = 5;                // unchanged
    pub const WeightFeeFactor: Balance = 10 * MILLIAFT;        // unchanged
}
```

---

## 7. Extrinsics surface

| Extrinsic | Origin | Effect | Refund | Sudo |
|---|---|---|---|---|
| `deposit(item)` | `ProviderOrigin` (signed) | HOLD bond on `sponsor_layer` (= depositor), insert MIDDS, queue finalization | — | — |
| `deposit_on_behalf(owner, item, nonce, sig)` | `ProviderOrigin` (operator) + owner signature | HOLD bond on `sponsor_layer` (= operator), attribution = owner | — | — |
| `update(id, item)` | depositor, ≤ 7d | On a self-deposit: extends `sponsor_layer`. On a sponsored record: extends or creates `owner_layer` (web3 escape hatch) | — | — |
| `update_on_behalf(id, item, nonce, sig)` | original sponsor + owner signature, ≤ 7d | Extends `sponsor_layer`. Co-exists with an already-created `owner_layer` | — | — |
| `remove_own(id)` | depositor, ≤ 7d | Refund base of each layer to its payer, premiums to the Treasury | ✅ partial | — |
| `remove_own_on_behalf(id, nonce, sig)` | any `ProviderOrigin` (relayer) + owner signature, ≤ 7d | Same as `remove_own` but drivable by any relayer; closes the meta-tx loop for an owner without AFT | ✅ partial | — |
| `finalize(id)` | anyone, > 7d | Release HOLD of each layer to the Treasury | — | — |
| `force_edit(id, item)` | sudo | Update via `sponsor_layer` (governance never creates an `owner_layer`) | — | ✅ |
| `force_remove_refund(id)` | sudo, ≤ 7d | Cleanup + full refund to each payer (good-faith typo) | ✅ full | ✅ |
| `force_remove_slash(id)` | sudo | Cleanup, holds of each layer → Treasury (reported abuse) | ❌ | ✅ |
| `force_remove_many(Vec<id>)` | sudo | Batch cleanup, linear weight | depends on the flag | ✅ |

### 7.1 Runtime hooks

```rust
fn on_initialize(n: BlockNumberFor<T>) -> Weight {
    let mut weight = T::DbWeight::get().reads(1);

    // Reset fast counter
    let fast_count = DepositsThisBlock::<T, I>::take();
    Self::adjust_fast_multiplier(fast_count);
    weight += /* ... */;

    // Slow: bucket rotation at midnight
    if n % T::BlocksPerDay::get() == 0 {
        Self::rotate_slow_bucket();
        Self::adjust_slow_multiplier();
        weight += /* ... */;
    }

    // Due finalizations
    let to_finalize = PendingFinalization::<T, I>::iter_prefix(n)
        .take(T::MaxFinalizationsPerBlock::get() as usize)
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    for id in to_finalize {
        Self::do_finalize(id);
        weight += T::WeightInfo::finalize_one();
    }

    weight
}
```

---

## 8. Storage layout

```rust
// Unchanged
Items: StorageMap<_, Blake2_128Concat, MiddsId, T::Midds>;
NextMiddsId: StorageValue<_, MiddsId, ValueQuery>;

// MODIFIED: multi-claim on identifier (replaces IdentifierIndex)
IdentifierClaims:
    StorageDoubleMap<_, Blake2_128Concat, Identifier, Twox64Concat, MiddsId, ()>;

// NEW: exact anti-duplicate
PayloadHashes: StorageMap<_, Identity, T::Hash, MiddsId>;

// MODIFIED: payload_hash + finalized
DepositInfo: StorageMap<_, Blake2_128Concat, MiddsId, Deposit<T, I>>;
pub struct Deposit<T, I> {
    pub depositor: T::AccountId,
    pub deposited_at: BlockNumberFor<T>,
    pub amount: BalanceOf<T, I>,
    pub payload_hash: T::Hash,
    pub finalized: bool,
}

// NEW: finalization queue indexed by expiry block
PendingFinalization:
    StorageDoubleMap<_, Twox64Concat, BlockNumberFor<T>, Identity, MiddsId, ()>;

// NEW: dynamic multipliers
DepositsThisBlock: StorageValue<_, u32, ValueQuery>;
FastMultiplier: StorageValue<_, FixedU128, ValueQuery>;

SlowWindowBuckets: StorageValue<_, BoundedVec<u32, ConstU32<7>>, ValueQuery>;
SlowWindowHead: StorageValue<_, u8, ValueQuery>;
SlowMultiplier: StorageValue<_, FixedU128, ValueQuery>;
```

### 8.1 Storage cost per deposit

- 1 write `Items`
- 1 write `IdentifierClaims`
- 1 write `PayloadHashes`
- 1 write `DepositInfo`
- 1 write `PendingFinalization`
- 1 write `NextMiddsId`
- 1 read+write `DepositsThisBlock`
- 1 read+write bucket `SlowWindowBuckets`
- 1 hold on the balance (1 read+write `Holds`)

→ ~9 writes per deposit. Accounted for in the benchmark.

---

## 9. Destination of the finalized bond: Foundation Treasury

### 9.1 Why not burn

The AFT supply is **capped at 1 B and never reissued** (whitepaper §2.1).
Burning bond would permanently reduce the circulating supply, which is not
desired — the supply must be able to be recycled toward the network's
productive uses (rewards, grants, operations).

### 9.2 Why Treasury and not validators

Validators are already compensated by **tx fees** (`DealWithFees` in
`transaction_payment.rs` sends 100 % to the block author). The MIDDS bond
is a distinct protocol revenue — it funds the maintenance of the
**registry itself**, not of the consensus.

### 9.3 Treasury governance model

The Treasury account (`MiddsTreasuryAccount`) receives all finalized bonds
and the non-refunded deltas. Its management is governed:

- **Top-up of the community rewards pool** when the initial 260 M runs out
  (cf. whitepaper §5.3 *buybacks and redistribution*: this mechanism
  implements exactly the reinjection promise)
- **Grants** to ecosystem contributors (data quality, indexers, tools,
  integrations)
- **Foundation operations** (audits, infrastructure, support)
- **Occasional buybacks** if the market situation justifies it (off-market
  AFT buyback → reinjection into rewards)

The precise allocation is out of scope for this document — driven by the
Foundation via a governance pallet or multisig at first.

---

## 10. What the model does NOT do

To be documented explicitly to calibrate expectations (and public
promises):

### 10.1 No anti-sybil defense at the protocol level

During the standalone phase (before PoM), a patient actor with a budget
can deposit content of dubious quality at the floor price by spreading it
over weeks. The quality filter is **strictly off-chain**:

- Reference indexers (Allfeat, partners)
- Front-ends that only display MIDDS conforming to published rules
- Depositor reputation heuristics (account age, history)
- Occasional cleanup via `force_remove_slash` on identified patterns

This is not a bug — it is the **assumed consequence** of the
permissionless choice without curation. PoM will eventually bring on-chain
curation.

### 10.2 No bond recovery after 7 days

Intentional. The short window forces a fast commitment; after 7 days, the
record is *permanent* and the bond belongs to the Treasury.
`force_remove_refund` exists for the exceptional cases where the
Foundation wants to indemnify a good-faith mistake, but it is a governance
operation, not a depositor's right.

### 10.3 No automatic refund on sudo after finalization

If a finalized MIDDS is `force_remove`'d afterward (late abuse detected),
the bond is already at the Treasury — no restitution is made. Sudo just
cleans up the storage. The cost of the abuse was paid via the initial
bond; the additional sanction is the loss of the on-chain record.

### 10.4 No sharing with the validators

MIDDS bonds **never** go to the block author. It is a clean separation: tx
fees → consensus, bonds → product. Avoids perverse incentives (a
validator who force-includes spam for their own bond).

### 10.5 No priority reservation (Coretime-style)

No subscription tier / credits in V1. The EIP-1559-style spot market with
two multipliers is sufficient for the bootstrap phase. A subscription tier
may be added post-launch if institutional demand is observed — see §12.2.

---

## 11. Alignment with the tokenomics whitepaper

| Whitepaper section | How the model aligns with it |
|---|---|
| §1.1 *minimal cost per million* | Bond ~500 mAFT/record in the nominal regime = ~$10K/million. Stays compatible with institutional mass-ingest. |
| §2.1 *supply 1 B, never issued beyond* | Bond goes to the Treasury (recycling), never burned. Supply preserved. |
| §2.2 *26 % community rewards (260 M)* | The MIDDS Treasury can top up this pool when it runs out (~10-14 years). |
| §4 *vesting / sell pressure* | The bond creates a *circulation lock for at least 7d* — a small but real reduction of immediate sell pressure. |
| §5.3 *buyback and redistribution* | The MIDDS Treasury is the natural instrument of this strategy. Finalized bonds fund the buybacks/redistribution without requiring inflation. |

### 11.1 Estimate of Treasury revenue at different adoption levels

At an average bond of 500 mAFT (nominal regime):

| Scenario | Deposits/year | Treasury revenue/year | % supply |
|---|---|---|---|
| Bootstrap year 1 | 100 K | 50 K AFT | 0.005 % |
| Modest adoption | 1 M | 500 K AFT | 0.05 % |
| Strong adoption | 10 M | 5 M AFT | 0.5 % |
| V1 target saturation | 10 M (200 K/week) | 5 M AFT | 0.5 % |
| Massive adoption | 100 M | 50 M AFT | 5 % |

At 100 M deposits/year with an average multiplier of ~2× under load, we
are talking about ~100 M AFT/year of Treasury revenue — equivalent to
~38 % of the initial community rewards pool each year. Largely sufficient
to sustain the incentives beyond the depletion of the initial allocation,
and to significantly fund the Foundation's R&D / operations.

---

## 12. UX and user-facing communication

### 12.1 External pitch

> *"The Allfeat network ingests up to ~200 000 MIDDS per week at the floor
> price (~140 mAFT, ~$0.003 for a standard title). The rate increases with
> the size of the payload — a typical single costs ~$0.003, a full album
> with a rich tracklist can reach ~$0.05. The price also rises with the
> network load; tip: register your metadata at the start of the week for
> the optimal rate, like preparing your setlist before the concert. You
> have 7 days after the deposit to correct or cancel with a full refund."*

Deliberately non-technical. No mention of "EIP-1559", "multiplier" or
"payload-aware" in the public communication.

### 12.2 RPC to expose

```rust
// midds-runtime-api v2
fn current_deposit_price(payload_size: u32) -> Balance;
fn current_multipliers() -> (FixedU128, FixedU128);  // (fast, slow)
fn weekly_target() -> u32;
fn weekly_actual() -> u32;
```

Allows dashboards / wallets / front-ends to display live:
- "Current rate: 520 mAFT for this MIDDS"
- "Network load this week: 87 % of the target"
- 24h chart of the multiplier

This is what makes the mechanism **legible** on the user side. Without
this RPC, the dynamic price seems arbitrary; with it, it becomes an
understandable market signal.

---

## 13. Future evolutions (out of scope for V1)

### 13.1 Subscription tier for institutions

When a stable institutional demand is observed (CMOs, aggregators
ingesting > 10 K MIDDS/month over several months), add a *credits*
pre-payment mechanism:

- Purchase of N credits at a fixed price (= average price of the last
  epoch)
- Each credit consumes a deposit slot at the floor price
- Volume-based degressive discount (e.g.: -10 % from 10 K credits)
- Locks the price for the buyer, guarantees revenue for the Treasury

To be calibrated post-launch, on real usage data.

### 13.2 PoM integration

When the PoM pallet exists:

- MIDDS *certified* by PoM may possibly benefit from a reduction on their
  bond (rewards quality)
- `force_remove_slash` can be triggered by a negative PoM consensus rather
  than sudo
- The PoM reward pool can be partially funded by the MIDDS Treasury

The interface between the two pallets will be spec'd in `docs/plan-pom.md`
(to be created).

### 13.3 Multi-instance and differentiated costs

When Recording and Release are added (each as a new pallet Instance), the
economic constants may diverge:

- Recording probably more voluminous than MusicalWork → the per-byte bond
  weighs more
- Release even more complex → may justify a higher base bond
- Each Instance carries its own `parameter_types!`

The multi-instance architecture allows it without refactoring.

### 13.4 Auto-calibration of `MiddsDepositBase` *(in progress)*

Rather than freezing `MiddsDepositBase` and `MiddsDepositPerByte` in the
runtime, move them to `StorageValue`s adjustable by sudo based on
long-term observations (months, quarters). Allows recalibrating the target
by governance without a runtime upgrade — useful if the AFT/USD ratio
moves significantly.

**Status**: implementation in progress on `pallet-midds` v0.2.0. The
Config trait loses `type DepositBase` / `type DepositPerByte` (breaking
change pre-1.0), replaced by two `StorageValue<BalanceOf<T, I>>`
initialized via `GenesisConfig` and mutable via two sudo extrinsics
(`force_set_deposit_base`, `force_set_deposit_per_byte`). The bond formula
and the pricing RPCs now read the storages instead of calling
`T::DepositBase::get()`.

**Why now**: at $0.02/AFT calibration B is sound, but an AFT price move of
5× (very plausible on testnet → mainnet) would otherwise force a runtime
upgrade just to restore the target pricing. Better to lay the governable
rail before the need becomes urgent.

---

## 14. Implementation roadmap

| Phase | Tasks | Estimated effort |
|---|---|---|
| **A. Decision** | This document merged into `docs/`, `plan.md` § 2 updated to point to the new decisions | ✅ this PR |
| **B. Storage refactor** | `IdentifierClaims`, `PayloadHashes`, extended `Deposit` struct, `PendingFinalization`, multipliers | 4 h |
| **C. Core extrinsics** | `deposit` (with multipliers + queue), `update`, `remove_own`, `finalize` | 4 h |
| **D. Sudo extrinsics** | `force_edit`, `force_remove_refund`, `force_remove_slash`, `force_remove_many` | 3 h |
| **E. Runtime hooks** | `on_initialize` (finalizations + multiplier adjustment), helpers | 3 h |
| **F. Unit tests** | Multi-claim, refund, finalize, non-refunded premium, sudo split cases | 6 h |
| **G. Property tests** | Storage invariants, multiplier monotonicity, supply conservation | 5 h |
| **H. Mass injection** | Realistic load profiles (music release patterns), `storage_root_hash` recalibration | 3 h |
| **I. Benchmarks** | Re-bench of all extrinsics + parametrized `on_initialize` | 5 h |
| **J. Runtime API v2** | `current_deposit_price`, `current_multipliers`, `weekly_target/actual`, multi-claim lookup | 3 h |
| **K. Melodie runtime** | Wiring of the new `parameter_types!`, MiddsTreasuryAccount, transaction_payment update | 2 h |
| **L. Doc & comments** | Update of `plan.md` § 2, runtime comments, crates README | 2 h |
| **Total** | | **~40 h ≈ 1 week** |

---

## 15. Open questions (to settle before phase B)

1. **Percentage of tx fee revenue redirected to the Treasury**: currently
   100 % to the block author. Should we recover a fraction (e.g.: 20 %) to
   double the Treasury's revenue base? Out of direct scope but to be
   arbitrated in parallel.

2. **Multi-instance: common or per-instance bond?** When Recording
   arrives, should it share `MiddsTreasuryAccount` or have its own
   account? Recommendation: shared, simplicity.

3. **Publicly exposed "health" metric**: should there be an on-chain event
   `WeeklyMetricsSnapshot { deposits, avg_multiplier, treasury_balance }`
   every Sunday, to ease tracking by indexers and dashboards?
   Recommendation: yes, little weight and great transparency benefit.

4. **`update` behavior when `M_slow` has changed**: if the user updates and
   the new computed bond is lower than the old one (multiplier came back
   down), do we refund the difference? Or do we keep the initial bond?
   Recommendation: partial refund for consistency with the principle "you
   pay the current market price" — but to be arbitrated.

5. **Rate display on the CLI side**: should `midds-cli deposit` do an RPC
   round-trip to show the price before confirmation? Recommendation: yes,
   with a `--max-price` flag to fail if the price exceeds a threshold (UX
   similar to a slippage limit).
