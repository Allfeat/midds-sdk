# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Reference docs

- `docs/plan.md` — architectural plan. Section 2 contains **locked decisions** (toolchain, pattern, versioning, bond, validation, etc.). Sections 5.x are the per-crate specs. Read it before proposing structural changes.
- `docs/economics.md` — economic model spec : bond + 7d refundable window + Foundation Treasury, dual dynamic multipliers (`M_fast` anti-DoS, `M_slow` anti-flood), multi-claim identifier index, exact-payload uniqueness via hash, two-variant `force_remove`. Read this before touching anything bond/fee/storage-uniqueness related.
- `docs/testing.md` — 5-layer test strategy and the planned `midds-fixtures` crate.
- `docs/validation.md` — **canonical, frozen per-field validation spec** for every MIDDS type (lengths, numeric ranges, cardinality, charset, enum membership) and where each rule is enforced (type vs `validate_format` vs `midds-validate`). Read this before changing any field bound, `validate_format`, or a fixture strategy. Section 7 lists the deliberate V1 asymmetries (e.g. `Release.release_date.year` unconstrained, `Recording.duration` uncapped).
- `flake.nix` provides a Nix dev shell (`direnv` autoloads it via `.envrc`). Without Nix, install the toolchain pinned in `rust-toolchain.toml`.

## Build & test commands

The workspace is a single `Cargo.toml` at the root. The pallet is `no_std`-by-default; CLI/client/RPC crates are `std`-only.

```bash
# Build everything (std)
cargo build

# Build the pallet for the runtime (no_std + wasm32)
cargo build -p pallet-midds --no-default-features --target wasm32-unknown-unknown

# Run all tests
cargo test

# Test a single crate / one test
cargo test -p pallet-midds
cargo test -p pallet-midds deposit_works
cargo test -p midds-traits identifier::tests::iswc_pass

# Pallet test suite that exercises every extrinsic via the mock runtime
cargo test -p pallet-midds --lib

# Run benchmarks (compiles the pallet with the runtime-benchmarks feature)
cargo test -p pallet-midds --features runtime-benchmarks
# Note: weight regeneration uses frame-omni-bencher and runs against melodie-runtime
# in the sibling Allfeat repo, not here.

# Lint (msrv set to 1.85 in clippy.toml)
cargo clippy --all-targets --all-features

# Format
cargo fmt --all

# CLI binary
cargo run -p midds-cli -- <args>          # bin name is `midds`
cargo run -p midds-cli -- validate T1234567890 --type iswc

# Codegen tool (subxt bindings from a running node's metadata)
cargo run -p midds-codegen -- --metadata ws://localhost:9944
```

## High-level architecture

### The pivot: `trait Midds`

Everything in this SDK is generic over `midds_traits::Midds` (`crates/midds-traits/src/lib.rs`). A new MIDDS type is a struct + an `impl Midds` + a new pallet `Instance` in the runtime. Zero duplication across the pallet, the runtime API, the validator, and the codegen.

```rust
pub trait Midds: Parameter + MaxEncodedLen {
    type Identifier: Parameter + MaxEncodedLen + Ord;  // ISWC, ISRC, ...
    fn identifier(&self) -> Self::Identifier;
    fn validate_format(&self) -> Result<(), MiddsFormatError>;
}
```

### Crate roles

| Crate | std/no_std | Role |
|---|---|---|
| `midds-traits` | no_std | `trait Midds`, identifier byte-string aliases (`Iswc`, `Isni`, `Ipi`, `Isrc`, `Upc`, `OffchainHash`), pure `validate_*_format` functions, `MiddsFormatError`. |
| `midds-types` | no_std | Canonical MIDDS payloads. V1 ships all three types — `MusicalWork`, `Recording`, `Release` — each a top-level versioned `enum X { V1(XV1) }`. Cross-type pieces (`Title`, `PartyId`, `WorkRef`, `RecordingRef`, `MusicalKey`) live in `shared`; `Country` (ISO 3166-1) and `Language` (ISO 639-1) are closed tag-byte enums. |
| `pallet-midds` | no_std | The generic multi-instance FRAME pallet. 4 extrinsics (`deposit`, `update`, `force_edit`, `force_remove`), bond via `fungible::MutateHold`, mock runtime in `src/mock.rs`. |
| `midds-runtime-api` | no_std | `decl_runtime_apis!` for `lookup_by_identifier` / `get` / `deposit_info`. Implemented once per instance in the runtime. |
| `midds-rpc` | std | Generic JSON-RPC handler bridging the runtime API to clients. |
| `midds-validate` | std | Tolerant parsers (`parse_iswc`, …), warning-only checksum verifiers, `MusicalWorkBuilder`. **Never** runs on-chain. |
| `midds-client` | std | Subxt façade. Uses `subxt::dynamic` (not generated bindings) — see "Client choices" below. |
| `midds-codegen` | std (bin) | CLI wrapping `subxt-codegen` for **external consumers** (TS via polkadot-api, etc.). `midds-client` itself does not consume its output. |
| `midds-cli` | std (bin: `midds`) | Operator CLI: deposit, update, query, bulk-deposit (JSONL), validate offline. |

### Pallet (`pallet-midds`) mechanics

- **Multi-instance**: parametrised on `<T, I>` and instantiated per-MIDDS-type in the runtime. `HoldReason<I>` is also instance-scoped.
- **Storage**: `Items<MiddsId, T::Midds>`, `IdentifierIndex<Identifier, MiddsId>` (reverse lookup, backs uniqueness), `DepositInfo<MiddsId, Deposit>` (depositor + deposited_at + held bond), `NextMiddsId` (monotonic per-instance counter).
- **Bond formula**: `DepositBase + DepositPerByte * encoded_size`. Stored in `Deposit::amount` so removal releases the exact original amount even if the formula has since changed.
- **Freeze window**: `update` only works while `now - deposited_at <= UpdateWindow` and only by the original depositor. `force_edit` (sudo) bypasses the freeze.
- **Identifier immutability**: neither `update` nor `force_edit` can change the canonical identifier.
- **Validation on-chain is format-only** (charset, length, structure) — explicitly **not** checksum verification. Real-world registries publish records with bad check digits; checksums are warning-only and live in `midds-validate`.

### Versioning strategy

Top-level enums (`enum MusicalWork { V1(...) }`). Adding a `V2` is additive; migrations are explicit `OnRuntimeUpgrade` impls. Storage layout never breaks.

### serde gating

`midds-traits` and `midds-types` expose a `serde` feature (off by default). Activated only by `std` consumers (the node, RPC, CLI) — never by the runtime, which stays `no_std`. Required because `jsonrpsee` needs `Serialize`/`Deserialize` on RPC output types.

### Client choices

`midds-client` uses `subxt::dynamic::tx` and `runtime_apis().call_raw` — **not** statically generated bindings. Reasons: avoids the bootstrap cycle of needing a running runtime to build the client, and avoids a `src/generated.rs` that drifts every runtime upgrade. The cost (no static check on pallet/extrinsic names) is mitigated by `PALLET_NAME` / `RUNTIME_API_NAME` constants and by `codec_bridge::EncodedCall`, which feeds `parity_scale_codec::Encode` types into subxt's `EncodeAsFields`-based dynamic API without duplicating type definitions. `midds-codegen` is shipped for external consumers, not for `midds-client` itself.

If you touch `midds-client`, keep the dynamic approach.

### Integration with Allfeat (sibling repo)

The `../Allfeat` runtime consumes this SDK via path dependencies. `pallet_midds<Instance1>` is wired for `MusicalWork` in `melodie-runtime` at pallet index 106. The mainnet runtime (`allfeat-runtime`) does **not** host the pallet — the node has two service entry points and a `dispatch_on_runtime_full!` macro picking the right one per chain spec. When extending the SDK with new MIDDS types, the runtime side is adding an `Instance`, the runtime-API impl, a service-side `MiddsRuntimeApiCollection` bound, and an RPC namespace.

### Testing layers

Per `docs/testing.md`, the planned strategy is 5 layers:

1. **Unit pallet** (`pallets/pallet-midds/src/tests.rs`) — exists, mock FRAME.
2. **Property-based pallet** — proptest on the mock (planned).
3. **Mass injection** — N=10k–100k on the mock with a committed `storage_root_hash` for anti-regression (planned).
4. **Runtime integration / fee reporting** — lives in the sibling `Allfeat` repo (`runtime/melodie/tests/`), not here, to preserve the SDK / runtime decoupling.
5. **E2E node** — extends `midds-cli` with `seed`, `bench`, `verify-state` subcommands (planned).

The cornerstone for all of the above is a planned `midds-fixtures` crate (proptest strategies + JSON corpora). Don't add a new test-tooling crate without checking whether `midds-fixtures` or `midds-cli` should host it.

## Release & versioning

- **Lockstep workspace versioning** — every crate inherits `version.workspace = true`. A bump moves all crates together. Rationale: the `trait Midds` is the pivot, Polkadot SDK upgrades force everyone to bump anyway, and consumers (Allfeat runtime, future explorer) only pin one number.
- **SemVer pré-1.0** — Cargo's pre-1.0 rules apply: `0.x.0` is breaking, `0.x.y` is compatible. Polkadot SDK upgrades (e.g. `stable2503` → `stable2506`) bump the minor and are documented as a compatibility break in the CHANGELOG.
- **Trunk branch is `master`** (not `main`). PRs target `master`.
- **Conventional Commits are mandatory** — `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`, `build:`, `ci:`, `perf:`, with `!` or a `BREAKING CHANGE:` footer for breaking changes. No exceptions: `release-plz` derives versions and CHANGELOG entries from commit messages.
- **crates.io is deferred** — while the API stabilises, consumers use path/git deps. No publish workflow is wired yet.
- **`midds-fixtures` is `publish = false`** — internal test scaffolding; never published even once other crates are.
- **Pre-built binaries (`midds`, `midds-codegen`)** — from v1.0+ only, via `cargo-dist` on GitHub Releases. Before that, install via `cargo install --path …` from a clone.
- **Release tooling — `release-plz`** (`release-plz.toml` + `.github/workflows/release-plz.yml`) — maintains a release PR on `master` that bumps versions and generates per-crate `CHANGELOG.md`. Configured in **git-only mode** (`git_only = true`): no `cargo publish`, versions derived from git tags. Single workspace tag (`vX.Y.Z`) carried by `pallet-midds`; the other publishable crates inherit the version via `version.workspace = true`. `midds-fixtures` is excluded (`release = false`).
- **CI** (`.github/workflows/ci.yml`) — 5 jobs run on push to `master` and on PR: `fmt`, `clippy` (`-D warnings`, `--all-targets --all-features`), `test` (`--workspace --all-features`), `wasm` (`pallet-midds` `no_std` build for `wasm32-unknown-unknown`), and `commitlint` (PR-only, validates Conventional Commits via `.commitlintrc.yaml`).

## Conventions

- Edition **2024** everywhere; Polkadot SDK pinned to **stable2503** (workspace deps in root `Cargo.toml`).
- `#![cfg_attr(not(feature = "std"), no_std)]` at the top of every library crate (binaries are std-only).
- On-chain types **always** use `BoundedVec`/`MiddsString<N>` and derive `MaxEncodedLen`. Never `String` or unbounded `Vec`.
- Identifiers are **ASCII-only**; charset is enforced by the on-chain `validate_*_format` helpers.
- Errors: per-crate enums; on-chain errors never carry `String`.
- License: GPL-3.0.
- The `serde` derive on MIDDS types is gated behind a feature, never unconditional.
- Commits follow **Conventional Commits** (`feat:`, `fix:`, `chore:`, `BREAKING CHANGE:`, …) — required for release automation. See "Release & versioning".
- **Never add a `Co-Authored-By: Claude …` trailer to commit messages.** Drop it from the heredoc commit template — the commit log should read as the maintainer's own work regardless of how much Claude assisted during authoring.
