# MIDDS SDK — Testing & mocking plan

> Reference document for the testing and mocking strategy of the
> `midds-sdk` repo. Counterpart to `docs/plan.md`. Goal: a stable and
> professional harness covering unit, property-based, mass, real fees,
> end-to-end, and seeding of dev chains for the frontend.

---

## 1. Guiding principles

- **`midds-fixtures` is the cornerstone**: a single source of truth
  for "what a plausible MIDDS looks like". Every layer depends on it.
- **No runtime duplication**: layers 1–3 live in `midds-sdk` (mock
  FRAME), layer 4 lives in `Allfeat` (real melodie-runtime), layer 5 is
  portable via `midds-cli`.
- **Deterministic by default**: seeded RNG so that a "10,000 MIDDS
  generated" test is reproducible bit-for-bit.
- **No separate `midds-loadgen`**: we extend `midds-cli` with
  `bench` and `seed` subcommands (consistent with its role as an operator
  client).
- **One layer = one distinct question**. No overlap of
  responsibility: if a bug can be caught at multiple levels, it is
  caught at the lowest.

---

## 2. Overview — 5 layers

| Layer | Question it answers | Location | Tooling |
|---|---|---|---|
| 1. Unit pallet | Does the lifecycle work? | `pallets/pallet-midds/src/tests.rs` | mock FRAME, `MockMidds` |
| 2. Property-based pallet | Do the invariants hold over 10k generated cases? | `pallets/pallet-midds/src/property_tests.rs` | `proptest` on the mock |
| 3. Mass injection mock | Do storage / weights / cumulative bond scale? | `pallets/pallet-midds/tests/mass_injection.rs` | mock FRAME + loop N=10k–100k |
| 4. Runtime integration | **Real fees** on melodie-runtime | `Allfeat/runtime/melodie/tests/midds_integration.rs` | `TestExternalities` on `melodie-runtime` |
| 5a. Inter-crate E2E (auto tests) | All SDK seams speak the same shape | `crates/midds-e2e/tests/` | external `--dev` node (`MIDDS_E2E_WS`) + subxt + `midds-client` |
| 5b. E2E node (operator tooling) | Real inclusion, fees, throughput, multi-account | `crates/midds-cli/src/bench/` | `--dev` node + subxt + `midds-client` |

---

## 3. Crate `midds-fixtures` (NEW)

Location: `crates/midds-fixtures/`. Std-only, no no_std.

### 3.1 Layout

```
crates/midds-fixtures/
├── Cargo.toml
├── data/                         # committed JSON
│   ├── iswc_real_sample.json     # ~500 valid ISWCs (anonymized)
│   ├── ipi_codes.json
│   ├── languages.json
│   └── titles_corpus.json        # corpus of plausible titles
└── src/
    ├── lib.rs
    ├── musical_work/
    │   ├── strategy.rs           # proptest::Strategy
    │   ├── builder.rs            # builder pattern (ergonomic test)
    │   └── corpus.rs             # access to static fixtures
    ├── identifiers.rs            # valid ISWC, IPI, ISNI (checksum-correct)
    ├── pathological.rs           # borderline / pathological cases
    └── rng.rs                    # SeededRng helper
```

### 3.2 Public API

- `MusicalWorkBuilder`: `.with_iswc(...).with_title(...).build()` for
  readable unit tests.
- `arb_musical_work()`: `proptest::Strategy<Value = MusicalWork>` for
  property tests.
- `arb_musical_work_max_size()`: payloads at exact `MaxEncodedLen`.
- `arb_musical_work_invalid()`: systematically generates cases that
  must fail validation.
- `corpus::iter_real_iswcs()`: iterator over the real dataset.
- `gen_n(seed, count) -> Vec<MusicalWork>`: deterministic mass
  generation for seed/loadgen.

### 3.3 Cargo features

- `default = ["proptest"]`
- `proptest`: enables `proptest::Strategy`
- `corpus`: embeds the JSON in the binary (otherwise read at runtime
  from `CARGO_MANIFEST_DIR`)

### 3.4 Static datasets

The JSON files are anonymized but structurally realistic (charset, length,
distribution). To be regenerated if the identifier spec evolves. No
GDPR-sensitive data (no real author names, just industry codes).

---

## 4. Layer 1 — Unit tests pallet (existing, to refine)

File: `pallets/pallet-midds/src/tests.rs` (already 435 lines).

**Action**: minimal refactor to consume `midds-fixtures` instead of the
ad-hoc helpers. Keep the current list of cases (lifecycle, freeze window,
`force_*`, errors). Verify coverage of the `MutateHold` branches
(hold/release/transfer-on-slash).

No adding of cases here — the new cases go in layers 2 and 3.

---

## 5. Layer 2 — Property-based pallet (NEW)

File: `pallets/pallet-midds/src/property_tests.rs` (gated `#[cfg(test)]`).

### 5.1 Invariants to prove

Each invariant = a dedicated `proptest!` block.

| Invariant | Description |
|---|---|
| `bond_formula` | `bond_held(account) == DepositBase + DepositPerByte * encoded_size(midds)` after each `deposit()` |
| `force_remove_releases` | `force_remove()` releases exactly the bond initially held (no more, no less) |
| `update_preserves_id` | `update()` never modifies the canonical identifier nor `NextMiddsId` |
| `freeze_window_blocks_update` | Any `update()` within `< owned_since + UpdateWindow` returns `Error::Frozen` |
| `unique_canonical_id` | No sequence of operations allows two `MusicalWork` with the same ISWC in storage |
| `encoded_len_consistency` | `midds.encoded_size() <= <Midds as MaxEncodedLen>::max_encoded_len()` for any MIDDS coming from `arb_musical_work()` |
| `events_match_storage` | For any sequence of extrinsics, the emitted events reflect the storage diffs |

### 5.2 Volume

- Default: `proptest_cases = 256` (fast PR).
- Nightly CI override: `PROPTEST_CASES=10000`.
- Counter-example persistence: `proptest-regressions/` committed in the
  repo (standard `proptest` practice).

---

## 6. Layer 3 — Mass injection on mock (NEW)

File: `pallets/pallet-midds/tests/mass_injection.rs` (integration
test, outside `#[cfg(test)]`).

### 6.1 Scenarios

| Scenario | N | Accounts |
|---|---|---|
| `mass_injection_10k` | 10,000 | 1 |
| `mass_injection_50k` | 50,000 | 100 |
| `mass_injection_100k` | 100,000 | 1,000 (CI nightly only) |
| `mass_injection_max_size` | 1,000 | each MIDDS at `MaxEncodedLen` |

### 6.2 Recorded measurements

Each test outputs a file
`target/test-reports/mass_injection_<scenario>.json`:

```json
{
  "scenario": "mass_injection_10k",
  "n": 10000,
  "total_bond_held": "...",
  "avg_encoded_size_bytes": ...,
  "storage_root_hash": "0x...",
  "wall_time_ms": ...,
  "peak_memory_kb": ...
}
```

### 6.3 Anti-regression

`storage_root_hash` is checked against a committed fixture
(`tests/fixtures/storage_root_10k.txt`). If the bond formula or the
encoding changes, the test fails with an explicit diff. Forces a
conscious update.

---

## 7. Layer 4 — Runtime integration tests (on the `Allfeat` side)

**Outside `midds-sdk`**: lives in
`Allfeat/runtime/melodie/tests/midds_integration.rs`.

### 7.1 Why elsewhere

`midds-sdk` must not depend on `melodie-runtime` (cf. the
melodie/mainnet decoupling decision). The runtime on the `Allfeat` side already consumes the SDK
as a path-dep, so the reverse would create a cycle.

### 7.2 Setup

Dependencies: `melodie-runtime` + `midds-fixtures` + `sp-io`.

`TestExternalities` built from `melodie-runtime::GenesisConfig::default()`
with pre-minted balances for 100 accounts.

### 7.3 Real-fee scenarios

| Test | Measurement |
|---|---|
| `fees_small_musical_work` | bond + tx fee for MusicalWork ~50 bytes |
| `fees_avg_musical_work` | bond + tx fee for MusicalWork ~200 bytes |
| `fees_max_musical_work` | bond + tx fee for MusicalWork at `MaxEncodedLen` |
| `fees_distribution_1000` | full distribution (p50 / p95 / p99) over 1k MIDDS coming from `arb_musical_work()` |

### 7.4 Output

`target/test-reports/fees_report.md` — commit-able markdown table in the
PR:

```
| Size (bytes) | Bond (AFT) | Weight fee (AFT) | Length fee (AFT) | Total user cost |
|--------------|------------|------------------|------------------|-----------------|
| 50           | ...        | ...              | ...              | ...             |
```

Serves as a baseline for deciding whether to tune `DepositBase` /
`DepositPerByte`.

---

## 8. Layer 5 — E2E node

Layer 5 is split into two separate deliverables but consuming the same `--dev` node:

### 8.a Crate `midds-e2e` — automated inter-crate tests

`crates/midds-e2e/` gathers the `[[test]]` targets that run the whole
stack (`types → pallet → runtime-api → rpc → client → cli`) against an external
node — its reason for being is to guarantee that all seams speak the
same shape.

#### 8.a.1 Layout

```
crates/midds-e2e/
├── Cargo.toml          # publish = false
├── src/
│   ├── lib.rs          # re-exports the scaffolding
│   ├── env.rs          # MIDDS_E2E_WS lookup
│   ├── client.rs       # try_connect() → silent skip if no node
│   ├── signer.rs       # //Alice, //Bob
│   ├── session.rs      # base index = nanos % 10^9, atomic slot per test
│   └── tx.rs           # raw helpers (update, force_remove_refund, sudo,
│                       #   free_balance) that bypass midds-client
└── tests/
    ├── happy_path.rs       # deposit → lookup → get → assert identical payload
    ├── update_window.rs    # update within the window → assert mutated payload
    ├── force_admin.rs      # sudo force_remove_refund → assert bond refunded
    ├── rpc_namespace.rs    # JSON-RPC calls midds_musicalWorks_*
    └── cli_smoke.rs        # `cargo run -p midds-cli -- seed --count 5`
```

#### 8.a.2 Skip contract

No node = `cargo test -p midds-e2e` stays green. Each test starts with
`let Some(client) = client::try_connect().await else { return; }`. URL
resolution:

```
MIDDS_E2E_WS=ws://… cargo test -p midds-e2e   # explicit node
cargo test -p midds-e2e                       # ws://127.0.0.1:9944 by default
```

#### 8.a.3 Run isolation

All tests share the same chain, so state accumulates between runs.
`session::fresh_musical_work()` mints deterministic ISWCs via
`base_index = nanos_since_epoch % 10^9` + an atomic slot per test, which
guarantees that no successive run hits `AlreadyExists` and that no
parallel test steps on another.

#### 8.a.4 Wiring against Allfeat

The `allfeat --dev` node is launched separately by the operator (or a CI
job that starts it in the background before the `cargo test`). No Rust
dependency on `../Allfeat` from the SDK side — the boundary stays the WS
RPC.

```bash
# Terminal 1
../Allfeat/target/release/allfeat --dev --tmp

# Terminal 2
cargo test -p midds-e2e
```

### 8.b Operator tooling — `midds-cli`

Independently of the tests, the CLI keeps exposing operator
subcommands for manual live-node use.

#### 8.b.1 Added subcommands

```
midds-cli seed
  --node ws://localhost:9944
  --count 50000
  --rng-seed 0xABCD...                # optional, default = deterministic
  --concurrency 16                     # extrinsics in-flight
  --signers alice,bob,//Alice//1..100  # multi-account
  --report seed_report.json

midds-cli bench fees
  --node ws://localhost:9944
  --count 1000
  --size-distribution real|max|mixed
  --out fees_report.md

midds-cli bench throughput
  --node ws://localhost:9944
  --count 100000
  --duration 10m
  --out throughput_report.json

midds-cli verify-state
  --node ws://localhost:9944
  --expected-count 50000
  --expected-storage-root 0x...
```

#### 8.b.2 Internal architecture

- Module `crates/midds-cli/src/bench/` (mod.rs, seed.rs, fees.rs,
  throughput.rs, verify.rs).
- Reuses `midds-client` for the extrinsics, `midds-fixtures` for the
  generation.
- The seed reports are replay-verifiable via `verify-state`.

#### 8.b.3 Multi-account

Deterministic derivation `//Alice//<N>` (up to several thousand).
Pre-funding via a dedicated subcommand:

```
midds-cli admin pre-fund-signers --count 1000 --amount 1000AFT
```

Implemented via a `force_set_balance` (sudo) extrinsic on the dev chain
side. To be documented clearly as a dev-only tool.

---

## 9. Snapshot workflow for frontend

Documented in `docs/seeding.md` (to be created in parallel).

### 9.1 Typical workflow

```bash
# 1. Boot dev node
allfeat --chain melodie-dev --base-path /tmp/midds-seed --tmp

# 2. Pre-fund signers
midds-cli admin pre-fund-signers --count 100

# 3. Seed via midds-cli
midds-cli seed --count 50000 --rng-seed 0xDEADBEEF --report seed.json

# 4. Verify
midds-cli verify-state --expected-count 50000

# 5. Export state
allfeat export-state --chain melodie-dev > seeded-state.json

# 6. Frontend bind
allfeat --chain ./seeded-state.json
```

### 9.2 Snapshot distribution

The `seeded-state.json` file can be:
- committed in a separate fixtures repo (if <100 MB),
- or a GitHub release-asset attached to a SDK tag (recommended for sizes
  >100 MB).

**Reproducibility**: a fixed `--rng-seed` guarantees that the snapshot is
regenerable bit-for-bit. CI can produce a new snapshot at each
release.

---

## 10. Weights benchmarks (existing, to extend)

File: `pallets/pallet-midds/src/benchmarking.rs` (currently 108
lines).

**Action**: add the worst-case with `MaxEncodedLen` (uses
`midds-fixtures::arb_musical_work_max_size`) for all extrinsics.
Regenerate the weights via `frame-omni-bencher` once per SDK release.

**Orthogonal** to the user perf benchmarks (Layer 5 throughput) —
do not confuse. Here we calibrate the FRAME weights, not the network throughput.

---

## 11. CI cadence

| Step | Trigger | Target duration |
|---|---|---|
| Layers 1 + 2 (default proptest cases) | Each PR | <2 min |
| Layer 3 (10k only) | Each PR | <5 min |
| Layer 3 (100k) + property with `PROPTEST_CASES=10000` | Nightly | <30 min |
| Layer 4 (Allfeat side) | Nightly Allfeat | <10 min |
| Layer 5a (`midds-e2e`) | Manual local for V1, to wire in CI later | <2 min after node boot |
| Layer 5b throughput | Manual + release tag | variable |
| Weights regeneration + seeded snapshot | Release tag | <1h |

---

## 12. Pathological cases to cover explicitly

To be distributed across the right layers, but listed once so nothing is
forgotten:

- MIDDS at exact `MaxEncodedLen` (max bond).
- Minimal MIDDS (min bond, but ≥ ED).
- Borderline charset (boundary ASCII characters).
- Account without sufficient funds for the bond.
- Update exactly at `owned_since + UpdateWindow` (off-by-one).
- 10k MIDDS from a single account (huge cumulative bond).
- Concurrency: two simultaneous updates on the same MIDDS (transactionality).
- Fictitious V1→V2 storage migration (test that the
  `OnRuntimeUpgrade` mechanism works).
- Canonical ID with collision (clean rejection).
- `force_remove` of a MIDDS in freeze window (must pass, proves that
  sudo bypass works).

---

## 13. Execution plan

Order that maximizes incremental value. Each step is mergeable
independently.

| # | Step | Immediate benefit |
|---|---|---|
| 1 | `midds-fixtures` skeleton + datasets + `MusicalWorkBuilder` | Unblocks everything else |
| 2 | Refactor Layer 1 to use fixtures | Validates the API |
| 3 | Layer 2 property tests | Likely finds latent bugs |
| 4 | Layer 3 mass injection | Sets the `storage_root` baseline (solid anti-regression) |
| 5 | `midds-cli seed` + `verify-state` | Unblocks the frontend immediately |
| 6 | Layer 4 Allfeat side | Concrete fees report for tuning decisions |
| 7 | `midds-cli bench fees` + `bench throughput` | Operator tooling |
| 8 | Snapshot workflow doc + CI release artifact | Industrialization |
| 9 | Final audit of pathological cases | Safety net |

---

## 14. Conventions

- All test reports: `target/test-reports/<scenario>.{json,md}`,
  stable format, parsable by CI.
- `proptest_cases` configurable via env var, never hardcoded.
- RNG: seeded `SmallRng`, never `thread_rng()` in reproducible
  tests.
- Multi-account in E2E: `//Alice//<N>` derivation, never a hardcoded key
  outside `Alice`/`Bob`.
- Fixture datasets: no real nominative data. Industry codes
  only (synthetic but checksum-correct ISWC/IPI/ISNI).

---

## 15. Cross-crate tests (outside pallet)

Layers 1–5 cover the on-chain flow. But the SDK hosts several
crates, each of which deserves its own coverage. Recap per crate.

### 15.1 `midds-traits` (no_std, pure)

- Unit tests on each `validate_*_format` (charset, length, structure).
- Explicit negative cases for each branch of `MiddsFormatError`.
- No dedicated proptest: surface too small for the ROI.
- Location: `crates/midds-traits/src/identifier/tests.rs` (already partial).

### 15.2 `midds-types` (no_std)

- SCALE encode/decode roundtrip for any MIDDS coming from `arb_musical_work()`.
- Invariant: `encoded_size(midds) <= <Midds as MaxEncodedLen>::max_encoded_len()`.
- serde JSON roundtrip under `--features serde` (serialized then deserialized,
  bit-identical).
- Location: `crates/midds-types/tests/encoding.rs`.

### 15.3 `midds-validate` (std, offline)

Critical coverage because it is the public API for the upstream tools
(catalog editors, importers).

- For each tolerant parser: `valid` / `canonicalisable` / `invalid`.
- Checksum verifiers: warnings emitted, never blocking.
- `MusicalWorkBuilder`: every successful build produces a payload that also
  passes `<MusicalWork as Midds>::validate_format`.
- **Key invariant**: on-chain ⊆ off-chain. Any payload that passes on-chain
  passes off-chain; the reverse is not true (off-chain accepts
  warnings).
- Location: `crates/midds-validate/src/musical_work/tests.rs`.

### 15.4 `midds-rpc` (std)

- Integration test with a minimal `impl MiddsRuntimeApi for TestApi`
  (in-memory stub, no node).
- Assert on the JSON shape: `lookup_by_identifier` on a nonexistent ID
  returns `null`, not an error.
- Location: `crates/midds-rpc/tests/rpc.rs`.

### 15.5 `midds-client` (std)

- Covered essentially by Layer 5 (subxt::dynamic executed against
  a `--dev` node).
- One useful unit test: `codec_bridge::EncodedCall` roundtrip with
  `parity_scale_codec::Encode` (safety against a regression of the bridge).
- No mock node, no mocked subxt — all the value of the dynamic choice
  comes from real execution, mocking it degrades it.

### 15.6 `midds-codegen` (std bin)

Surface here (CLI smoke):

- CLI smoke: `--help` / `--version` / missing args / nonexistent
  metadata path. Guardrail on the wrapper layer (clap, `is_url`,
  `from_file_blocking`) — the only code that `midds-codegen` actually
  owns.
- Location: `crates/midds-codegen/tests/cli_smoke.rs`.

Full codegen snapshot (deferred to the Allfeat side):

- The original idea (successful generation from a committed SCALE metadata
  + `cargo check` on the produced binding) requires the real metadata of
  `melodie-runtime`. Committing it here would contradict the SDK / runtime
  decoupling locked by `CLAUDE.md` ("the runtime side is in
  `../Allfeat`") and the metadata drifts at each runtime release, so the
  refresh cadence belongs to the runtime, not the SDK.
- Target location: `Allfeat/runtime/melodie/tests/codegen_snapshot.rs`
  (or a CI step that regenerates + diffs the bindings from a
  `melodie-dev` node).

### 15.7 `midds-runtime-api` (no_std)

No direct test — declarations only. Implicitly covered by
Layer 4 (impl on the runtime side) and Layer 5 (consumed via RPC).

---

## 16. Migration / versioning tests

The strategy locks in versioned top-level enums
(`enum MusicalWork { V1(...), V2(...) }`). Adding a variant is
additive. But it must be mechanically proven that this is non-breakable.

### 16.1 Invariants to prove at each variant addition

| Invariant | Description |
|---|---|
| Wire stability | A SCALE-encoded `MusicalWorkV1` stays decodable as `MusicalWork::V1` after adding V2 |
| Storage stability | `Items<MiddsId, MusicalWork>` requires no migration on a pure variant addition |
| Identifier stability | `identifier()` returns the same value for a V1, independently of the added variants |

### 16.2 Mechanics

File: `crates/midds-types/tests/version_stability.rs`.

```rust
#[test]
fn v1_payload_stays_decodable() {
    let v1_bytes = include_bytes!("fixtures/musical_work_v1.scale");
    let decoded = MusicalWork::decode(&mut &v1_bytes[..]).unwrap();
    assert!(matches!(decoded, MusicalWork::V1(_)));
}
```

`musical_work_v1.scale` is committed as an **immutable fixture**. Any
accidental modification of the V1 wire format makes the test fail — that's
exactly the net being sought.

### 16.3 OnRuntimeUpgrade

When a real migration becomes necessary (structural change
forced by a business constraint), it will live in
`Allfeat/runtime/melodie/migrations/` with its test in
`Allfeat/runtime/melodie/tests/migrations.rs`. Not in the SDK.

**Rule**: the SDK guarantees the wire format of the MIDDS; the runtime manages
its own storage migrations. Clean boundary.

---

## 17. Artifacts & commit policy

| Artifact | Location | Commit? |
|---|---|---|
| Reports `target/test-reports/*.{json,md}` | local + CI artifact | **no** (gitignored) |
| `proptest-regressions/` | repo | **yes** (standard proptest practice) |
| `tests/fixtures/storage_root_*.txt` | repo | **yes** (Layer 3 anti-regression) |
| Datasets `crates/midds-fixtures/data/*.json` | repo | **yes** (determinism) |
| `crates/midds-types/tests/fixtures/musical_work_v1.scale` | repo | **yes** (wire stability) |
| `crates/midds-codegen/tests/fixtures/metadata.scale` | repo | **yes** (codegen snapshot) |
| `seeded-state.json` (seeded dev chain) | GitHub release asset | **no** (size) |
| `seed_report.json` | local | **no** (regenerable from seed) |

**General rule**: if it is 100% deterministically regenerable
from the code + a seed, we do not commit it. Otherwise we commit it. The `.gitignore`
must reflect this rule, not contradict it.

---

## 18. Open items

- Precise choice of the mass injection lib (subxt direct vs `txwrapper`).
- Exact format of the `seed_report.json` (to be frozen after the Layer 5 PR).
- Rotation policy for seeded snapshots on releases (how many we
  keep, how many we publish).
- `criterion` or `divan` integration for the real user perf
  benchmarks (Layer 5) — to be decided when we tackle step 7.
