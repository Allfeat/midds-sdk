# MIDDS SDK — V1 Validation Rules Specification

> **Canonical** reference document for the per-field validation of each MIDDS
> type. The rules below are **frozen for V1**: a change here implies a payload
> version bump (`V2`) or, for bounds encoded in the type, a wire-format /
> `MaxEncodedLen` change.
>
> Origin: business rules designed in the legacy `../midds` frontend
> (Zod schemas + imperative validations), **reconciled** with the current
> SDK types. Reconciliation decision: **the current SDK is authoritative** —
> its tight bounds and V1 model choices (slimmed enums, structured
> `MusicalKey`, `u32` `duration`, `PartyId` without `Both`, no
> `manufacturer_name`) are deliberate and kept. The legacy frontend is the
> source for the **numeric / cardinality rules** (BPM, years, number of
> voices, cardinality + uniqueness + non-self-reference of Medley/Mashup/
> Adaptation source works) that were missing from on-chain validation and are
> added here.

---

## 1. Principles

1. **On-chain = format only.** `Midds::validate_format` checks
   charset / length / structure / numeric bounds / cardinality. It **never
   checks checksums** (ISWC/IPI/ISNI/GTIN check digit): real-world registries
   publish records with wrong check digits. Checksums are *warning-only* and
   live in `midds-validate`, never blocking on-chain.
2. **Max lengths are structural.** Every bounded field is a
   `BoundedVec` / `MiddsString<N>`: exceeding the bound is impossible to
   construct/decode. `validate_format` therefore does not re-test the max
   length; it tests the **non-emptiness** of mandatory fields and the
   **numeric bounds / minimum cardinality**.
3. **Enums are closed.** `Country`, `Language`, `Genre`,
   `RecordingVersion`, `ReleaseType/Format/Packaging/Status`, `CreatorRole`,
   `PitchClass`, `Mode` are tag-byte SCALE enums: membership is guaranteed by
   the type, not by `validate_format`.
4. **Identifiers are ASCII only.** Charset enforced by the
   `validate_*_format` helpers in `midds-traits`.
5. **Errors without `String`.** Diagnostics via `MiddsFormatError`
   (`InvalidIdentifierStructure`, `InvalidCharset`, `OutOfBounds`,
   `EmptyMandatoryField`, `CrossFieldInconsistency` — used for `Release`
   tracklist integrity (recording uniqueness + contiguous numbering) — plus
   the reserved `DateInconsistency`). **No new
   variant** is introduced (adding a variant is a SCALE *breaking* change):
   any numeric-bound or minimum-cardinality violation reuses `OutOfBounds`
   ("length below the minimum or above the maximum").

Table notation: **N** = rule added in this stabilization (absent from
on-chain validation before); **S** = guaranteed structurally by the type
(no code in `validate_format`); **=** = already enforced.

---

## 2. Identifiers (`midds-traits`)

| Id | Type / bound | Structure enforced on-chain | Legacy-frontend origin |
|---|---|---|---|
| `Iswc` | `MiddsString<11>` | 11 bytes: `T` + 10 ASCII digits | `^T\d{9}[0-9A-Z]$` — the SDK is **stricter** (10th position = digit, not alpha); kept |
| `Isni` | `MiddsString<16>` | 15 digits + (digit \| `X`) | `^[0-9]{15}[0-9X]$` — identical |
| `Ipi` | `MiddsString<11>` | 9 to 11 ASCII digits | legacy: `1..=11` digits — the SDK enforces **min 9**; kept |
| `Isrc` | `MiddsString<12>` | 2 upper-alpha + 3 upper-alphanum + 2 digits + 5 digits | `^[A-Z]{2}[A-Z0-9]{3}[0-9]{2}[0-9]{5}$` — identical (no dashes) |
| `Upc` | `MiddsString<13>` | exactly 12 (UPC-A) **or** 13 (EAN-13) digits | legacy: `1..=13` digits — the SDK enforces **exactly 12 or 13**; kept |
| `OffchainHash` | `MiddsString<64>` | non-empty (≥ 1 byte); opaque, CIDv1 by client convention | no legacy equivalent |

The check digit (ISWC/IPI mod-10 CISAC, ISNI ISO 7064, GTIN mod-10) is
**not** verified on-chain. Warning-only verifiers:
`midds-validate::checksum`.

---

## 3. Shared types (`midds-types::shared`)

| Type | Canonical definition | Validation |
|---|---|---|
| `Title` | `MiddsString<256>` (`TITLE_MAX_LEN = 256`) | non-empty when mandatory; length **S** |
| `PartyId` | `enum { Ipi(Ipi) \| Isni(Isni) \| Both { ipi: Ipi, isni: Isni } }` — at least one of the two identifiers | structure of each present identifier (both for `Both`). The `Both` variant was **reintroduced** (cf. §7 — the same party can carry IPI and ISNI simultaneously; a native representation more faithful than two duplicated entries) |
| `MusicalKey` | `{ pitch: PitchClass(21), mode: Mode(2) }` = 42 combinations | membership **S**. `PitchClass` covers the 12 chromatic positions with their sharp and flat spellings (`D♭`/`E♭`/`G♭`/`A♭`/`B♭` alongside `C♯`/`D♯`/`F♯`/`G♯`/`A♯`) plus the four cross-natural enharmonics (`B♯`,`E♯`,`C♭`,`F♭`) appended at tags 17..=20 |
| `WorkRef` | `enum { Midds(u64) \| Iswc(Iswc) }` | if `Iswc` ⇒ ISWC structure |
| `RecordingRef` | `enum { Midds(u64) \| Isrc(Isrc) }` | if `Isrc` ⇒ ISRC structure |
| `Country` | closed ISO 3166-1 alpha-2 enum (complete, uppercase JSON) | membership **S** — superset of the legacy frontend's 249 codes |
| `Language` | closed ISO 639-1 alpha-2 enum (complete, lowercase JSON) | membership **S** — superset of the legacy frontend's 22 languages |

`CreatorRole`: `Author | Composer | Arranger | Adapter | Publisher` (5,
identical to the legacy frontend).

---

## 4. `MusicalWork` V1

| Field | Type | Req. | Canonical bound | On-chain rule | |
|---|---|---|---|---|---|
| `iswc` | `Iswc` | yes | 11 | ISWC structure | = |
| `title` | `Title` | yes | ≤ 256 | non-empty | = |
| `creation_year` | `Option<u16>` | no | — | if `Some` ⇒ **`1..=2999`** | **N** |
| `instrumental` | `bool` | yes | — | none (default `false`) | = |
| `language` | `Option<Language>` | no | — | membership | S |
| `explicit_lyrics` | `bool` | yes | — | none (default `false`) | **N** |
| `bpm` | `Option<u16>` | no | — | if `Some` ⇒ **`20..=300`** | **N** |
| `key` | `Option<MusicalKey>` | no | — | membership | S |
| `work_type` | `WorkType` | yes | — | see below | |
| `samples` | `BoundedVec<WorkRef, 64>` (`SAMPLES_MAX`) | no | ≤ 64 | each ref valid (`WorkRef`); **distinct refs**; **non-self-reference** (ISWC variant ≠ the work's `iswc`) | **N** |
| `creators` | `Creators` | yes | ≤ 32 (`CREATORS_MAX`) | **non-empty**; each entry: see `Creator` below | = |
| `classical_info` | `Option<ClassicalInfo>` | no | — | see below | |
| `offchain_extension` | `Option<OffchainHash>` | no | ≤ 64 | if `Some` ⇒ non-empty | = |

**`WorkType`** (`Original | Medley(refs) | Mashup(refs) | Adaptation(iswc) |
Rearrangement(iswc)`):

| Variant | On-chain rule | |
|---|---|---|
| `Original` | none | = |
| `Medley(refs)` / `Mashup(refs)` | `refs.len() >= 2` (`OutOfBounds` if < 2); each ref: ISWC structure; **refs distinct and ≠ the work's `iswc`** (`CrossFieldInconsistency`); max 32 (`WORK_REFERENCES_MAX`) | **N** (was: non-empty ≥ 1) |
| `Adaptation(iswc)` | exactly 1 ISWC (**S**); ISWC structure; **≠ the work's `iswc`** (`CrossFieldInconsistency`) | **N** (was: structure only) |
| `Rearrangement(iswc)` | identical to `Adaptation`: exactly 1 ISWC (**S**); ISWC structure; **≠ the work's `iswc`** (`CrossFieldInconsistency`). A distinct variant (SCALE tag 4, append-only) to carry the *type* of derivation on the wire | **N** |

**`ClassicalInfo`** (optional block):

| Sub-field | Type | Bound | On-chain rule | |
|---|---|---|---|---|
| `opus` | `Option<MiddsString<32>>` (`OPUS_MAX_LEN = 32`) | ≤ 32 | if `Some` ⇒ non-empty | = |
| `catalog_number` | `Option<MiddsString<32>>` (`CATALOG_NUMBER_MAX_LEN = 32`) | ≤ 32 | if `Some` ⇒ non-empty | = |
| `number_of_voices` | `Option<u16>` | — | if `Some` ⇒ **`>= 1`** | **N** |

**`Creator`**: `{ roles: BoundedBTreeSet<CreatorRole, 5> (CREATOR_ROLES_MAX),
party: PartyId }`. `roles` is a **bounded set** (`BoundedBTreeSet`):
duplicates are impossible to construct, SCALE iterates in canonical order
(`Ord` on the discriminants), and the maximum cardinality is exactly the
number of `CreatorRole` variants (5). On-chain validation: `roles` non-empty
(`EmptyMandatoryField`); `party` valid per `PartyId` (`Ipi`, `Isni`, or
`Both` — each present identifier must pass its own structure).
**Reconciliation note**: the legacy frontend merged roles by `PartyId`; the
initial V1 SDK reversed that choice by flattening the list (several `Creator`
entries for the same `PartyId`) — the stabilized version above returns to
merging, more SCALE-economical and more faithful to the business model.

> *Builder-side only* convention (non-blocking on-chain, to be surfaced as a
> warning in `midds-validate`): `instrumental == true` ⇒ `language` should be
> `None` and `explicit_lyrics` should stay `false` (an instrumental work has
> no lyrics).

---

## 5. `Recording` V1

| Field | Type | Req. | Canonical bound | On-chain rule | |
|---|---|---|---|---|---|
| `isrc` | `Isrc` | yes | 12 | ISRC structure | = |
| `title` | `Title` | yes | ≤ 256 | non-empty | = |
| `title_aliases` | `BoundedVec<Title, 8>` (`TITLE_ALIASES_MAX = 8`) | no | ≤ 8 × 256 | each alias non-empty | = |
| `artist` | `PartyId` | yes | — | id structure | = |
| `featuring` | `BoundedVec<PartyId, 16>` (`FEATURING_MAX = 16`) | no | ≤ 16 | each id: structure (featured artists = same `PartyId` as the main artist, not `PerformerId`) | **N** |
| `work` | `WorkRef` | yes | — | if `Iswc` ⇒ structure | = |
| `genre` | `Option<Genre>` | no | — | membership | S |
| `sub_genre` | `Option<Genre>` | no | — | membership (same taxonomy as `genre`); **`Some` ⇒ `genre` is `Some`** (cross-field) | **N** |
| `record_year` | `Option<u16>` | no | — | if `Some` ⇒ **`1..=2999`** | **N** |
| `version_type` | `Option<RecordingVersion>` | no | — | membership | S |
| `performers` | `BoundedVec<Performer, 64>` (`PERFORMERS_MAX = 64`) | no | ≤ 64 | each `Performer`: `id` (`PerformerId`) structure; `instruments` = `BoundedVec<Instrument, 8>` (`INSTRUMENTS_PER_PERFORMER_MAX = 8`), membership **S**, **≥ 1 required** (a performer credit with no instrument is rejected — id checked first) | **N** |
| `producers` | `BoundedVec<Isni, 8>` (`PRODUCERS_MAX = 8`) | no | ≤ 8 | each: ISNI structure (ISNI-only by design) | = |
| `duration` | `Option<u32>` | no | — | **no cap** (seconds; `u32` ≈ 136 years, deliberate V1 choice) | (cf. §7) |
| `bpm` | `Option<u16>` | no | — | if `Some` ⇒ **`20..=300`** | **N** |
| `key` | `Option<MusicalKey>` | no | — | membership | S |
| `places` | `Option<ProductionPlaces>` | no | — | see below | |
| `contributors` | `BoundedVec<PartyId, 32>` (`CONTRIBUTORS_MAX = 32`) | no | ≤ 32 | each id: structure | = |
| `offchain_extension` | `Option<OffchainHash>` | no | ≤ 64 | if `Some` ⇒ non-empty | = |

**`ProductionPlaces`** (optional block): `recording`, `mixing`, `mastering`
each `Option<MiddsString<128>>` (`PLACE_MAX_LEN = 128`); if `Some` ⇒
non-empty.

Closed enums:

- **`Genre`** (25): `Pop, Rock, HipHop, RnB, Electronic, Dance, Jazz, Blues,
  Classical, Country, Folk, Metal, Punk, Reggae, Latin, World, Soul, Funk,
  Gospel, Soundtrack, Ambient, Experimental, Children, SpokenWord, Other`.
  Deliberately flattened taxonomy (the legacy frontend exposed ≈160
  hierarchical ones) — fine granularity deferred to a future payload version.
- **`RecordingVersion`** (14): `Original, RadioEdit, Extended, Remix, Live,
  Acoustic, Instrumental, ACapella, Karaoke, Demo, ReRecorded, Edited, Cover,
  Clean`. `Clean` (the "clean" / explicit-content-free version, the
  parental-advisory counterpart of an explicit release) is **appended at the
  tail** (SCALE tag 13, append-only); the tags of the previous 13 variants
  are unchanged.
- **`Instrument`** (77): instrument played by a `Performer`. Broad taxonomy
  grouped by family (vocals, keyboards, plucked strings, bowed strings,
  woodwinds, brass, pitched percussion, percussion / drums, electronic) with
  family-level generics (`Vocals`, `Guitar`, `Keyboards`, `Strings`,
  `Percussion`) and a final `Other`. A single SCALE tag-byte like `Genre`;
  new instruments **appended at the tail** in a future payload version, never
  reordered.

---

## 6. `Release` V1

| Field | Type | Req. | Canonical bound | On-chain rule | |
|---|---|---|---|---|---|
| `upc` | `Upc` | yes | 12 or 13 | UPC/EAN structure | = |
| `title` | `Title` | yes | ≤ 256 | non-empty | = |
| `title_aliases` | `BoundedVec<Title, 8>` (`TITLE_ALIASES_MAX = 8`) | no | ≤ 8 × 256 | each alias non-empty | = |
| `artist` | `PartyId` | yes | — | id structure | = |
| `featuring` | `BoundedVec<PartyId, 16>` (`FEATURING_MAX = 16`) | no | ≤ 16 | each id: structure (featured artists = same `PartyId` as the main artist, like `Recording.featuring`) | **N** |
| `tracks` | `BoundedVec<Track, 256>` (`TRACKS_MAX = 256`) | yes | ≤ 256 | **non-empty (≥ 1)**; each `Track`: see below; **`RecordingRef` uniqueness** AND **strict contiguous 1-based `number`s** (`1, 2, …, N`) (`CrossFieldInconsistency`) | **N** |
| `producers` | `BoundedVec<Producer, 16>` (`PRODUCERS_MAX = 16`) | no | ≤ 16 | see below | |
| `status` | `ReleaseStatus` | yes | — | membership | S |
| `release_date` | `ReleaseDate` | yes | — | `month 1..=12`, `day 1..=31`; **`year` unconstrained** (cf. §7) | = |
| `country` | `Country` | yes | — | membership | S |
| `distributor_name` | `MiddsString<128>` (`DISTRIBUTOR_NAME_MAX_LEN = 128`) | yes | ≤ 128 | non-empty | = |
| `release_type` | `ReleaseType` | yes | — | membership | S |
| `format` | `ReleaseFormat` | yes | — | membership | S |
| `packaging` | `ReleasePackaging` | yes | — | membership | S |
| `cover_contributors` | `BoundedVec<MiddsString<128>, 16>` (`COVER_CONTRIBUTORS_MAX = 16`, `…_NAME_MAX_LEN = 128`) | no | ≤ 16 × 128 | each name non-empty | = |
| `offchain_extension` | `Option<OffchainHash>` | no | ≤ 64 | if `Some` ⇒ non-empty | = |

> No `manufacturer_name` field (present in the legacy frontend, **removed**
> in V1 — decision kept).

**`Track`**: `{ number: u16, recording: RecordingRef }`. `number` is the
1-based track number (≠ the vector index). Across the tracklist the numbers
must form a **strict contiguous 1-based sequence**: sorted, they are exactly
`1, 2, …, N` for an `N`-track release (`CrossFieldInconsistency` otherwise).
That single rule subsumes positivity (a `number` of 0 is rejected per-track
first, as `OutOfBounds`), uniqueness, the start at 1 and the step of 1 —
**gaps are rejected** (cf. §7). Numbering is checked as a *set*, so the stored
vector order is free: an out-of-order but complete `1..=N` numbering still
validates. `recording`: `RecordingRef` structure (if `Isrc` ⇒ ISRC structure)
and **unique** within the tracklist (no recording — hence no ISRC — listed
twice). The recording-uniqueness check uses a `BTreeSet` and the numbering
check a single sort, both O(n log n) over the ≤ 256 entries.

**`Producer`**: `{ isni: Isni, catalog_number: MiddsString<32> }`
(`CATALOG_NUMBER_MAX_LEN = 32`). `isni`: ISNI structure; `catalog_number`:
**non-empty** (each co-publishing label keeps its own number).

Closed enums:

- **`ReleaseStatus`** (7): `Official, Promotional, Bootleg, PseudoRelease,
  Withdrawn, Cancelled, Other`.
- **`ReleaseType`** (11): `Album, Single, Ep, Broadcast, Compilation,
  Soundtrack, Live, Remix, Mixtape, Demo, Other`.
- **`ReleaseFormat`** (11): `Cd, Vinyl, Cassette, DigitalDownload, Streaming,
  Dvd, BluRay, Sacd, MiniDisc, ReelToReel, Other`.
- **`ReleasePackaging`** (9): `None, JewelCase, SlimJewelCase, Digipak,
  CardboardSleeve, Gatefold, KeepCase, Box, Other`.

---

## 7. V1 asymmetries and assumed choices

Explicit, frozen decisions, not to be "fixed" without a version bump:

1. **`Release.release_date.year` unconstrained** (`1..=u16::MAX`), whereas
   `MusicalWork.creation_year` and `Recording.record_year` — both
   `Option<u16>` — are bounded `1..=2999` when present. Rationale: a release
   may be *announced for the future* (planned date); the legacy frontend in
   fact imposed no year bound on the release date (`z.date()` free). Only
   `month`/`day` are checked (a structural, not calendar, check: Feb 30 is
   accepted on-chain; strict calendar checking is `midds-validate`'s job).
2. **`Recording.duration` with no cap**: `Option<u32>` in seconds
   (≈ 136 years). The legacy frontend capped at 65535 s (18:12:15). The SDK
   keeps `u32` deliberately — no on-chain cap.
3. **`PartyId` with a `Both` variant**: stabilized V1 = `Ipi | Isni | Both {
   ipi, isni }`. The `Both` variant had initially been removed from the V1
   draft (by symmetry with other slimmed choices), then **reintroduced**:
   the same party frequently carries an IPI (CISAC) and an ISNI (ISO);
   merging them into a single on-chain structure saves SCALE versus two
   duplicated `Creator`s pointing at the same person, and restores the domain
   semantics. Validation: each present sub-identifier passes its
   `validate_*_format`.
4. **`Creator` merges roles in a `BoundedBTreeSet`**: the V1 draft
   represented "several roles for the same party" via a flat list of
   `Creator`s sharing the same `PartyId`. Stabilized V1 = a single
   `Creator { roles: Set, party }` per party — more compact encoding, no
   duplicates to validate, stable canonical order on the SCALE side.
5. **Structured `MusicalKey`** (`PitchClass × Mode`, 42) instead of the
   legacy frontend's 42 flat keys. `PitchClass` carries the 12 chromatic
   positions with their usual sharp and flat spellings plus the four
   cross-natural enharmonics (21 variants total): `D♭` and `C♯` are distinct
   on the wire — a decision driven by fidelity to registries (CWR/DDEX carry
   the spelling). The rarer `B♯`/`E♯`/`C♭`/`F♭` were appended (tags 17..=20)
   so keys notated with them — `C♯ major` spelling `E♯`/`B♯`, the `C♭`/`G♭`
   side spelling `C♭`/`F♭` — round-trip faithfully.
6. **Slimmed enums**: `Genre` 25 (vs ≈160), `RecordingVersion` 13 (vs 21),
   `ReleaseFormat` 11 (vs 63), `ReleasePackaging` 9 (vs 17), `ReleaseStatus`
   7 (vs 10), `ReleaseType` 11 (vs 6, redefined). Fine granularity = a
   future payload version, not a sub-type tree.
7. **Tight cardinality/length bounds**: the legacy frontend was very
   permissive (often 512 for party lists, 128–256 for free strings). The SDK
   keeps bounds optimized for on-chain cost (`CREATORS_MAX = 32`,
   `CREATOR_ROLES_MAX = 5`, `PERFORMERS_MAX = 64`, `PRODUCERS_MAX = 8/16`,
   `CONTRIBUTORS_MAX = 32`, `OPUS/CATALOG = 32`, `PLACE = 128`,
   `DISTRIBUTOR/COVER_NAME = 128`, `TITLE_ALIASES = 8`,
   `TRACKS = 256`, `COVER_CONTRIBUTORS = 16`). These values are the
   reference; the legacy frontend's figures (UI-only) are obsolete.
8. **Convention `instrumental ⇒ language = None` / `explicit_lyrics = false`**:
   non-blocking on-chain (the validator does not test it), to be surfaced as
   a warning in `midds-validate`.
9. **`MusicalWork.samples` accepts `WorkRef` (MIDDS id *or* ISWC), not
   `Medley`/`Mashup`**: the list of works sampled *by* this work takes both
   reference forms (`WorkRef::Midds | WorkRef::Iswc`) — a sample may be cited
   before the sampled work is registered — whereas `Medley`/`Mashup` refs
   stay ISWC-only. Assumed consequence: a sample's non-self-reference is only
   checkable for the `Iswc` variant (the `Midds` variant points to an id
   assigned at deposit, unknown at validation time). Bound `SAMPLES_MAX = 64`.
10. **`Recording.featuring` as `PartyId`, single `genre`/`sub_genre`, broad
    `Instrument`**: a featured artist is credited on the same footing as the
    main artist (`artist: PartyId`), not as a session performer — hence the
    same identity type (IPI / ISNI / both), distinct from the `Performer`
    which carries a `PerformerId` (IPN-capable) **and** the list of its
    instruments. Both `genre` and `sub_genre` are a *single* `Option<Genre>`
    drawn from the same flat taxonomy (a secondary refinement, not a
    hierarchical tree); a `sub_genre` may only accompany a primary `genre`.
    `Instrument` is deliberately *broad* (≈77 variants) —
    the inverse choice of the slimmed enums in point 6 — because the
    instrument played is a frequent and precise credit field; the cost stays
    one tag-byte per instrument, capped at `INSTRUMENTS_PER_PERFORMER_MAX = 8`
    per performer. **At least one instrument is mandatory**: once a performer
    id is entered, the instrument played must be named — an empty list is an
    incomplete credit and is rejected by `validate_format` (the performer id
    is format-checked first, so a malformed id surfaces its own error).
11. **`Release.featuring` and the `Track { number, recording }` tracklist**:
    `Release` now carries featured artists exactly like `Recording` —
    `BoundedVec<PartyId, 16>` (`FEATURING_MAX = 16`), same identities (IPI /
    ISNI / both) as the main artist. The tracklist moves from a raw
    `RecordingRef` to `Track { number: u16, recording: RecordingRef }`: each
    track carries a **mandatory** number. Frozen V1 choice: the numbers must
    form a **strict contiguous 1-based sequence** — sorted, exactly
    `1, 2, …, N` for an `N`-track release (they start at 1, increment by 1 and
    leave no gaps; positivity and uniqueness both fall out of that). An earlier
    V1 draft tolerated gaps ("unique & ≥ 1", no contiguity imposed); this
    stabilization **tightened** it to `1..=N`, because V1 `Release` models a
    single flat tracklist (no disc / medium concept) for which a complete,
    gapless global numbering is the only meaningful one. Numbering is validated
    as a *set*: the stored vector order is unconstrained (an explicitly
    numbered tracklist supplied out of input order still validates as long as
    its numbers cover `1..=N`), which keeps explicit track numbers meaningful.
    `RecordingRef` uniqueness (already in V1) is kept and coexists with the
    numbering rule. Builder operator decision (`midds-validate`,
    `midds-fixtures`): 1-based numbering in input order (the trivially
    contiguous default); the interactive CLI offers this default while letting
    the operator set numbers explicitly.

---

## 8. Where each rule applies

| Layer | Role |
|---|---|
| Type (`BoundedVec` / `MiddsString` / enum) | Max lengths, max cardinalities, enum membership — **impossible to violate** by construction/decoding. |
| `Midds::validate_format` (on-chain, blocking) | Identifier structure, non-emptiness of mandatory fields, **numeric bounds (`creation_year`, `record_year`, `bpm`, `number_of_voices`, `Track.number ≥ 1`)**, **minimum cardinality (`Medley/Mashup ≥ 2`, `tracks ≥ 1`, `creators ≥ 1`)**, **uniqueness + non-self-reference of `Medley`/`Mashup`/`Adaptation`/`Rearrangement` refs and of `samples`** + a `Release`'s `Track`s: **`RecordingRef` uniqueness and strict contiguous 1-based `number`s (`1, 2, …, N`)**, `release_date` month/day. Format only, never checksums. |
| `midds-validate` (std, warning-only, never on-chain) | Tolerant parsing, checksum verification (warnings), non-blocking conventions (`instrumental⇒language`, strict calendar checking, business `duration` cap). |

Cross-field invariants **enforced** on-chain (`CrossFieldInconsistency`):
`Release` tracklist integrity (no `RecordingRef` duplicated, and the `Track`
`number`s forming a strict contiguous `1..=N` sequence — a `number` of 0 is a
per-track `OutOfBounds` caught first); a `MusicalWork`
`Medley` / `Mashup` / `Adaptation` / `Rearrangement` source refs distinct and
different from the work's own `iswc`; a `MusicalWork`'s `samples` distinct
(no duplicate `WorkRef`) and, for the ISWC variant, different from the work's
`iswc`.

Reserved invariant not yet used: `MiddsFormatError::DateInconsistency`
(e.g. `recording_year > work_year`) — planned for a later cross-field
hardening without a new SCALE variant.
