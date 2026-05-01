# MIDDS SDK — Plan d'implémentation V1

> Document architectural de référence pour l'implémentation du repo `midds-sdk`.
> Cible : MusicalWork de bout en bout, architecture générique permettant
> l'ajout trivial de Recording et Release par la suite.

---

## 1. Contexte et périmètre V1

**Allfeat** : blockchain Substrate dédiée à l'industrie musicale.
**MIDDS** (Music Industry Decentralized Data Structure) : objets normés stockés
on-chain, identifiés par standards externes (ISWC, ISRC, IPI, ISNI…) — pas
d'objet Artist en V1, conformité RGPD par minimisation.

### Dans le périmètre V1

- 1 type MIDDS implémenté end-to-end : **MusicalWork** (draft de champs minimal,
  l'objectif est l'architecture, pas le modèle métier final)
- Pallet FRAME **générique multi-instance** : `pallet_midds<T, I>`, une `Instance`
  par type MIDDS dans le runtime
- SDK Rust complet : types, validation, runtime API, RPC, client subxt, codegen, CLI
- Mécaniques : dépôt permissionless avec bond, update sous fenêtre de gel,
  `force_*` via sudo
- Cible Polkadot SDK : **stable2503**, edition Rust **2024** (alignement avec
  le runtime `../Allfeat`)

### Hors scope V1

- `pallet-midds-party` (registre des contributeurs — spec à part, pallet
  séparée prévue mais pas implémentée ici)
- Modèle de pinning des extensions off-chain (qui pin, comment, incentives)
- Proof of Metadata (vérification communautaire — phase future)
- JSON Schema generation (reporté V2)
- TypeScript SDK officiel (le client TS sera dérivé via subxt → metadata par
  des bindings au moment d'ajouter Recording/Release)
- Recording et Release (triviaux à ajouter une fois MusicalWork stable)

---

## 2. Décisions architecturales verrouillées

> **Note** : le modèle économique (bond, pricing dynamique, fenêtre de
> finalisation, destination des fonds) est spec'd en détail dans
> [`docs/economics.md`](./economics.md). Les entrées #4, #5, #6, #10 ci-dessous
> sont les décisions structurantes ; voir `economics.md` pour la mécanique
> complète.

| # | Sujet | Décision |
|---|-------|----------|
| 1 | Toolchain | Polkadot SDK stable2503, Rust edition 2024 |
| 2 | Pattern pallet | Générique multi-instance (`pallet_midds<T, I>`) |
| 3 | Versioning | Enum top-level : `enum MusicalWork { V1(MusicalWorkV1) }`, migrations explicites via `OnRuntimeUpgrade` |
| 4 | Bond | `fungible::MutateHold` + `HoldReason` par instance, **fenêtre refundable 7j puis transfert Foundation Treasury** (cf. `economics.md`) |
| 5 | Formule de bond | `(DepositBase + DepositPerByte × encoded_size) × M_fast × M_slow` — multiplicateurs dynamiques anti-DoS et anti-flood (cf. `economics.md` §5) |
| 6 | Fenêtre 7j | `BlockNumberFor<T>`, **double rôle** : update (depositor) + refund via `remove_own` (cf. `economics.md` §4) |
| 7 | `ForceOrigin` | `EnsureRoot` (sudo kill-switch). **Deux variantes** : `force_remove_refund` (typo) et `force_remove_slash` (abus) |
| 8 | Validation on-chain | Format/mask uniquement (charset, longueur, structure) — **pas** de checksum |
| 9 | Validation riche | Côté `midds-validate` (std), checksums en *warning only* |
| 10 | Index identifier | **Multi-claim** : `IdentifierClaims: DoubleMap<Identifier, MiddsId, ()>`. Doublons d'identifier autorisés ; anti-doublon exact via `PayloadHashes: Map<H256, MiddsId>` |
| 11 | Compteur d'ID | `NextMiddsId` par instance |
| 12 | Tests | Mock runtime FRAME standard dans `pallet-midds/src/mock.rs` (pas de node, pas de runtime-example) |
| 13 | Client Rust | subxt uniquement |
| 14 | Contributeurs | Codes externes uniquement (IPI/ISNI), pas de référence vers Party registry |
| 15 | `OffchainHash` | `BoundedVec<u8, ConstU32<64>>` opaque on-chain, interprétation CIDv1 IPFS par convention côté client |

---

## 3. Layout du workspace

```
midds-sdk/
├── Cargo.toml                     # workspace, [workspace.dependencies] aligné stable2503
├── rust-toolchain.toml            # déjà présent
├── flake.nix                      # déjà présent
├── rustfmt.toml
├── clippy.toml
├── docs/
│   └── plan.md                    # ce document
├── crates/
│   ├── midds-traits/              # no_std — trait Midds, identifiants typés, erreurs
│   ├── midds-types/               # no_std — MusicalWork enum + V1 + impl Midds
│   ├── midds-validate/            # std    — regex, normalisation, checksums (warn)
│   ├── midds-runtime-api/         # no_std — sp_api::decl_runtime_apis
│   ├── midds-rpc/                 # std    — jsonrpsee handlers
│   ├── midds-client/              # std    — wrapper subxt
│   ├── midds-codegen/             # std    — bindings depuis metadata
│   └── midds-cli/                 # std    — bin (deposit, query, bulk, validate)
└── pallets/
    └── pallet-midds/              # générique multi-instance + mock + tests + benchmarks
```

`pallets/pallet-midds-party/` arrivera dans une PR séparée quand sa spec sera
disponible.

---

## 4. Pivot architectural : trait `Midds`

Tout l'écosystème (pallet, runtime API, validate, codegen) est générique sur
ce trait. Ajouter un nouveau type MIDDS = définir un struct + impl `Midds` +
nouvelle Instance dans le runtime. Zéro duplication.

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

### Identifiants typés

```rust
// crates/midds-traits/src/identifier.rs
use frame_support::{BoundedVec, traits::ConstU32};

pub type MiddsString<const N: u32> = BoundedVec<u8, ConstU32<N>>;

pub type Iswc = MiddsString<11>;  // T + 9 digits + 1 check char
pub type Isni = MiddsString<16>;  // 16 digits
pub type Ipi  = MiddsString<11>;  // up to 11 digits
pub type Isrc = MiddsString<12>;  // CC + 3 alpha + 2 year + 5 num
pub type OffchainHash = MiddsString<64>; // CIDv1 by convention

// Pure functions (no_std-safe), shared with midds-validate:
pub fn validate_iswc_format(b: &[u8]) -> Result<(), MiddsFormatError>;
pub fn validate_isni_format(b: &[u8]) -> Result<(), MiddsFormatError>;
pub fn validate_ipi_format(b: &[u8])  -> Result<(), MiddsFormatError>;
pub fn validate_isrc_format(b: &[u8]) -> Result<(), MiddsFormatError>;
pub fn validate_offchain_hash(b: &[u8]) -> Result<(), MiddsFormatError>;
```

### Erreurs

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

## 5. Spécifications par crate

### 5.1 `midds-traits` (no_std)

**Rôle** : interface entre les types et le pallet. Ne dépend de rien d'autre
(à part frame-support, scale).

**Dépendances** : `parity-scale-codec`, `scale-info`, `frame-support`.

**Contenu** : trait `Midds`, types alias identifiants, fonctions pures
`validate_*_format`, `MiddsFormatError`.

**Tests unitaires** : matrice pass/fail sur chaque `validate_*_format`.

### 5.2 `midds-types` (no_std)

**Rôle** : types canoniques MIDDS. Source de vérité côté Rust.

**Dépendances** : `midds-traits`, `parity-scale-codec`, `scale-info`,
`frame-support`.

**Contenu V1** :
- `musical_work/v1.rs` — `MusicalWorkV1` ultra-light (4 champs)
- `musical_work/mod.rs` — `enum MusicalWork { V1(MusicalWorkV1) }` + impl `Midds`

**Draft minimal `MusicalWorkV1`** :

```rust
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
         Clone, PartialEq, Eq, Debug)]
pub struct MusicalWorkV1 {
    pub iswc: Iswc,
    pub title: MiddsString<256>,
    pub creators: BoundedVec<Ipi, ConstU32<32>>,
    pub offchain_extension: Option<OffchainHash>,
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
        if let Some(h) = &self.offchain_extension {
            validate_offchain_hash(h)?;
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

**Note** : la liste réelle des champs (BPM, key, language, work_type,
classical_info, etc.) sera ajoutée dans une PR ultérieure une fois
l'architecture validée. La structure enum permet d'évoluer sans casser le
storage : `V2(MusicalWorkV2)` peut être ajouté plus tard avec migration.

**Tests** : encode/decode round-trip, format pass/fail, `MaxEncodedLen` borne
cohérente.

### 5.3 `pallet-midds` (no_std, le cœur)

**Rôle** : pallet FRAME générique multi-instance gérant le cycle de vie d'un
type MIDDS.

**Dépendances** : `midds-traits`, `frame-support`, `frame-system`,
`parity-scale-codec`, `scale-info`, `frame-benchmarking` (gated).

**Config** :

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

**Storage** :

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

**Type `Deposit`** :

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

**Extrinsics** (4) :

| Extrinsic | Origin | Conditions | Effets |
|-----------|--------|------------|--------|
| `deposit(item)` | `ProviderOrigin` | format valide ; identifier non encore présent | hold bond, insère item + index + deposit info, increment NextId, emit `Deposited` |
| `update(id, item)` | `ProviderOrigin` | caller == depositor ; `now - deposited_at <= UpdateWindow` ; identifier inchangé ; format valide | ajuste hold (delta), remplace item, emit `Updated` |
| `force_edit(id, item)` | `ForceOrigin` | item existe ; format valide ; identifier inchangé | bypass freeze, ajuste hold pour le déposant, remplace item, emit `ForceEdited` |
| `force_remove(id)` | `ForceOrigin` | item existe | release total, remove item + index + deposit info, emit `ForceRemoved` |

Logique partagée extraite en helpers privés :
- `do_validate_format(item) -> DispatchResult`
- `do_compute_bond(size) -> BalanceOf` : `DepositBase + DepositPerByte * size`
- `do_adjust_hold(account, old, new) -> DispatchResult` : compare et hold ou release le delta

**Events** :

```rust
#[pallet::event]
pub enum Event<T: Config<I>, I: 'static = ()> {
    Deposited { id: MiddsId, depositor: T::AccountId, bond: BalanceOf<T, I> },
    Updated { id: MiddsId, new_bond: BalanceOf<T, I> },
    ForceEdited { id: MiddsId },
    ForceRemoved { id: MiddsId },
}
```

**Errors** :

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

**Mock runtime** (`pallet-midds/src/mock.rs`) : `frame_system` +
`pallet_balances` + `pallet_midds::<Instance1>` instancié sur un `MockMidds`
trivial implémentant `Midds`, juste assez pour tester la mécanique
(identifier + payload).

**Tests** (`tests.rs`) — matrice de couverture :

- `deposit_works` (happy path : storage rempli, hold pris, event émis)
- `deposit_rejects_duplicate_identifier`
- `deposit_rejects_invalid_format`
- `deposit_holds_correct_bond` (DepositBase + DepositPerByte * size)
- `update_within_window_works`
- `update_after_window_freezes`
- `update_only_by_depositor`
- `update_keeps_identifier_immutable`
- `update_adjusts_bond_up` (taille en hausse → hold additionnel)
- `update_adjusts_bond_down` (taille en baisse → release partiel)
- `force_edit_bypasses_freeze`
- `force_edit_requires_root`
- `force_remove_releases_bond_and_clears_index`
- `force_remove_requires_root`

**Benchmarks** (`benchmarking.rs`) : parametrés sur la taille via
`BenchmarkHelper::bench_instance(s: u32) -> T::Midds` :

- `deposit(s)` worst-case
- `update(s)`
- `force_edit(s)`
- `force_remove`

`weights.rs` généré via `frame-benchmarking-cli`.

### 5.4 `midds-validate` (std)

**Rôle** : validation riche pour les outils dev/SDK. Ne tourne **jamais**
on-chain.

**Dépendances** : `midds-traits`, `midds-types`, `regex`, `thiserror`.

**Contenu** :

- Regex tolérantes : ISWC `^T-?\d{3}\.?\d{3}\.?\d{3}-?\d$`, idem ISNI/IPI/ISRC
- `parse_iswc(s: &str) -> Result<Iswc, ParseError>` : strip separators,
  uppercase, normalisation
- `verify_iswc_checksum(&Iswc) -> CheckResult { Pass, Fail, NotApplicable }` :
  *warning only*, jamais utilisé par le pallet (réel terrain bruité)
- Idem `verify_isni_checksum` (mod 11), `verify_ipi_checksum` (mod 10)
- `MusicalWorkBuilder` ergonomique : `.iswc()`, `.title()`,
  `.add_creator()`, `.build() -> Result<MusicalWork, BuildError>` qui aggrège
  les erreurs

Réutilise les `validate_*_format` de `midds-traits` pour zéro duplication.

### 5.5 `midds-runtime-api` (no_std)

**Rôle** : runtime API générique pour lookups par identifier et accès au
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
        fn lookup_by_identifier(id: Identifier) -> Option<MiddsId>;
        fn get(id: MiddsId) -> Option<Item>;
        fn deposit_info(id: MiddsId) -> Option<(AccountId, Balance)>;
    }
}
```

Le runtime fait `impl_runtime_apis!` une fois par instance (3 implémentations
à terme : MusicalWorks, Recordings, Releases).

### 5.6 `midds-rpc` (std)

**Rôle** : exposition JSON-RPC des runtime APIs via `jsonrpsee`.

Handler générique `MiddsRpc<C, B, Identifier, Item, AccountId, Balance>` →
un seul code, instancié N fois côté node. Namespace par instance :
`midds_musicalWork_lookupByIswc`, `midds_recording_lookupByIsrc`, etc.

**Note implémentation V1** : la macro `#[rpc(server)]` de `jsonrpsee` fige le
nom des méthodes émises. Avec une seule instance (V1 = MusicalWorks), les
méthodes sont publiées sous `midds_*` directement. Quand le node hébergera
plusieurs instances, il devra renommer manuellement chaque méthode en
`midds_<instance>_*` avant de merger les modules pour éviter les collisions.
Le multi-instance n'étant pas exercé en V1, le helper de renommage est
volontairement laissé au node d'intégration.

### 5.7 `midds-client` (std)

**Rôle** : wrapper ergonomique au-dessus de subxt.

**Dépendances** : `subxt`, `subxt-signer`, `midds-types`, `midds-validate`
(pour validation côté client avant submit).

**Structure V1** :

- `src/lib.rs` : façade typée par instance (`MusicalWorksApi`, …)
- `src/codec_bridge.rs` : bridge `parity_scale_codec::Encode` →
  `subxt::scale_encode::EncodeAsFields` permettant de passer les types MIDDS
  natifs (`BoundedVec`, etc.) à l'API de tx dynamique de subxt sans
  duplication de types
- `src/musical_works.rs` : tx + runtime-api calls pour l'instance MusicalWorks
- `src/error.rs` : `Error` agrégeant subxt + format MIDDS + decode

**Choix : tx & runtime-api dynamiques (`subxt::dynamic`)** plutôt que bindings
statiques générés. Avantages : pas de dépendance circulaire au moment du
bootstrap (pas besoin d'une metadata d'un runtime déjà en marche pour build le
client), pas de `src/generated.rs` versionné qui dérive à chaque update du
runtime, et les types MIDDS natifs (`midds-types`) restent les SoT côté Rust.
Coût : pas de vérification statique des noms de pallets/extrinsics — atténué
par les constantes `PALLET_NAME` / `RUNTIME_API_NAME` configurables.

`midds-codegen` reste fourni pour les consommateurs externes qui veulent des
bindings typés (TypeScript via `polkadot-api`, autres langages, runtimes
custom), mais `midds-client` lui-même ne les utilise pas.

API style :
```rust
let client = MiddsClient::connect("ws://localhost:9944").await?;
let id = client.musical_works().deposit(&signer, work).await?;
let found = client.musical_works().lookup_by_iswc(iswc).await?;
```

### 5.8 `midds-codegen` (std)

**Rôle** : génération de bindings Rust depuis la metadata Substrate.

Binaire wrappant `subxt-cli` :
```
cargo run -p midds-codegen -- \
    --metadata <ws-url|path> \
    --out crates/midds-client/src/generated/
```

Génère un module Rust statique. Optionnel à terme : feature gate pour générer
des bindings TS (via une chaîne `subxt → metadata-portal → polkadot-api`),
mais hors V1.

### 5.9 `midds-cli` (std, binaire)

**Rôle** : outil de debug, dépôts en masse, opérations.

**Commands** :

- `midds deposit musical-work <json-file>`
- `midds update <id> <json-file>`
- `midds query <iswc>`
- `midds bulk-deposit <jsonl-file>` : dépôts en masse depuis un fichier JSON Lines
- `midds force-remove <id>` (sudo signer)
- `midds validate <iswc>` : utilise `midds-validate`, affiche checksum en warning

---

## 6. Plan de phases / PRs

| PR | Contenu | Effort estimé | Bloqué par |
|----|---------|---------------|------------|
| 0  | Bootstrap workspace (Cargo.toml, rustfmt, clippy, crates vides) | 1h | — |
| 1  | `midds-traits` (trait Midds, identifiants, format validation) | 2-3h | 0 |
| 2  | `midds-types` (MusicalWork enum + V1 minimal) | 2-3h | 1 |
| 3a | `pallet-midds` core (config, storage, extrinsics, mock, tests) | 1-2j | 1, 2 |
| 3b | `pallet-midds` benchmarks + weights | 4-6h | 3a |
| 4  | `midds-validate` (regex, builder, checksums warn) | 4h | 1, 2 |
| 5  | `midds-runtime-api` + `midds-rpc` | 4-6h | 3a |
| 6a | `midds-codegen` | 3h | 5 (besoin metadata) |
| 6b | `midds-client` (façade subxt) | 6h | 6a |
| 6c | `midds-cli` | 4h | 6b, 4 |

**Chemin critique** : 0 → 1 → 2 → 3a (l'architecture générique en place).
Les autres PRs s'enchaînent ensuite, certaines en parallèle (3b et 4 peuvent
être faites en parallèle de 5).

---

## 7. Sécurité et invariants

### Invariants on-chain à préserver

- Un identifier ↔ au plus un MiddsId (unicité globale par instance)
- `DepositInfo` existe ssi `Items` existe (couplage strict)
- Le hold sur le bond est toujours = `DepositInfo.amount`
- L'identifier ne peut jamais changer après dépôt (immutabilité)
- `NextMiddsId` est strictement monotone croissant

### Tests d'invariants

Les tests de la PR 3a doivent vérifier explicitement chaque invariant après
chaque extrinsic.

### Vecteurs d'attaque considérés

- **Bond bypass** : `update` permet-il de réduire le bond et fuir avec ?
  → Non, le release est strictement proportionnel à la baisse de taille
- **Identifier squatting** : `force_edit` permet-il de réassigner un
  identifier à un autre item ? → Non, identifier immutable y compris pour
  `force_edit`
- **DOS via MIDDS énorme** : borné par `MaxEncodedLen` des types + bond
  proportionnel
- **Deletion sans release** : `force_remove` doit *toujours* release le hold

---

## 8. Conventions

- Edition 2024, `#![cfg_attr(not(feature = "std"), no_std)]` partout sauf bin
- Licensing : GPL-3.0
- Tous les types stockés on-chain : `MaxEncodedLen` obligatoire,
  `BoundedVec` partout, jamais `String` ou `Vec` libre
- Tous les identifiants : ASCII uniquement, validation charset on-chain
- Erreurs : enum dédié par crate, pas de `String` dans les erreurs on-chain

---

## 9. Items ouverts / à trancher plus tard

- Champs réels de `MusicalWorkV1` (BPM, key, language, classical_info,
  duration, etc.) — V1 livré minimal, itération ultérieure
- Modèle de pinning des extensions off-chain (déposant ? incentives type
  Crust/Filecoin ? noeuds Allfeat dédiés ?)
- Spec complète de `pallet-midds-party`
- Cadrage juridique RGPD à valider avec un juriste avant communication publique
- Choix précis du format `OffchainHash` (CIDv1 fixe vs multihash flexible) —
  borne 64 bytes laisse les deux options ouvertes sans migration

---

## 10. Démarrage

Une fois ce plan validé : démarrer par la PR 0 (bootstrap workspace).
Chaque PR suivante est validée par le user avant la suivante. Le découpage
en PRs petites et focalisées permet d'itérer sur l'architecture sans
accumuler de dette.
