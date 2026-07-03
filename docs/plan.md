# MIDDS SDK — V1 Implementation Plan

> Reference architectural document for the implementation of the `midds-sdk` repo.
> Target: MusicalWork end-to-end, a generic architecture enabling
> the trivial addition of Recording and Release afterwards.

---

## 1. V1 context and scope

**Allfeat**: Substrate blockchain dedicated to the music industry.
**MIDDS** (Music Industry Decentralized Data Structure): normalized objects stored
on-chain, identified by external standards (ISWC, ISRC, IPI, ISNI…) — no
Artist object in V1, GDPR compliance through minimization.

### In V1 scope

- 1 MIDDS type implemented end-to-end: **MusicalWork** (minimal field draft,
  the goal is the architecture, not the final business model)
- **Generic multi-instance** FRAME pallet: `pallet_midds<T, I>`, one `Instance`
  per MIDDS type in the runtime
- Full Rust SDK: types, validation, runtime API, RPC, subxt client, codegen, CLI
- Mechanics: permissionless deposit with bond, update under a freeze window,
  `force_*` via sudo
- Polkadot SDK target: **stable2503**, Rust edition **2024** (aligned with
  the `../Allfeat` runtime)

### Out of V1 scope

- `pallet-midds-party` (contributor registry — separate spec, separate pallet
  planned but not implemented here)
- Off-chain extension pinning model (who pins, how, incentives)
- Proof of Metadata (community verification — future phase)
- JSON Schema generation (deferred to V2)
- Official TypeScript SDK (the TS client will be derived via subxt → metadata via
  bindings when Recording/Release are added)
- Recording and Release (trivial to add once MusicalWork is stable)

---

## 2. Locked architectural decisions

> **Note**: the economic model (bond, dynamic pricing, finalization
> window, fund destination) is spec'd in detail in
> [`docs/economics.md`](./economics.md). Entries #4, #5, #6, #10 below
> are the structural decisions; see `economics.md` for the complete
> mechanics.

| # | Topic | Decision |
|---|-------|----------|
| 1 | Toolchain | Polkadot SDK stable2503, Rust edition 2024 |
| 2 | Pallet pattern | Generic multi-instance (`pallet_midds<T, I>`) |
| 3 | Versioning | Top-level enum: `enum MusicalWork { V1(MusicalWorkV1) }`, explicit migrations via `OnRuntimeUpgrade` |
| 4 | Bond | `fungible::MutateHold` + `HoldReason` per instance, **7d refundable window then transfer to Foundation Treasury** (cf. `economics.md`) |
| 5 | Bond formula | `(DepositBase + DepositPerByte × encoded_size) × M_fast × M_slow` — dynamic anti-DoS and anti-flood multipliers (cf. `economics.md` §5) |
| 6 | 7d window | `BlockNumberFor<T>`, **dual role**: update (depositor) + refund via `remove_own` (cf. `economics.md` §4) |
| 7 | `ForceOrigin` | `EnsureRoot` (sudo kill-switch). **Two variants**: `force_remove_refund` (typo) and `force_remove_slash` (abuse) |
| 8 | On-chain validation | Format/mask only (charset, length, structure) — **no** checksum |
| 9 | Rich validation | On the `midds-validate` (std) side, checksums *warning only* |
| 10 | Identifier index | **Multi-claim**: `IdentifierClaims: DoubleMap<Identifier, MiddsId, ()>`. Duplicate identifiers allowed; exact-duplicate prevention via `PayloadHashes: Map<H256, MiddsId>` |
| 11 | ID counter | `NextMiddsId` per instance |
| 12 | Tests | Standard FRAME mock runtime in `pallet-midds/src/mock.rs` (no node, no runtime-example) |
| 13 | Rust client | subxt only |
| 14 | Contributors | External codes only (IPI/ISNI), no reference to the Party registry |

---

## 3. Workspace layout

```
midds-sdk/
├── Cargo.toml                     # workspace, [workspace.dependencies] aligned with stable2503
├── rust-toolchain.toml            # already present
├── flake.nix                      # already present
├── rustfmt.toml
├── clippy.toml
├── docs/
│   └── plan.md                    # this document
├── crates/
│   ├── midds-traits/              # no_std — trait Midds, typed identifiers, errors
│   ├── midds-types/               # no_std — MusicalWork enum + V1 + impl Midds
│   ├── midds-validate/            # std    — regex, normalization, checksums (warn)
│   ├── midds-runtime-api/         # no_std — sp_api::decl_runtime_apis
│   ├── midds-rpc/                 # std    — jsonrpsee handlers
│   ├── midds-client/              # std    — subxt wrapper
│   ├── midds-codegen/             # std    — bindings from metadata
│   └── midds-cli/                 # std    — bin (deposit, query, bulk, validate)
└── pallets/
    └── pallet-midds/              # generic multi-instance + mock + tests + benchmarks
```

`pallets/pallet-midds-party/` will arrive in a separate PR once its spec is
available.

---

## 4. Architectural pivot: trait `Midds`

The whole ecosystem (pallet, runtime API, validate, codegen) is generic over
this trait. Adding a new MIDDS type = define a struct + impl `Midds` +
a new Instance in the runtime. Zero duplication.

```rust
// crates/midds-traits/src/lib.rs
#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::Parameter;
use parity_scale_codec::MaxEncodedLen;

pub mod identifier;
pub mod error;

pub use error::MiddsFormatError;
pub use identifier::*;

/// MIDDS unique on-chain id (per-instance counter).
pub type MiddsId = u64;

pub trait Midds: Parameter + MaxEncodedLen {
    /// Canonical industry identifier (ISWC for MusicalWork, ISRC for Recording…).
    type Identifier: Parameter + MaxEncodedLen + Ord;

    /// Extract the canonical identifier (used for the reverse index).
    fn identifier(&self) -> Self::Identifier;

    /// Format/mask validation only — charset, length, structure.
    /// Does NOT verify checksums (real-world identifiers are noisy and
    /// blocking on checksums would be too restrictive on-chain).
    fn validate_format(&self) -> Result<(), MiddsFormatError>;
}
```

### Typed identifiers

```rust
// crates/midds-traits/src/identifier.rs
use frame_support::{BoundedVec, traits::ConstU32};

pub type MiddsString<const N: u32> = BoundedVec<u8, ConstU32<N>>;

pub type Iswc = MiddsString<11>;  // T + 9 digits + 1 check char
pub type Isni = MiddsString<16>;  // 16 digits
pub type Ipi  = MiddsString<11>;  // up to 11 digits
pub type Isrc = MiddsString<12>;  // CC + 3 alpha + 2 year + 5 num

// Pure functions (no_std-safe), shared with midds-validate:
pub fn validate_iswc_format(b: &[u8]) -> Result<(), MiddsFormatError>;
pub fn validate_isni_format(b: &[u8]) -> Result<(), MiddsFormatError>;
pub fn validate_ipi_format(b: &[u8])  -> Result<(), MiddsFormatError>;
pub fn validate_isrc_format(b: &[u8]) -> Result<(), MiddsFormatError>;
```

### Errors

```rust
// crates/midds-traits/src/error.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
pub enum MiddsFormatError {
    InvalidIdentifierStructure,
    InvalidCharset,
    OutOfBounds,
    EmptyMandatoryField,
    DateInconsistency,
}
```

---

## 5. Per-crate specifications

### 5.1 `midds-traits` (no_std)

**Role**: interface between the types and the pallet. Depends on nothing else
(apart from frame-support, scale).

**Dependencies**: `parity-scale-codec`, `scale-info`, `frame-support`.

**Content**: trait `Midds`, identifier type aliases, pure
`validate_*_format` functions, `MiddsFormatError`.

**Unit tests**: pass/fail matrix on each `validate_*_format`.

### 5.2 `midds-types` (no_std)

**Role**: canonical MIDDS types. Source of truth on the Rust side.

**Dependencies**: `midds-traits`, `parity-scale-codec`, `scale-info`,
`frame-support`.

**V1 content**:
- `musical_work/v1.rs` — `MusicalWorkV1` ultra-light (4 fields)
- `musical_work/mod.rs` — `enum MusicalWork { V1(MusicalWorkV1) }` + impl `Midds`

**Minimal `MusicalWorkV1` draft**:

```rust
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
         Clone, PartialEq, Eq, Debug)]
pub struct MusicalWorkV1 {
    pub iswc: Iswc,
    pub title: MiddsString<256>,
    pub creators: BoundedVec<Ipi, ConstU32<32>>,
}

impl MusicalWorkV1 {
    pub fn validate_format(&self) -> Result<(), MiddsFormatError> {
        validate_iswc_format(&self.iswc)?;
        if self.title.is_empty() {
            return Err(MiddsFormatError::EmptyMandatoryField);
        }
        for ipi in &self.creators {
            validate_ipi_format(ipi)?;
        }
        Ok(())
    }
}
```

```rust
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
         Clone, PartialEq, Eq, Debug)]
pub enum MusicalWork {
    V1(MusicalWorkV1),
}

impl Midds for MusicalWork {
    type Identifier = Iswc;

    fn identifier(&self) -> Iswc {
        match self { Self::V1(v) => v.iswc.clone() }
    }

    fn validate_format(&self) -> Result<(), MiddsFormatError> {
        match self { Self::V1(v) => v.validate_format() }
    }
}
```

**Note**: the actual list of fields (BPM, key, language, work_type,
classical_info, etc.) will be added in a later PR once the
architecture is validated. The enum structure allows evolution without breaking
storage: `V2(MusicalWorkV2)` can be added later with a migration.

**Tests**: encode/decode round-trip, format pass/fail, consistent `MaxEncodedLen`
bound.

### 5.3 `pallet-midds` (no_std, the core)

> ⚠️ **This subsection describes the initial V1 draft (4 extrinsics, simple
> `IdentifierIndex` storage, static bond) and is SUPERSEDED by
> [`docs/economics.md`](./economics.md) AND by the delivered implementation.** The
> actual pallet has 13 call indices, a stratified sponsor/owner bond with
> dynamic multipliers, a 7d window + finalization queue, `_on_behalf`
> meta-transactions, multi-claim (`IdentifierClaims` +
> `PayloadHashes`) and `force_remove_refund/slash/many`. For the
> exact surface, see `CLAUDE.md` ("Pallet mechanics") and the code
> (`pallets/pallet-midds/src/{lib,impls,multipliers,types}.rs`). The text
> below is kept for design history only.

**Role**: generic multi-instance FRAME pallet managing the lifecycle of a
MIDDS type.

**Dependencies**: `midds-traits`, `frame-support`, `frame-system`,
`parity-scale-codec`, `scale-info`, `frame-benchmarking` (gated).

**Config**:

```rust
#[pallet::config]
pub trait Config<I: 'static = ()>: frame_system::Config {
    type RuntimeEvent: From<Event<Self, I>>
        + IsType<<Self as frame_system::Config>::RuntimeEvent>;

    type Currency: MutateHold<Self::AccountId, Reason = Self::RuntimeHoldReason>;
    type RuntimeHoldReason: From<HoldReason<I>>;

    /// The MIDDS payload type for this instance.
    type Midds: Midds;

    /// Origin allowed to deposit/update.
    type ProviderOrigin: EnsureOrigin<Self::RuntimeOrigin, Success = Self::AccountId>;

    /// Origin for force_edit/force_remove (EnsureRoot at launch).
    type ForceOrigin: EnsureOrigin<Self::RuntimeOrigin>;

    #[pallet::constant] type DepositBase: Get<BalanceOf<Self, I>>;
    #[pallet::constant] type DepositPerByte: Get<BalanceOf<Self, I>>;
    #[pallet::constant] type UpdateWindow: Get<BlockNumberFor<Self>>;

    type WeightInfo: WeightInfo;

    #[cfg(feature = "runtime-benchmarks")]
    type BenchmarkHelper: BenchmarkHelper<Self::Midds>;
}

#[pallet::composite_enum]
pub enum HoldReason<I: 'static = ()> {
    Deposit,
}
```

**Storage**:

```rust
#[pallet::storage]
pub type NextMiddsId<T, I = ()> = StorageValue<_, MiddsId, ValueQuery>;

#[pallet::storage]
pub type Items<T: Config<I>, I: 'static = ()> =
    StorageMap<_, Blake2_128Concat, MiddsId, T::Midds>;

#[pallet::storage]
pub type IdentifierIndex<T: Config<I>, I: 'static = ()> = StorageMap<
    _, Blake2_128Concat,
    <T::Midds as Midds>::Identifier, MiddsId,
>;

#[pallet::storage]
pub type DepositInfo<T: Config<I>, I: 'static = ()> =
    StorageMap<_, Blake2_128Concat, MiddsId, DepositOf<T, I>>;
```

**`Deposit` type**:

```rust
#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
pub struct Deposit<AccountId, Balance, BlockNumber> {
    pub depositor: AccountId,
    pub deposited_at: BlockNumber,
    pub amount: Balance,
}

pub type DepositOf<T, I> = Deposit<
    <T as frame_system::Config>::AccountId,
    BalanceOf<T, I>,
    BlockNumberFor<T>,
>;
```

**Extrinsics** (4):

| Extrinsic | Origin | Conditions | Effects |
|-----------|--------|------------|--------|
| `deposit(item)` | `ProviderOrigin` | valid format ; identifier not yet present | hold bond, insert item + index + deposit info, increment NextId, emit `Deposited` |
| `update(id, item)` | `ProviderOrigin` | caller == depositor ; `now - deposited_at <= UpdateWindow` ; identifier unchanged ; valid format | adjust hold (delta), replace item, emit `Updated` |
| `force_edit(id, item)` | `ForceOrigin` | item exists ; valid format ; identifier unchanged | bypass freeze, adjust hold for the depositor, replace item, emit `ForceEdited` |
| `force_remove(id)` | `ForceOrigin` | item exists | release all, remove item + index + deposit info, emit `ForceRemoved` |

Shared logic extracted into private helpers:
- `do_validate_format(item) -> DispatchResult`
- `do_compute_bond(size) -> BalanceOf`: `DepositBase + DepositPerByte * size`
- `do_adjust_hold(account, old, new) -> DispatchResult`: compare and hold or release the delta

**Events**:

```rust
#[pallet::event]
pub enum Event<T: Config<I>, I: 'static = ()> {
    Deposited { id: MiddsId, depositor: T::AccountId, bond: BalanceOf<T, I> },
    Updated { id: MiddsId, new_bond: BalanceOf<T, I> },
    ForceEdited { id: MiddsId },
    ForceRemoved { id: MiddsId },
}
```

**Errors**:

```rust
#[pallet::error]
pub enum Error<T, I = ()> {
    IdentifierAlreadyExists,
    MiddsNotFound,
    NotProvider,
    UpdateWindowClosed,
    IdentifierImmutable,
    InvalidFormat,
    BondHoldFailed,
    BondReleaseFailed,
}
```

**Mock runtime** (`pallet-midds/src/mock.rs`): `frame_system` +
`pallet_balances` + `pallet_midds::<Instance1>` instantiated on a trivial
`MockMidds` implementing `Midds`, just enough to test the
mechanics (identifier + payload).

**Tests** (`tests.rs`) — coverage matrix:

- `deposit_works` (happy path: storage filled, hold taken, event emitted)
- `deposit_rejects_duplicate_identifier`
- `deposit_rejects_invalid_format`
- `deposit_holds_correct_bond` (DepositBase + DepositPerByte * size)
- `update_within_window_works`
- `update_after_window_freezes`
- `update_only_by_depositor`
- `update_keeps_identifier_immutable`
- `update_adjusts_bond_up` (size increased → additional hold)
- `update_adjusts_bond_down` (size decreased → partial release)
- `force_edit_bypasses_freeze`
- `force_edit_requires_root`
- `force_remove_releases_bond_and_clears_index`
- `force_remove_requires_root`

**Benchmarks** (`benchmarking.rs`): parametrized on size via
`BenchmarkHelper::bench_instance(s: u32) -> T::Midds`:

- `deposit(s)` worst-case
- `update(s)`
- `force_edit(s)`
- `force_remove`

`weights.rs` generated via `frame-benchmarking-cli`.

### 5.4 `midds-validate` (std)

**Role**: rich validation for the dev/SDK tools. **Never** runs
on-chain.

**Dependencies**: `midds-traits`, `midds-types`, `regex`, `thiserror`.

**Content**:

- Tolerant regexes: ISWC `^T-?\d{3}\.?\d{3}\.?\d{3}-?\d$`, likewise ISNI/IPI/ISRC
- `parse_iswc(s: &str) -> Result<Iswc, ParseError>`: strip separators,
  uppercase, normalization
- `verify_iswc_checksum(&Iswc) -> CheckResult { Pass, Fail, NotApplicable }`:
  *warning only*, never used by the pallet (noisy real-world data)
- Likewise `verify_isni_checksum` (mod 11), `verify_ipi_checksum` (mod 10)
- ✅ Ergonomic `MusicalWorkBuilder`: `.iswc()`, `.title()`,
  `.add_creator()`, `.build() -> Result<MusicalWork, BuildError>` which aggregates
  errors (delivered in sprint C.3 of [`v1-hardening.md`](./v1-hardening.md))

Reuses the `validate_*_format` functions from `midds-traits` for zero duplication.

### 5.5 `midds-runtime-api` (no_std)

**Role**: generic runtime API for lookups by identifier and access to the
deposit info.

```rust
sp_api::decl_runtime_apis! {
    pub trait MiddsApi<Identifier, Item, AccountId, Balance>
    where
        Identifier: Codec,
        Item: Codec,
        AccountId: Codec,
        Balance: Codec,
    {
        fn lookup_by_identifier(id: Identifier) -> Vec<MiddsId>;
        fn get(id: MiddsId) -> Option<Item>;
        fn deposit_info(id: MiddsId) -> Option<DepositInfoOf<AccountId, Balance>>;
    }
}
```

The runtime does `impl_runtime_apis!` once per instance (3 implementations
eventually: MusicalWorks, Recordings, Releases).

### 5.6 `midds-rpc` (std)

**Role**: JSON-RPC exposure of the runtime APIs via `jsonrpsee`.

Generic handler `MiddsRpc<C, B, Identifier, Item, AccountId, Balance>` →
a single piece of code, instantiated N times on the node side. Namespace per instance:
`midds_musicalWork_lookupByIswc`, `midds_recording_lookupByIsrc`, etc.

**V1 implementation note**: the `#[rpc(server)]` macro of `jsonrpsee` freezes the
name of the emitted methods. With a single instance (V1 = MusicalWorks), the
methods are published under `midds_*` directly. When the node hosts
several instances, it will have to manually rename each method to
`midds_<instance>_*` before merging the modules to avoid collisions.
Since multi-instance is not exercised in V1, the renaming helper is
deliberately left to the integration node.

### 5.7 `midds-client` (std)

**Role**: ergonomic wrapper on top of subxt.

**Dependencies**: `subxt`, `subxt-signer`, `midds-types`, `midds-validate`
(for client-side validation before submit).

**V1 structure**:

- `src/lib.rs`: typed façade per instance (`MusicalWorksApi`, …)
- `src/codec_bridge.rs`: bridge `parity_scale_codec::Encode` →
  `subxt::scale_encode::EncodeAsFields` allowing native MIDDS types
  (`BoundedVec`, etc.) to be passed to subxt's dynamic tx API without
  type duplication
- `src/musical_works.rs`: tx + runtime-api calls for the MusicalWorks instance
- `src/error.rs`: `Error` aggregating subxt + MIDDS format + decode

**Choice: dynamic tx & runtime-api (`subxt::dynamic`)** rather than generated
static bindings. Advantages: no circular dependency at
bootstrap time (no need for the metadata of an already-running runtime to build the
client), no versioned `src/generated.rs` that drifts on each runtime update,
and the native MIDDS types (`midds-types`) remain the SoT on the Rust side.
Cost: no static verification of pallet/extrinsic names — mitigated
by the configurable `PALLET_NAME` / `RUNTIME_API_NAME` constants.

`midds-codegen` is still provided for external consumers who want typed
bindings (TypeScript via `polkadot-api`, other languages, custom runtimes),
but `midds-client` itself does not use them.

API style:
```rust
let client = MiddsClient::connect("ws://localhost:9944").await?;
let id = client.musical_works().deposit(&signer, work).await?;
let found = client.musical_works().lookup_by_iswc(iswc).await?;
```

### 5.8 `midds-codegen` (std)

**Role**: generation of Rust bindings from the Substrate metadata.

Binary wrapping `subxt-cli`:
```
cargo run -p midds-codegen -- \
    --metadata <ws-url|path> \
    --out crates/midds-client/src/generated/
```

Generates a static Rust module. Optional eventually: a feature gate to generate
TS bindings (via a `subxt → metadata-portal → polkadot-api` chain),
but out of V1.

### 5.9 `midds-cli` (std, binary)

**Role**: debugging tool, mass deposits, operations.

**Commands**:

- `midds deposit musical-work <json-file>`
- `midds update <id> <json-file>`
- `midds query <iswc>`
- `midds bulk-deposit <jsonl-file>`: mass deposits from a JSON Lines file
- `midds force-remove <id>` (sudo signer)
- `midds validate <iswc>`: uses `midds-validate`, displays checksum as a warning

---

## 6. Phase / PR plan

> **V1 hardening**: see [`v1-hardening.md`](./v1-hardening.md) for the
> A–D sprints that harden the SDK before adding Recording / Release
> (enriched Midds trait, `DepositInfoOf` struct, strictly-less-than 7d
> window, property tests of the invariants, generify fixtures/CLI/bench,
> nightly CI + Justfile + hardened clippy.toml).


| PR | Content | Estimated effort | Blocked by |
|----|---------|---------------|------------|
| 0  | Bootstrap workspace (Cargo.toml, rustfmt, clippy, empty crates) | 1h | — |
| 1  | `midds-traits` (trait Midds, identifiers, format validation) | 2-3h | 0 |
| 2  | `midds-types` (MusicalWork enum + minimal V1) | 2-3h | 1 |
| 3a | `pallet-midds` core (config, storage, extrinsics, mock, tests) | 1-2d | 1, 2 |
| 3b | `pallet-midds` benchmarks + weights | 4-6h | 3a |
| 4  | `midds-validate` (regex, builder, checksums warn) | 4h | 1, 2 |
| 5  | `midds-runtime-api` + `midds-rpc` | 4-6h | 3a |
| 6a | `midds-codegen` | 3h | 5 (needs metadata) |
| 6b | `midds-client` (subxt façade) | 6h | 6a |
| 6c | `midds-cli` | 4h | 6b, 4 |

**Critical path**: 0 → 1 → 2 → 3a (the generic architecture in place).
The other PRs then follow on, some in parallel (3b and 4 can
be done in parallel with 5).

---

## 7. Security and invariants

### On-chain invariants to preserve

- One identifier ↔ at most one MiddsId (global uniqueness per instance)
- `DepositInfo` exists iff `Items` exists (strict coupling)
- The hold on the bond is always = `DepositInfo.amount`
- The identifier can never change after deposit (immutability)
- `NextMiddsId` is strictly monotonically increasing

### Invariant tests

The tests of PR 3a must explicitly verify each invariant after
each extrinsic.

### Attack vectors considered

- **Bond bypass**: does `update` allow reducing the bond and fleeing with it?
  → No, the release is strictly proportional to the size decrease
- **Identifier squatting**: does `force_edit` allow reassigning an
  identifier to another item? → No, identifier immutable including for
  `force_edit`
- **DOS via huge MIDDS**: bounded by the `MaxEncodedLen` of the types + bond
  proportional
- **Deletion without release**: `force_remove` must *always* release the hold

---

## 8. Conventions

- Edition 2024, `#![cfg_attr(not(feature = "std"), no_std)]` everywhere except bin
- Licensing: GPL-3.0
- All types stored on-chain: `MaxEncodedLen` mandatory,
  `BoundedVec` everywhere, never `String` or free `Vec`
- All identifiers: ASCII only, on-chain charset validation
- Errors: dedicated enum per crate, no `String` in on-chain errors

---

## 9. Open items / to be decided later

- Actual fields of `MusicalWorkV1` (BPM, key, language, classical_info,
  duration, etc.) — V1 delivered minimal, later iteration
- Off-chain extension pinning model (depositor? Crust/Filecoin-style
  incentives? dedicated Allfeat nodes?)
- Complete spec of `pallet-midds-party`
- GDPR legal framing to be validated with a lawyer before public communication

---

## 10. Getting started

Once this plan is validated: start with PR 0 (bootstrap workspace).
Each subsequent PR is validated by the user before the next one. Splitting
into small, focused PRs allows iterating on the architecture without
accumulating debt.
