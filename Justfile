# Reproducible developer entry points for the MIDDS SDK.
# Mirrors the `.github/workflows/ci.yml` jobs so `just check` exercises the
# same suite locally that the per-PR pipeline runs.

default:
    @just --list

# Full pre-PR gate: format, lint, test workspace.
check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

# Quick iteration loop: skips `--all-features` to drop heavy dev-dependencies
# from the build graph. Use during in-flight work; switch to `just check`
# before pushing.
test-fast:
    cargo test --workspace

# Nightly-only suites that exceed the per-PR budget — heavy property tests
# (10k cases) and the 50k / 100k mass-injection scenarios. Mirrors the
# `.github/workflows/nightly.yml` jobs so a developer can repro a nightly
# failure locally.
test-nightly:
    PROPTEST_CASES=10000 cargo test -p pallet-midds property_tests --release
    cargo test -p pallet-midds --release --test mass_injection -- --ignored

# `pallet-midds` `no_std` build for the runtime target. Catches any
# accidentally-`std`-only API leaking into the on-chain code path.
wasm:
    cargo build -p pallet-midds --no-default-features --target wasm32-unknown-unknown

# Refresh the committed mass-injection storage-root fixtures. Run after a
# deliberate change to bond accounting / `MusicalWork` SCALE encoding;
# commit the regenerated `pallets/pallet-midds/tests/fixtures/storage_root_*.txt`.
bless-fixtures:
    BLESS_MASS_INJECTION_FIXTURES=1 cargo test -p pallet-midds --test mass_injection

# Single-crate test runner. Examples:
#   just test-crate pallet-midds
#   just test-crate midds-validate
test-crate crate:
    cargo test -p {{crate}} --all-features
