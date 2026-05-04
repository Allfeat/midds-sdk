# Plan de stabilisation V1 — MusicalWork avant scale-out

> Document de plan : ce qu'il faut consolider sur le SDK MIDDS **avant**
> d'ajouter Recording et Release. Pendant de [`plan.md`](./plan.md),
> [`economics.md`](./economics.md) et [`testing.md`](./testing.md). Issu d'un
> audit complet (pallet / traits-types-validate / client-RPC-CLI /
> fixtures-CI) réalisé le 2026-05-04.

---

## 1. Contexte

V1 cible **MusicalWork end-to-end**. L'architecture générique (`trait Midds`
+ pallet multi-instance) est en place mais n'a jamais été exercée par un
deuxième type. L'audit a révélé trois familles de problèmes :

1. **Choix structurants encore corrigeables sans douleur** (trait, API
   client, runtime API) qui deviendront *breaking* dès qu'une 2e instance
   sera en production.
2. **Bugs/ambiguïtés dans le pallet** sur des invariants documentés mais non
   testés (bornes de fenêtre, bond cumulé, branches mortes).
3. **Friction d'extension** dans `midds-fixtures`, `midds-cli` et `bench/`
   qui forcera du copier-coller à chaque nouveau type.

Objectif de ce plan : refermer ces trois fronts en 4 sprints PR-sized
(~5–6 jours total) **avant** de toucher à Recording.

---

## 2. Séquencement

| Sprint | Goal | Effort | Bloque | Bloqué par |
|---|---|---|---|---|
| A | `trait Midds` enrichi + API client/runtime-api complète | 1.5–2j | Recording, Release | — |
| B | Bugs pallet + invariants couverts par property tests | 1j | — | — |
| C | Génériser `midds-fixtures`, `midds-cli` et `bench/` sur `M: Midds` | 1.5–2j | Recording (corpus, builders) | A |
| D | CI durcie + dette doc + quick wins | 0.5j | release-plz prod-ready | — |

A et B sont indépendants → peuvent partir en parallèle. C dépend de A pour
l'API client générique. D peut s'intercaler à tout moment.

---

## 3. Sprint A — `trait Midds` enrichi + API client/runtime-api complète

### 3.1 Goal

Geler les contrats publics qui touchent toutes les instances (trait,
runtime API, namespace RPC, façade client) **maintenant**, pendant qu'il n'y
a qu'une seule impl à migrer. Ce sont les seuls changements de ce plan qui
sont réellement *breaking* SCALE.

### 3.2 Tâches

#### 3.2.1 `trait Midds` — `KIND` + `identifier()` par référence

`crates/midds-traits/src/lib.rs:20`

```rust
pub trait Midds: Parameter + MaxEncodedLen {
    /// Stable string discriminator (used by events, RPC, indexers).
    /// Convention: PascalCase singular ("MusicalWork", "Recording", "Release").
    const KIND: &'static str;

    type Identifier: Parameter + MaxEncodedLen + Ord;

    fn identifier(&self) -> &Self::Identifier;  // était: -> Self::Identifier
    fn validate_format(&self) -> Result<(), MiddsFormatError>;
}
```

- Mettre à jour `crates/midds-types/src/musical_work/mod.rs` (impl Midds).
- Mettre à jour `pallets/pallet-midds/src/{lib,mock}.rs` — chaque `.identifier()`
  perd son `.clone()`.

#### 3.2.2 `MiddsFormatError` — variants prospectifs

`crates/midds-traits/src/error.rs:11`

Ajouter `DateInconsistency` et `CrossFieldInconsistency` (planifiés
`plan.md:166`, jamais implémentés). Recording et Release en auront besoin
(year vs release_year, tracklist vs durations). C'est un `enum` SCALE → tout
ajout ultérieur serait breaking.

#### 3.2.3 Runtime API — `DepositInfoOf` typé

`crates/midds-runtime-api/src/lib.rs:41`

Remplacer le tuple `(AccountId, Balance, Balance, bool)` par :

```rust
#[derive(Encode, Decode, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug)]
pub struct DepositInfoOf<AccountId, Balance> {
    pub depositor: AccountId,
    pub amount: Balance,
    pub price_at_deposit: Balance,
    pub finalized: bool,
}
```

Propager dans `midds-rpc/src/lib.rs:39` (alias `DepositInfoView` à supprimer
au profit du type partagé) et dans `crates/midds-client/src/pallet/api.rs:381`
(décodage manuel du tuple à supprimer).

#### 3.2.4 Client — méthodes query type-safe

`crates/midds-client/src/pallet/api.rs`

Le helper privé `runtime_api<T: Decode>` existe déjà ; ajouter publiquement
sur `PalletApi<M: Midds>` :

```rust
pub async fn lookup_by_identifier(&self, id: &M::Identifier) -> Result<Vec<MiddsId>>;
pub async fn get(&self, id: MiddsId) -> Result<Option<M>>;
pub async fn deposit_info(&self, id: MiddsId) -> Result<Option<DepositInfoOf<AccountId, Balance>>>;
```

Pas de logique nouvelle — câblage trivial, alignement sur le runtime API.

#### 3.2.5 RPC — namespacing préparé pour multi-instance

`crates/midds-rpc/src/lib.rs:46`

Le commentaire (l. 11-20) reconnaît la dette. Deux options :

- **Option 1 (préférée)** : macro `define_midds_rpc!(MusicalWorks)` qui
  génère le trait `#[rpc(server)]` avec préfixe `midds_musicalWorks_*`.
- **Option 2 (minimale)** : helper `rename_methods(module: RpcModule, prefix: &str)`
  pour que le node renomme à l'enregistrement, sans macro.

Choisir Option 1 si le coût macro reste raisonnable (~50 lignes), sinon
Option 2 documenté comme *opt-in côté node*.

### 3.3 Tests

- `crates/midds-traits/tests/kind.rs` — assert `MusicalWork::KIND == "MusicalWork"`.
- `crates/midds-runtime-api/tests/deposit_info_codec.rs` — round-trip SCALE
  de `DepositInfoOf` (la migration tuple → struct **doit** être backward-incompatible
  par design, mais le test verrouille la nouvelle représentation).
- `crates/midds-client/tests/query_smoke.rs` — appel des 3 nouvelles méthodes
  contre un mock subxt (ou marqué `#[ignore]` si trop lourd, exécuté en E2E).

### 3.4 Critères de validation

- `cargo test --workspace --all-features` vert.
- `cargo build -p pallet-midds --no-default-features --target wasm32-unknown-unknown`.
- Aucune occurrence de `.identifier().clone()` ne subsiste.
- `grep -rn 'tuple.*deposit_info\|(AccountId, Balance, Balance, bool)' crates/` retourne 0.
- CHANGELOG : marquer `BREAKING CHANGE: Midds trait now requires KIND const; identifier() returns &Identifier; runtime API DepositInfo is now a struct.`

---

## 4. Sprint B — Bugs pallet + invariants couverts

### 4.1 Goal

Refermer les ambiguïtés de comportement détectées dans le pallet et **prouver
par property tests** les invariants documentés `plan.md:560-566`. Tout ce
qui passe ce sprint vaut pour toutes les futures instances gratuitement.

### 4.2 Tâches

#### 4.2.1 Borne unique de la fenêtre 7j

`pallets/pallet-midds/src/lib.rs:494,520,551`

État actuel : `update`/`remove_own` utilisent `elapsed <= window`,
`finalize` utilise `elapsed > window`. Au bloc `expiry` exact, l'ordre
intra-bloc décide qui gagne (non-déterministe pour le user).

Décision proposée : **`<` partout**. Le bloc `expiry` lui-même est
finalisable. Cohérent avec `economics.md` ("fenêtre 7 jours strictement
inférieure"). Documenter dans le doc-comment de `Config::CommitmentWindow`.

Helper à extraire : `fn ensure_in_window(info: &Deposit, now: BlockNumber)
-> DispatchResult` — utilisé par `update` et `remove_own`.

#### 4.2.2 Borner `force_remove_many`

`pallets/pallet-midds/src/lib.rs:603`

```rust
#[pallet::constant]
type MaxRemovalsPerCall: Get<u32>;
```

Signature passe de `Vec<MiddsId>` à `BoundedVec<RemovalRequest, T::MaxRemovalsPerCall>`
où :

```rust
pub enum RemovalKind { Refund, Slash }
pub struct RemovalRequest { id: MiddsId, kind: RemovalKind }
```

Bénéfices : weight bornable, fin du flag `slash: bool` global (chaque id
peut avoir son traitement). Mock runtime : `MaxRemovalsPerCall = 32`.

#### 4.2.3 Branche morte `do_apply_edit`

`pallets/pallet-midds/src/lib.rs:670-682`

Soit retirer la branche `DuplicatePayload` (interceptée plus tôt par
`IdentifierClaims::contains_key`), soit ajouter un test qui force le path
résiduel (deux items même identifier, payloads différents, second update
qui collide en hash avec un troisième). Décision recommandée : **garder + tester**,
car la pré-vérification pourrait sauter dans une refacto future.

#### 4.2.4 Property tests des invariants `plan.md:560-566`

`pallets/pallet-midds/src/property_tests.rs`

Ajouter un test générant une séquence arbitraire d'extrinsics
(`deposit`, `update`, `remove_own`, `force_remove_*`, `on_initialize`)
puis assertant après chaque step :

| Invariant | Assertion |
|---|---|
| Bond cumulé | `pour chaque account: held(account) == Σ DepositInfo[id].amount where depositor==account` |
| Couplage Items↔DepositInfo | `Items::iter().count() == DepositInfo::iter().count()` |
| Cardinalité PayloadHashes | `PayloadHashes::iter().count() == Items::iter().count()` |
| Identifier immuable | `forall id: identifier_at(t) == identifier_at(t-1)` |
| `NextMiddsId` monotone | jamais décroissant |

Cas pathologiques à inclure : `arb_invalid_mock_midds()` produisant des
payloads malformés et assertant `assert_noop!(InvalidFormat)`.

#### 4.2.5 Tests `current_deposit_price` runtime API

`pallets/pallet-midds/src/lib.rs:923`

Pin un test mock : prix retourné == bond effectivement débité par un
`deposit` immédiat dans le même bloc.

#### 4.2.6 Test mass-injection avec multipliers actifs

`pallets/pallet-midds/tests/mass_injection.rs`

Nouveau scénario `mass_injection_10k_with_multipliers` ciblant ~1000
extrinsics/bloc pour faire monter `M_fast` au-dessus de 1.0×. Storage root
committé. **Sans ça, la logique `economics.md` n'est testée par aucune
fixture.**

### 4.3 Critères de validation

- Tous les invariants de `plan.md:560-566` ont un test prop dédié.
- `cargo test -p pallet-midds property_tests --release` passe avec
  `PROPTEST_CASES=10000`.
- Aucune borne `<=` / `>` ne reste sur `elapsed` vs `CommitmentWindow`.
- `force_remove_many` accepte au plus `MaxRemovalsPerCall` ids.

---

## 5. Sprint C — Génériser `midds-fixtures`, `midds-cli` et `bench/`

### 5.1 Goal

Tuer toute mention hardcodée de `MusicalWork` dans les outils transverses
pour qu'**ajouter Recording = définir un struct + impl `Midds` + un
corpus**, point. Aujourd'hui ce serait copier-coller 3×.

### 5.2 Tâches

#### 5.2.1 `midds-fixtures` — trait + sous-modules par type

`crates/midds-fixtures/src/`

Extraire dans `lib.rs` :

```rust
pub trait MiddsFixtures {
    type Item: Midds;
    fn corpus() -> &'static [Self::Item];
    fn strategy() -> BoxedStrategy<Self::Item>;
    fn gen_n(seed: u64, n: usize) -> Vec<Self::Item>;
    fn pathological() -> Vec<Self::Item>;
}
```

Refactor `musical_work/{mod,builder,strategy,corpus}.rs` pour implémenter
`MiddsFixtures for MusicalWorkFixtures`. Helpers transverses
(`BoundedFieldStrategy<N>`, `ChecksumIdStrategy`) extraits dans
`crates/midds-fixtures/src/common.rs`.

#### 5.2.2 ISRC support dans `midds-validate` + `midds-fixtures`

Manquant aujourd'hui :
- `crates/midds-validate/src/parse.rs` : `parse_isrc(s: &str) -> Result<Isrc, ParseError>`
  (regex tolérante `^[A-Z]{2}-?[A-Z0-9]{3}-?\d{2}-?\d{5}$`).
- `crates/midds-validate/src/checksum.rs` : `verify_isrc_checksum` (mod-7).
- `crates/midds-fixtures/src/identifiers.rs` : `isrc_valid_strategy()`,
  `isrc_invalid_strategy()` + corpus de ~50 ISRC réels (`data/isrc_real_sample.json`).

(Sera consommé par Recording, mais l'effort est ici parce qu'il est
mécaniquement transverse.)

#### 5.2.3 `MusicalWorkBuilder` (mandaté `plan.md:425`, jamais fait)

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

Pattern réutilisable : doc-comment `// Recording/Release suivront ce template`.

#### 5.2.4 CLI + bench paramétrés sur `M: Midds`

Cibles :

- `crates/midds-cli/src/bench/{seed,fees,throughput,worker,util}.rs`
- `crates/midds-cli/src/admin.rs:21,151,171,227`

Approche minimale : enum CLI

```rust
#[derive(clap::ValueEnum, Clone)]
enum MiddsKind { MusicalWork /* + Recording, Release plus tard */ }
```

Les fonctions de scaffolding (`setup_runner`, `partition_round_robin`,
`auto_fund`) deviennent génériques sur `M: Midds + Encode`. Les commandes
CLI prennent `--midds-type <kind>` (default `musical-work`) et dispatchent.

V1 : seul `MusicalWork` est branché — mais l'enum + dispatch en place.
Recording = ajouter une variante.

#### 5.2.5 Constantes pallet/event regroupées

`crates/midds-client/src/pallet/{api,events}.rs`

Aujourd'hui dispersé (`"DepositBase"`, `"NextMiddsId"`, `"Deposited"`,
`"TransactionPayment"`…). Regrouper dans un module `pallet::names` (ou
mieux : injecté via `Midds::KIND` du sprint A pour le préfixe pallet).

### 5.3 Critères de validation

- `grep -rn 'MusicalWork' crates/midds-cli/src/bench/` retourne uniquement
  les variantes d'enum `MiddsKind`.
- `grep -rn 'MusicalWork' crates/midds-fixtures/src/lib.rs crates/midds-fixtures/src/common.rs`
  retourne 0.
- `parse_isrc` + `verify_isrc_checksum` ont chacun ≥ 5 cas pass + 5 cas fail.
- `MusicalWorkBuilder` a un test "happy path" + un test "agrégation de 3 erreurs".

---

## 6. Sprint D — CI durcie + dette doc + quick wins

### 6.1 Goal

Refermer les trous qui laisseraient passer une régression silencieuse, et
nettoyer la doc/le repo des incohérences identifiées.

### 6.2 Tâches

#### 6.2.1 CI — jobs manquants

`.github/workflows/ci.yml`

Ajouter :

```yaml
- name: Check pallet with runtime-benchmarks
  run: cargo check -p pallet-midds --features runtime-benchmarks
```

Nouveau workflow `.github/workflows/nightly.yml` (cron quotidien) :

```yaml
- name: Mass injection 50k/100k
  run: cargo test -p pallet-midds --test mass_injection -- --ignored
- name: Property tests intensive
  env: { PROPTEST_CASES: "10000" }
  run: cargo test -p pallet-midds property_tests --release
```

#### 6.2.2 `release-plz` — required status checks

`.github/workflows/release-plz.yml:7-10` reconnaît que `GITHUB_TOKEN` ne
trigger pas CI sur la PR de release. Avant la 1ère release publique :

- Soit basculer sur PAT/GitHub App (préféré).
- Soit ajouter une required-status-check sur `master` qui force un re-run
  CI manuel avant merge.

#### 6.2.3 `clippy.toml` durci

`clippy.toml`

Ajouter :

```toml
disallowed-methods = [
    { path = "core::result::Result::unwrap", reason = "use ? or expect with context" },
    { path = "core::option::Option::unwrap", reason = "use ? or expect with context" },
]
```

(Tests excepted via `#[allow(clippy::disallowed_methods)]` localisé.)

#### 6.2.4 `Justfile`

Nouveau fichier racine :

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

#### 6.2.5 Doc — corriger `plan.md`

`docs/plan.md`

- L. 445 : `lookup_by_identifier(id) -> Option<MiddsId>` → `Vec<MiddsId>`
  (multi-claim acté).
- Section 5.4 `MusicalWorkBuilder` : si fait au sprint C, marquer ✅,
  sinon préciser "implémenté dans Sprint C de v1-hardening".
- Renvoyer vers ce document (`v1-hardening.md`) en haut de section 6.

#### 6.2.6 Quick wins

- `seed.json` racine : ajouter `/seed.json` au `.gitignore` et supprimer
  le fichier (orphelin, jamais lu).
- `crates/midds-types/src/language.rs:42` : ajouter
  `from_code_ignore_ascii_case`.
- `crates/midds-traits/src/identifier.rs` : `static_assertions::const_assert_eq!`
  sur la `max_encoded_len` de chaque alias (`Iswc`, `Isni`, `Ipi`, `Isrc`,
  `OffchainHash`).

### 6.3 Critères de validation

- CI master : 6 jobs (fmt, clippy, test, wasm, bench-check, commitlint).
- CI nightly : configurée et passe au moins une fois.
- `just check` reproduit la CI en local.
- `seed.json` n'existe plus.

---

## 7. Hors scope (volontairement reporté)

- **Refacto `do_force_remove_slash` post-finalisation** (`lib.rs:797-816`) :
  comportement correct, juste opérations redondantes inoffensives. Pas
  bloquant.
- **`apply_multipliers` precision sur petites bases** : faux problème en
  prod (planck à 10^12 décimales). Test mock peut être ajouté en sprint B
  si trivial, sinon ignorer.
- **`SLOW_WINDOW_DAYS = 7` hardcodé** (`lib.rs:79`) : par spec
  (`economics.md` §4) ce 7j est **identique pour toutes instances**.
  Configurabilité = YAGNI.
- **Snapshot-tests `midds-codegen`** : le crate est documenté comme
  "external consumers only", `midds-client` n'en dépend pas. Pas de valeur
  à tester sans un runtime cible.
- **Tests d'intégration `midds-client` contre un node éphémère** : le
  bench/ E2E dans `midds-cli` couvre déjà le path. À reconsidérer si on
  introduit une 2e instance sans repasser par `midds-cli`.

---

## 8. Démarrage suggéré

1. Ouvrir 4 issues GitHub (une par sprint) avec ce document en référence.
2. Sprints A et B en parallèle (deux PRs distinctes, branches séparées).
3. Sprint C démarre dès que A est mergé (dépendance API client).
4. Sprint D peut être tronçonné en sous-PRs (CI / clippy / quick wins
   indépendants).

Critère de "v1 stabilisé, prêt pour Recording" :

- Les 4 sprints mergés sur `master`.
- Une release `0.2.0` (ou `0.1.x` selon politique semver pré-1.0) cuttée
  via `release-plz`.
- CHANGELOG agrégeant les `BREAKING CHANGE` du sprint A.
