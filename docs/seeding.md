# MIDDS SDK — seeding & snapshot workflow

> Companion to `docs/testing.md` (Layer 5 / step 8). Documents the
> end-to-end recipe for producing a deterministically-seeded Allfeat dev
> chain that the frontend (or any downstream consumer) can boot against.

---

## 1. What this gives you

A dev chain whose `MusicalWorks` instance contains `N` plausibly-shaped
records, byte-for-byte reproducible across machines from a single
`(rng_seed, count)` pair. Useful for:

- frontend development without manual record entry,
- exploratory benchmarking and load testing,
- regression demos pinned to a known state.

The whole pipeline runs locally with `midds-cli` against a `--dev` node;
no GitHub artefacts or remote chain involved.

---

## 2. Prerequisites

- An `allfeat` binary built from the sibling `Allfeat` repo with the
  `melodie` runtime enabled (the `MusicalWorks` `Instance1` only lives in
  `melodie-runtime`).
- The `midds` binary from this repo: `cargo build -p midds-cli --release`.
- A funded `//Alice` account on the dev chain (default `--chain
  melodie-dev`). All commands below assume this.

---

## 3. Reproducibility contract

Two inputs control bit-for-bit replay:

1. `--rng-seed` — 64-bit ChaCha20 seed driving payload generation
   (`midds-fixtures::gen_n`). Defaults are fixed, so unattended runs stay
   reproducible.
2. `--count` — number of records.

Same `(seed, count)` ⇒ same `Vec<MusicalWork>` SCALE-encoded byte-for-byte,
and therefore same on-chain state after `seed`.

Account derivation is independent: `--base-signer //Alice` plus
`--signer-count N` always yields the deterministic set
`//Alice//1`, …, `//Alice//N` via sr25519 hard junctions.

---

## 4. Workflow

### 4.1 Boot a clean dev node

```bash
allfeat --chain melodie-dev --base-path /tmp/midds-seed --tmp \
        --rpc-port 9944
```

`--tmp` keeps the chain DB in a temp dir so each run starts fresh; drop
it once you're snapshotting a state you want to keep.

### 4.2 Pre-fund the signers

`seed` partitions deposits round-robin across `--signer-count` signers.
Each one needs enough free balance to cover the bond
(`DepositBase + DepositPerByte * encoded_size`). Fund them in one shot:

```bash
midds admin pre-fund-signers \
    --count 100 \
    --amount 1000000000000000   # 1000 AFT in plancks
```

Defaults: `--funder //Alice`, `--base-signer //Alice` →
funds `//Alice//1` … `//Alice//100`.

### 4.3 Seed the chain

```bash
midds seed \
    --count 50000 \
    --signer-count 100 \
    --concurrency 16 \
    --rng-seed 0xDEADBEEF \
    --report seed.json
```

Output is a JSON report capturing the inputs that uniquely determine the
resulting state, including the post-run `NextMiddsId`.

### 4.4 Verify

```bash
midds verify-state \
    --expected-count 50000 \
    --sample 200 \
    --rng-seed 0xDEADBEEF
```

`--sample N` picks `N` random ids and confirms they are queryable through
the runtime API. Increase for stronger spot-checks; the cost is one RPC
round-trip per sample.

### 4.5 Export the chain state

The dev node exposes the standard Substrate state-export RPC:

```bash
allfeat export-state --chain melodie-dev /tmp/midds-seed > seeded-state.json
```

This yields a chain-spec-shaped JSON containing the entire post-seed
storage. Boot from it with:

```bash
allfeat --chain ./seeded-state.json --tmp --rpc-port 9944
```

Any node — frontend dev, CI integration runner, demo machine — will then
start with the same seeded MIDDS records.

---

## 5. Distribution options

Choose based on snapshot size:

| Size | Recommended channel |
|---|---|
| < 10 MB | commit to a fixtures repo |
| 10 MB – 100 MB | release asset attached to a SDK tag |
| > 100 MB | release asset + LFS, or regenerate on-demand |

Regeneration is always free given the reproducibility contract, so
"regenerate on demand" is a viable fallback for very large snapshots —
ship the seed parameters instead of the raw JSON.

---

## 6. Related operator commands

Once a chain is seeded, two adjacent commands round out Layer 5 of
`docs/testing.md`:

- **`midds bench fees`** — measures real bond + tx fee per `deposit`,
  bucketed by payload size, written as a markdown report. Use to inform
  `DepositBase` / `DepositPerByte` tuning decisions.
- **`midds bench throughput`** — sustained submission rate against a live
  node, with finalisation-latency percentiles. Use to characterise node
  capacity under realistic deposit load.

Both consume the same `midds-fixtures` payload generator and signer
derivation as `seed`, so their outputs are reproducible under the same
`(rng_seed, count)` contract.

---

## 7. Open items

- Fixed snapshot rotation policy (how many to keep / publish per release).
- Storage-root verification: `verify-state` only checks `NextMiddsId` plus
  a sample. Adding a `runtime_api` exposing the per-instance storage root
  would make full anti-regression on snapshots possible.
- An automated CI workflow producing a snapshot per SDK tag — referenced
  in `docs/testing.md` §13 step 8 but not yet wired up.
