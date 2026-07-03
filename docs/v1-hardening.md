# V1 hardening plan — MusicalWork before scale-out

> Planning document: what needs to be consolidated on the MIDDS SDK **before**
> adding Recording and Release. Counterpart to [`plan.md`](./plan.md),
> [`economics.md`](./economics.md) and [`testing.md`](./testing.md). Derived from a
> complete audit (pallet / traits-types-validate / client-RPC-CLI /
> fixtures-CI) carried out on 2026-05-04.

---

## 1. Context

V1 targets **MusicalWork end-to-end**. The generic architecture (`trait Midds`
+ multi-instance pallet) is in place but has never been exercised by a
second type. The audit revealed three families of problems:

1. **Structural choices still fixable without pain** (trait, client
   API, runtime API) that will become *breaking* as soon as a 2nd instance
   is in production.
2. **Bugs/ambiguities in the pallet** on invariants that are documented but not
   tested (window bounds, cumulative bond, dead branches).
3. **Extension friction** in `midds-fixtures`, `midds-cli` and `bench/`
   that will force copy-paste with every new type.

Goal of this plan: close these three fronts in 4 PR-sized sprints
(~5–6 days total) **before** touching Recording.

---

## 2. Sequencing

| Sprint | Goal | Effort | Blocks | Blocked by |
|---|---|---|---|---|
| A | Enriched `trait Midds` + complete client/runtime-api API | 1.5–2d | Recording, Release | — |
| B | Pallet bugs + invariants covered by property tests | 1d | — | — |
| C | Genericize `midds-fixtures`, `midds-cli` and `bench/` over `M: Midds` | 1.5–2d | Recording (corpus, builders) | A |
| D | Hardened CI + doc debt + quick wins | 0.5d | release-plz prod-ready | — |

A and B are independent → can start in parallel. C depends on A for
the generic client API. D can slot in at any time.

---

## 3. Sprint A — Enriched `trait Midds` + complete client/runtime-api API

### 3.1 Goal

Freeze the public contracts that touch all instances (trait,
runtime API, RPC namespace, client façade) **now**, while there is
only a single impl to migrate. These are the only changes in this plan that
are genuinely SCALE-*breaking*.

### 3.2 Tasks

#### 3.2.1 `trait Midds` — `KIND` + `identifier()` by reference

`crates/midds-traits/src/lib.rs:20`

```rust
pub trait Midds: Parameter + MaxEncodedLen {
    /// Stable string discriminator (used by events, RPC, indexers).
    /// Convention: PascalCase singular ("MusicalWork", "Recording", "Release").
    const KIND: &'static str;

    type Identifier: Parameter + MaxEncodedLen + Ord;

    fn identifier(&self) -> &Self::Identifier;  // was: -> Self::Identifier
    fn validate_format(&self) -> Result<(), MiddsFormatError>;
}
```

- Update `crates/midds-types/src/musical_work/mod.rs` (impl Midds).
- Update `pallets/pallet-midds/src/{lib,mock}.rs` — each `.identifier()`
  loses its `.clone()`.

#### 3.2.2 `MiddsFormatError` — forward-looking variants

`crates/midds-traits/src/error.rs:11`

Add `DateInconsistency` and `CrossFieldInconsistency` (planned
`plan.md:166`, never implemented). Recording and Release will need them
(year vs release_year, tracklist vs durations). It's a SCALE `enum` → any
later addition would be breaking.

#### 3.2.3 Runtime API — typed `DepositInfoOf`

`crates/midds-runtime-api/src/lib.rs:41`

Replace the tuple `(AccountId, Balance, Balance, bool)` with:

```rust
#[derive(Encode, Decode, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug)]
pub struct DepositInfoOf<AccountId, Balance> {
    pub depositor: AccountId,
    pub amount: Balance,
    pub price_at_deposit: Balance,
    pub finalized: bool,
}
```

Propagate it into `midds-rpc/src/lib.rs:39` (the `DepositInfoView` alias to be removed
in favor of the shared type) and into `crates/midds-client/src/pallet/api.rs:381`
(manual tuple decoding to be removed).

#### 3.2.4 Client — type-safe query methods

`crates/midds-client/src/pallet/api.rs`

The private helper `runtime_api<T: Decode>` already exists; add publicly
on `PalletApi<M: Midds>`:

```rust
pub async fn lookup_by_identifier(&self, id: &M::Identifier) -> Result<Vec<MiddsId>>;
pub async fn get(&self, id: MiddsId) -> Result<Option<M>>;
pub async fn deposit_info(&self, id: MiddsId) -> Result<Option<DepositInfoOf<AccountId, Balance>>>;
```

No new logic — trivial wiring, alignment with the runtime API.

#### 3.2.5 RPC — namespacing prepared for multi-instance

`crates/midds-rpc/src/lib.rs:46`

The comment (l. 11-20) acknowledges the debt. Two options:

- **Option 1 (preferred)**: a `define_midds_rpc!(MusicalWorks)` macro that
  generates the `#[rpc(server)]` trait with the `midds_musicalWorks_*` prefix.
- **Option 2 (minimal)**: a `rename_methods(module: RpcModule, prefix: &str)` helper
  so the node renames at registration time, without a macro.

Choose Option 1 if the macro cost stays reasonable (~50 lines), otherwise
Option 2 documented as *opt-in on the node side*.

### 3.3 Tests

- `crates/midds-traits/tests/kind.rs` — assert `MusicalWork::KIND == "MusicalWork"`.
- `crates/midds-runtime-api/tests/deposit_info_codec.rs` — SCALE round-trip
  of `DepositInfoOf` (the tuple → struct migration **must** be backward-incompatible
  by design, but the test locks the new representation).
- `crates/midds-client/tests/query_smoke.rs` — call the 3 new methods
  against a subxt mock (or marked `#[ignore]` if too heavy, run in E2E).

### 3.4 Validation criteria

- `cargo test --workspace --all-features` green.
- `cargo build -p pallet-midds --no-default-features --target wasm32-unknown-unknown`.
- No occurrence of `.identifier().clone()` remains.
- `grep -rn 'tuple.*deposit_info\|(AccountId, Balance, Balance, bool)' crates/` returns 0.
- CHANGELOG: mark `BREAKING CHANGE: Midds trait now requires KIND const; identifier() returns &Identifier; runtime API DepositInfo is now a struct.`

---

## 4. Sprint B — Pallet bugs + invariants covered

### 4.1 Goal

Close the behavioral ambiguities detected in the pallet and **prove
by property tests** the documented invariants `plan.md:560-566`. Everything
that passes this sprint applies to all future instances for free.

### 4.2 Tasks

#### 4.2.1 Single bound for the 7d window

`pallets/pallet-midds/src/lib.rs:494,520,551`

Current state: `update`/`remove_own` use `elapsed <= window`,
`finalize` uses `elapsed > window`. At the exact `expiry` block, the
intra-block order decides who wins (non-deterministic for the user).

Proposed decision: **`<` everywhere**. The `expiry` block itself is
finalizable. Consistent with `economics.md` ("window strictly
less than 7 days"). Document in the `Config::CommitmentWindow` doc-comment.

Helper to extract: `fn ensure_in_window(info: &Deposit, now: BlockNumber)
-> DispatchResult` — used by `update` and `remove_own`.

#### 4.2.2 Bound `force_remove_many`

`pallets/pallet-midds/src/lib.rs:603`

```rust
#[pallet::constant]
type MaxRemovalsPerCall: Get<u32>;
```

Signature changes from `Vec<MiddsId>` to `BoundedVec<RemovalRequest, T::MaxRemovalsPerCall>`
where:

```rust
pub enum RemovalKind { Refund, Slash }
pub struct RemovalRequest { id: MiddsId, kind: RemovalKind }
```

Benefits: boundable weight, end of the global `slash: bool` flag (each id
can have its own handling). Mock runtime: `MaxRemovalsPerCall = 32`.

#### 4.2.3 Dead branch `do_apply_edit`

`pallets/pallet-midds/src/lib.rs:670-682`

Either remove the `DuplicatePayload` branch (intercepted earlier by
`IdentifierClaims::contains_key`), or add a test that forces the residual
path (two items with the same identifier, different payloads, a second update
that collides in hash with a third). Recommended decision: **keep + test**,
because the pre-check could be dropped in a future refactor.

#### 4.2.4 Property tests of the invariants `plan.md:560-566`

`pallets/pallet-midds/src/property_tests.rs`

Add a test generating an arbitrary sequence of extrinsics
(`deposit`, `update`, `remove_own`, `force_remove_*`, `on_initialize`)
then asserting after each step:

| Invariant | Assertion |
|---|---|
| Cumulative bond | `for each account: held(account) == Σ DepositInfo[id].amount where depositor==account` |
| Items↔DepositInfo coupling | `Items::iter().count() == DepositInfo::iter().count()` |
| PayloadHashes cardinality | `PayloadHashes::iter().count() == Items::iter().count()` |
| Immutable identifier | `forall id: identifier_at(t) == identifier_at(t-1)` |
| `NextMiddsId` monotonic | never decreasing |

Pathological cases to include: `arb_invalid_mock_midds()` producing
malformed payloads and asserting `assert_noop!(InvalidFormat)`.

#### 4.2.5 Tests for `current_deposit_price` runtime API

`pallets/pallet-midds/src/lib.rs:923`

Pin a mock test: returned price == bond actually debited by an
immediate `deposit` in the same block.

#### 4.2.6 Mass-injection test with multipliers active

`pallets/pallet-midds/tests/mass_injection.rs`

New scenario `mass_injection_10k_with_multipliers` targeting ~1000
extrinsics/block to push `M_fast` above 1.0×. Storage root
committed. **Without this, the `economics.md` logic is tested by no
fixture.**

### 4.3 Validation criteria

- All the invariants from `plan.md:560-566` have a dedicated prop test.
- `cargo test -p pallet-midds property_tests --release` passes with
  `PROPTEST_CASES=10000`.
- No `<=` / `>` bound remains on `elapsed` vs `CommitmentWindow`.
- `force_remove_many` accepts at most `MaxRemovalsPerCall` ids.

---

## 5. Sprint C — Genericize `midds-fixtures`, `midds-cli` and `bench/`

### 5.1 Goal

Kill every hardcoded mention of `MusicalWork` in the cross-cutting tools
so that **adding Recording = defining a struct + impl `Midds` + a
corpus**, full stop. Today it would be copy-paste 3×.

### 5.2 Tasks

#### 5.2.1 `midds-fixtures` — trait + per-type submodules

`crates/midds-fixtures/src/`

Extract into `lib.rs`:

```rust
pub trait MiddsFixtures {
    type Item: Midds;
    fn corpus() -> &'static [Self::Item];
    fn strategy() -> BoxedStrategy<Self::Item>;
    fn gen_n(seed: u64, n: usize) -> Vec<Self::Item>;
    fn pathological() -> Vec<Self::Item>;
}
```

Refactor `musical_work/{mod,builder,strategy,corpus}.rs` to implement
`MiddsFixtures for MusicalWorkFixtures`. Cross-cutting helpers
(`BoundedFieldStrategy<N>`, `ChecksumIdStrategy`) extracted into
`crates/midds-fixtures/src/common.rs`.

#### 5.2.2 ISRC support in `midds-validate` + `midds-fixtures`

Missing today:
- `crates/midds-validate/src/parse.rs`: `parse_isrc(s: &str) -> Result<Isrc, ParseError>`
  (tolerant regex `^[A-Z]{2}-?[A-Z0-9]{3}-?\d{2}-?\d{5}$`).
- `crates/midds-validate/src/checksum.rs`: `verify_isrc_checksum` (mod-7).
- `crates/midds-fixtures/src/identifiers.rs`: `isrc_valid_strategy()`,
  `isrc_invalid_strategy()` + a corpus of ~50 real ISRCs (`data/isrc_real_sample.json`).

(Will be consumed by Recording, but the effort is here because it is
mechanically cross-cutting.)

#### 5.2.3 `MusicalWorkBuilder` (mandated `plan.md:425`, never done)

`crates/midds-validate/src/lib.rs`

```rust
pub struct MusicalWorkBuilder { /* fields */ }
impl MusicalWorkBuilder {
    pub fn iswc(self, s: &str) -> Self;
    pub fn title(self, s: &str) -> Self;
    pub fn add_creator(self, ipi: &str) -> Self;
    pub fn build(self) -> Result<MusicalWork, BuildError>; // aggregates errors
}
```

Reusable pattern: doc-comment `// Recording/Release will follow this template`.

#### 5.2.4 CLI + bench parametrized over `M: Midds`

Targets:

- `crates/midds-cli/src/bench/{seed,fees,throughput,worker,util}.rs`
- `crates/midds-cli/src/admin.rs:21,151,171,227`

Minimal approach: CLI enum

```rust
#[derive(clap::ValueEnum, Clone)]
enum MiddsKind { MusicalWork /* + Recording, Release later */ }
```

The scaffolding functions (`setup_runner`, `partition_round_robin`,
`auto_fund`) become generic over `M: Midds + Encode`. The CLI commands
take `--midds-type <kind>` (default `musical-work`) and dispatch.

V1: only `MusicalWork` is wired — but the enum + dispatch are in place.
Recording = adding a variant.

#### 5.2.5 Pallet/event constants grouped together

`crates/midds-client/src/pallet/{api,events}.rs`

Today scattered (`"DepositBase"`, `"NextMiddsId"`, `"Deposited"`,
`"TransactionPayment"`…). Group them into a `pallet::names` module (or
better: injected via `Midds::KIND` from sprint A for the pallet prefix).

### 5.3 Validation criteria

- `grep -rn 'MusicalWork' crates/midds-cli/src/bench/` returns only
  the `MiddsKind` enum variants.
- `grep -rn 'MusicalWork' crates/midds-fixtures/src/lib.rs crates/midds-fixtures/src/common.rs`
  returns 0.
- `parse_isrc` + `verify_isrc_checksum` each have ≥ 5 pass cases + 5 fail cases.
- `MusicalWorkBuilder` has a "happy path" test + a "3-error aggregation" test.

---

## 6. Sprint D — Hardened CI + doc debt + quick wins

### 6.1 Goal

Close the holes that would let a silent regression slip through, and
clean the docs/repo of the inconsistencies identified.

### 6.2 Tasks

#### 6.2.1 CI — missing jobs

`.github/workflows/ci.yml`

Add:

```yaml
- name: Check pallet with runtime-benchmarks
  run: cargo check -p pallet-midds --features runtime-benchmarks
```

New workflow `.github/workflows/nightly.yml` (daily cron):

```yaml
- name: Mass injection 50k/100k
  run: cargo test -p pallet-midds --test mass_injection -- --ignored
- name: Property tests intensive
  env: { PROPTEST_CASES: "10000" }
  run: cargo test -p pallet-midds property_tests --release
```

#### 6.2.2 `release-plz` — required status checks

`.github/workflows/release-plz.yml:7-10` acknowledges that `GITHUB_TOKEN` does
not trigger CI on the release PR. Before the 1st public release:

- Either switch to a PAT/GitHub App (preferred).
- Or add a required-status-check on `master` that forces a manual CI
  re-run before merge.

#### 6.2.3 Hardened `clippy.toml`

`clippy.toml`

Add:

```toml
disallowed-methods = [
    { path = "core::result::Result::unwrap", reason = "use ? or expect with context" },
    { path = "core::option::Option::unwrap", reason = "use ? or expect with context" },
]
```

(Tests excepted via a localized `#[allow(clippy::disallowed_methods)]`.)

#### 6.2.4 `Justfile`

New root file:

```makefile
default:
    just --list

check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

test-fast:
    cargo test --workspace

test-nightly:
    PROPTEST_CASES=10000 cargo test -p pallet-midds property_tests --release
    cargo test -p pallet-midds --test mass_injection -- --ignored

wasm:
    cargo build -p pallet-midds --no-default-features --target wasm32-unknown-unknown

bless-fixtures:
    UPDATE_FIXTURES=1 cargo test -p pallet-midds --test mass_injection
```

#### 6.2.5 Doc — fix `plan.md`

`docs/plan.md`

- L. 445: `lookup_by_identifier(id) -> Option<MiddsId>` → `Vec<MiddsId>`
  (multi-claim confirmed).
- Section 5.4 `MusicalWorkBuilder`: if done in sprint C, mark ✅,
  otherwise note "implemented in Sprint C of v1-hardening".
- Point to this document (`v1-hardening.md`) at the top of section 6.

#### 6.2.6 Quick wins

- Root `seed.json`: add `/seed.json` to `.gitignore` and remove
  the file (orphan, never read).
- `crates/midds-types/src/language.rs:42`: add
  `from_code_ignore_ascii_case`.
- `crates/midds-traits/src/identifier.rs`: `static_assertions::const_assert_eq!`
  on the `max_encoded_len` of each alias (`Iswc`, `Isni`, `Ipi`, `Isrc`).

### 6.3 Validation criteria

- CI master: 6 jobs (fmt, clippy, test, wasm, bench-check, commitlint).
- CI nightly: configured and passes at least once.
- `just check` reproduces CI locally.
- `seed.json` no longer exists.

---

## 7. Out of scope (deliberately deferred)

- **Refactor `do_force_remove_slash` post-finalization** (`lib.rs:797-816`):
  correct behavior, just redundant harmless operations. Not
  blocking.
- **`apply_multipliers` precision on small bases**: a non-issue in
  prod (planck at 10^12 decimals). A mock test can be added in sprint B
  if trivial, otherwise ignore.
- **`SLOW_WINDOW_DAYS = 7` hardcoded** (`lib.rs:79`): per spec
  (`economics.md` §4) this 7d is **identical for all instances**.
  Configurability = YAGNI.
- **`midds-codegen` snapshot tests**: the crate is documented as
  "external consumers only", `midds-client` does not depend on it. No value
  in testing without a target runtime.
- **`midds-client` integration tests against an ephemeral node**: the
  E2E bench/ in `midds-cli` already covers the path. To reconsider if we
  introduce a 2nd instance without going through `midds-cli`.

---

## 8. Suggested kickoff

1. Open 4 GitHub issues (one per sprint) referencing this document.
2. Sprints A and B in parallel (two distinct PRs, separate branches).
3. Sprint C starts as soon as A is merged (client API dependency).
4. Sprint D can be split into sub-PRs (CI / clippy / quick wins
   independent).

Criterion for "v1 stabilized, ready for Recording":

- The 4 sprints merged onto `master`.
- A `0.2.0` release (or `0.1.x` depending on the pre-1.0 semver policy) cut
  via `release-plz`.
- CHANGELOG aggregating the `BREAKING CHANGE` entries from sprint A.
