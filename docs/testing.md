# MIDDS SDK — Plan testing & mocking

> Document de référence pour la stratégie de tests et de mocking du repo
> `midds-sdk`. Pendant de `docs/plan.md`. Cible : un harnais stable et
> professionnel couvrant unitaire, property-based, masse, fees réelles,
> end-to-end, et seeding de chaînes de dev pour le frontend.

---

## 1. Principes directeurs

- **`midds-fixtures` est la pierre angulaire** : une seule source de vérité
  pour "à quoi ressemble un MIDDS plausible". Toutes les couches en
  dépendent.
- **Pas de duplication runtime** : couches 1–3 vivent dans `midds-sdk` (mock
  FRAME), couche 4 vit dans `Allfeat` (melodie-runtime réel), couche 5 est
  portable via `midds-cli`.
- **Déterministe par défaut** : RNG seedé pour qu'un test "10 000 MIDDS
  générés" soit reproductible bit-à-bit.
- **Pas de `midds-loadgen` séparé** : on étend `midds-cli` avec des
  sous-commandes `bench` et `seed` (cohérent avec son rôle de client
  opérateur).
- **Une couche = une question distincte**. Pas de chevauchement de
  responsabilité : si un bug peut être attrapé à plusieurs niveaux, il est
  attrapé au plus bas.

---

## 2. Vue d'ensemble — 5 couches

| Couche | Question à laquelle elle répond | Localisation | Outillage |
|---|---|---|---|
| 1. Unit pallet | Le lifecycle marche-t-il ? | `pallets/pallet-midds/src/tests.rs` | mock FRAME, `MockMidds` |
| 2. Property-based pallet | Les invariants tiennent-ils sur 10k cas générés ? | `pallets/pallet-midds/src/property_tests.rs` | `proptest` sur le mock |
| 3. Mass injection mock | Storage / weights / bond cumulé scalent-ils ? | `pallets/pallet-midds/tests/mass_injection.rs` | mock FRAME + boucle N=10k–100k |
| 4. Runtime integration | **Fees réelles** sur melodie-runtime | `Allfeat/runtime/melodie/tests/midds_integration.rs` | `TestExternalities` sur `melodie-runtime` |
| 5. E2E node | Inclusion réelle, RPC, multi-comptes | `crates/midds-cli/src/bench/` | node `--dev` + subxt + `midds-client` |

---

## 3. Crate `midds-fixtures` (NEW)

Localisation : `crates/midds-fixtures/`. Std-only, pas de no_std.

### 3.1 Layout

```
crates/midds-fixtures/
├── Cargo.toml
├── data/                         # JSON committés
│   ├── iswc_real_sample.json     # ~500 ISWC valides (anonymisés)
│   ├── ipi_codes.json
│   ├── languages.json
│   └── titles_corpus.json        # corpus de titres plausibles
└── src/
    ├── lib.rs
    ├── musical_work/
    │   ├── strategy.rs           # proptest::Strategy
    │   ├── builder.rs            # builder pattern (test ergonomique)
    │   └── corpus.rs             # accès aux fixtures statiques
    ├── identifiers.rs            # ISWC, IPI, ISNI valides (checksum-correct)
    ├── pathological.rs           # cas borderline / pathologiques
    └── rng.rs                    # SeededRng helper
```

### 3.2 API publique

- `MusicalWorkBuilder` : `.with_iswc(...).with_title(...).build()` pour les
  tests unitaires lisibles.
- `arb_musical_work()` : `proptest::Strategy<Value = MusicalWork>` pour
  property tests.
- `arb_musical_work_max_size()` : payloads à `MaxEncodedLen` exacte.
- `arb_musical_work_invalid()` : génère systématiquement des cas qui
  doivent échouer en validation.
- `corpus::iter_real_iswcs()` : itérateur sur le dataset réel.
- `gen_n(seed, count) -> Vec<MusicalWork>` : génération déterministe en
  masse pour seed/loadgen.

### 3.3 Features Cargo

- `default = ["proptest"]`
- `proptest` : active `proptest::Strategy`
- `corpus` : embarque les JSON dans le binaire (sinon lus à l'exécution
  depuis `CARGO_MANIFEST_DIR`)

### 3.4 Datasets statiques

Les JSON sont anonymisés mais structurellement réalistes (charset, longueur,
distribution). À régénérer si la spec d'identifiants évolue. Aucune donnée
RGPD-sensible (pas de noms d'auteurs réels, juste codes industrie).

---

## 4. Couche 1 — Unit tests pallet (existant, à raffiner)

Fichier : `pallets/pallet-midds/src/tests.rs` (déjà 435 lignes).

**Action** : refactor minimal pour consommer `midds-fixtures` au lieu des
helpers ad-hoc. Garder la liste actuelle de cas (lifecycle, freeze window,
`force_*`, errors). Vérifier la couverture des branches `MutateHold`
(hold/release/transfer-on-slash).

Pas d'ajout de cas ici — les nouveaux cas vont dans les couches 2 et 3.

---

## 5. Couche 2 — Property-based pallet (NEW)

Fichier : `pallets/pallet-midds/src/property_tests.rs` (gated `#[cfg(test)]`).

### 5.1 Invariants à prouver

Chaque invariant = un `proptest!` block dédié.

| Invariant | Description |
|---|---|
| `bond_formula` | `bond_held(account) == DepositBase + DepositPerByte * encoded_size(midds)` après chaque `deposit()` |
| `force_remove_releases` | `force_remove()` libère exactement le bond initialement tenu (pas plus, pas moins) |
| `update_preserves_id` | `update()` ne modifie jamais l'identifiant canonique ni `NextMiddsId` |
| `freeze_window_blocks_update` | Tout `update()` dans `< owned_since + UpdateWindow` retourne `Error::Frozen` |
| `unique_canonical_id` | Aucune séquence d'opérations ne permet deux `MusicalWork` avec le même ISWC en storage |
| `encoded_len_consistency` | `midds.encoded_size() <= <Midds as MaxEncodedLen>::max_encoded_len()` pour tout MIDDS issu de `arb_musical_work()` |
| `events_match_storage` | Pour toute séquence d'extrinsics, les events émis reflètent les diffs storage |

### 5.2 Volume

- Default : `proptest_cases = 256` (PR rapide).
- Override CI nightly : `PROPTEST_CASES=10000`.
- Persistance des contre-exemples : `proptest-regressions/` commit dans le
  repo (pratique standard `proptest`).

---

## 6. Couche 3 — Mass injection sur mock (NEW)

Fichier : `pallets/pallet-midds/tests/mass_injection.rs` (test
d'intégration, hors `#[cfg(test)]`).

### 6.1 Scénarios

| Scénario | N | Comptes |
|---|---|---|
| `mass_injection_10k` | 10 000 | 1 |
| `mass_injection_50k` | 50 000 | 100 |
| `mass_injection_100k` | 100 000 | 1 000 (CI nightly only) |
| `mass_injection_max_size` | 1 000 | chaque MIDDS à `MaxEncodedLen` |

### 6.2 Mesures consignées

Chaque test sort un fichier
`target/test-reports/mass_injection_<scenario>.json` :

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

### 6.3 Anti-régression

`storage_root_hash` est checké contre une fixture committée
(`tests/fixtures/storage_root_10k.txt`). Si la formule de bond ou
l'encoding change, le test échoue avec un diff explicite. Force à updater
consciemment.

---

## 7. Couche 4 — Runtime integration tests (côté `Allfeat`)

**Hors `midds-sdk`** : vit dans
`Allfeat/runtime/melodie/tests/midds_integration.rs`.

### 7.1 Pourquoi ailleurs

`midds-sdk` ne doit pas dépendre de `melodie-runtime` (cf. décision de
découplage melodie/mainnet). Le runtime côté `Allfeat` consomme déjà le SDK
en path-dep, donc l'inverse créerait un cycle.

### 7.2 Setup

Dépendances : `melodie-runtime` + `midds-fixtures` + `sp-io`.

`TestExternalities` construit depuis `melodie-runtime::GenesisConfig::default()`
avec balances pré-mintées pour 100 comptes.

### 7.3 Scénarios fee-réel

| Test | Mesure |
|---|---|
| `fees_small_musical_work` | bond + tx fee pour MusicalWork ~50 bytes |
| `fees_avg_musical_work` | bond + tx fee pour MusicalWork ~200 bytes |
| `fees_max_musical_work` | bond + tx fee pour MusicalWork à `MaxEncodedLen` |
| `fees_distribution_1000` | distribution complète (p50 / p95 / p99) sur 1k MIDDS issus de `arb_musical_work()` |

### 7.4 Sortie

`target/test-reports/fees_report.md` — tableau markdown commit-able dans le
PR :

```
| Size (bytes) | Bond (AFT) | Weight fee (AFT) | Length fee (AFT) | Total user cost |
|--------------|------------|------------------|------------------|-----------------|
| 50           | ...        | ...              | ...              | ...             |
```

Sert de baseline pour décider de tuner `DepositBase` / `DepositPerByte`.

---

## 8. Couche 5 — E2E node via `midds-cli` (extension)

Plutôt qu'une nouvelle crate, on étend la CLI existante.

### 8.1 Sous-commandes ajoutées

```
midds-cli seed
  --node ws://localhost:9944
  --count 50000
  --rng-seed 0xABCD...                # optionnel, défaut = déterministe
  --concurrency 16                     # extrinsics in-flight
  --signers alice,bob,//Alice//1..100  # multi-comptes
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

### 8.2 Architecture interne

- Nouveau module `crates/midds-cli/src/bench/` (mod.rs, seed.rs, fees.rs,
  throughput.rs, verify.rs).
- Réutilise `midds-client` pour les extrinsics, `midds-fixtures` pour la
  génération.
- Les rapports de seed sont rejoués-vérifiables via `verify-state`.

### 8.3 Multi-comptes

Dérivation déterministe `//Alice//<N>` (jusqu'à plusieurs milliers).
Pré-funding via une sous-commande dédiée :

```
midds-cli admin pre-fund-signers --count 1000 --amount 1000AFT
```

Implémentée via une extrinsic `force_set_balance` (sudo) côté chain dev. À
documenter clairement comme outil de dev uniquement.

---

## 9. Workflow snapshot pour frontend

Documenté dans `docs/seeding.md` (à créer en parallèle).

### 9.1 Workflow type

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

### 9.2 Distribution du snapshot

Le fichier `seeded-state.json` peut être :
- commit dans un repo fixtures séparé (si <100 Mo),
- ou release-asset GitHub attaché à un tag du SDK (recommandé pour la
  taille >100 Mo).

**Reproductibilité** : `--rng-seed` fixé garantit que le snapshot est
régénérable bit-à-bit. CI peut produire un nouveau snapshot à chaque
release.

---

## 10. Benchmarks weights (existant, à étendre)

Fichier : `pallets/pallet-midds/src/benchmarking.rs` (108 lignes
actuellement).

**Action** : ajouter le worst-case avec `MaxEncodedLen` (utilise
`midds-fixtures::arb_musical_work_max_size`) pour tous les extrinsics.
Régénérer les weights via `frame-omni-bencher` une fois par release SDK.

**Orthogonal** aux benchmarks de perf utilisateur (Couche 5 throughput) —
ne pas confondre. Ici on calibre les weights FRAME, pas le débit réseau.

---

## 11. CI cadence

| Étape | Trigger | Durée cible |
|---|---|---|
| Couches 1 + 2 (default proptest cases) | Chaque PR | <2 min |
| Couche 3 (10k seulement) | Chaque PR | <5 min |
| Couche 3 (100k) + property avec `PROPTEST_CASES=10000` | Nightly | <30 min |
| Couche 4 (côté Allfeat) | Nightly Allfeat | <10 min |
| Couche 5 throughput | Manuel + tag release | variable |
| Régénération weights + snapshot seeded | Tag release | <1h |

---

## 12. Cas pathologiques à couvrir explicitement

À distribuer dans les bonnes couches, mais listés une fois pour ne rien
oublier :

- MIDDS à `MaxEncodedLen` exact (bond max).
- MIDDS minimal (bond min, mais ≥ ED).
- Charset borderline (caractères ASCII limites).
- Compte sans funds suffisants pour le bond.
- Update pile à `owned_since + UpdateWindow` (off-by-one).
- 10k MIDDS depuis un seul compte (bond cumulé énorme).
- Concurrence : deux updates simultanés sur même MIDDS (transactionnalité).
- Storage migration fictive V1→V2 (test que la mécanique
  `OnRuntimeUpgrade` marche).
- ID canonique avec collision (rejet propre).
- `force_remove` d'un MIDDS en freeze window (doit passer, prouve que
  sudo bypass marche).

---

## 13. Plan d'exécution

Ordre qui maximise la valeur incrémentale. Chaque étape est mergeable
indépendamment.

| # | Étape | Bénéfice immédiat |
|---|---|---|
| 1 | `midds-fixtures` skeleton + datasets + `MusicalWorkBuilder` | Débloque tout le reste |
| 2 | Refactor Couche 1 pour utiliser fixtures | Valide l'API |
| 3 | Couche 2 property tests | Trouve probablement des bugs latents |
| 4 | Couche 3 mass injection | Pose le baseline `storage_root` (anti-régression solide) |
| 5 | `midds-cli seed` + `verify-state` | Débloque le frontend immédiatement |
| 6 | Couche 4 côté Allfeat | Fees report concret pour décisions de tuning |
| 7 | `midds-cli bench fees` + `bench throughput` | Outillage opérateur |
| 8 | Workflow snapshot doc + CI release artifact | Industrialisation |
| 9 | Audit final cas pathologiques | Filet de sécurité |

---

## 14. Conventions

- Tous les rapports de tests : `target/test-reports/<scenario>.{json,md}`,
  format stable, parsable par CI.
- `proptest_cases` configurable via env var, jamais hardcodé.
- RNG : `SmallRng` seedé, jamais `thread_rng()` dans les tests
  reproductibles.
- Multi-comptes en E2E : dérivation `//Alice//<N>`, jamais de clé hardcodée
  hors `Alice`/`Bob`.
- Datasets fixtures : aucune donnée réelle nominative. Codes industrie
  uniquement (ISWC/IPI/ISNI synthétiques mais checksum-correct).

---

## 15. Tests cross-crate (hors pallet)

Les Couches 1–5 couvrent le flux on-chain. Mais le SDK héberge plusieurs
crates dont chacune mérite sa propre couverture. Récapitulatif par crate.

### 15.1 `midds-traits` (no_std, pure)

- Tests unitaires sur chaque `validate_*_format` (charset, longueur, structure).
- Cas négatifs explicites pour chaque branche de `MiddsFormatError`.
- Pas de proptest dédié : surface trop petite pour le ROI.
- Localisation : `crates/midds-traits/src/identifier/tests.rs` (déjà partiel).

### 15.2 `midds-types` (no_std)

- Roundtrip SCALE encode/decode pour tout MIDDS issu de `arb_musical_work()`.
- Invariant : `encoded_size(midds) <= <Midds as MaxEncodedLen>::max_encoded_len()`.
- Roundtrip serde JSON sous `--features serde` (sérialisé puis désérialisé,
  bit-identique).
- Localisation : `crates/midds-types/tests/encoding.rs`.

### 15.3 `midds-validate` (std, offline)

Couverture critique car c'est l'API publique pour les outils amont
(éditeurs de catalog, importers).

- Pour chaque parseur tolérant : `valid` / `canonicalisable` / `invalid`.
- Vérificateurs de checksum : warnings émis, jamais bloquants.
- `MusicalWorkBuilder` : tout build réussi produit un payload qui passe
  aussi `<MusicalWork as Midds>::validate_format`.
- **Invariant clé** : on-chain ⊆ off-chain. Tout payload qui passe on-chain
  passe off-chain ; l'inverse n'est pas vrai (off-chain accepte des
  warnings).
- Localisation : `crates/midds-validate/src/musical_work/tests.rs`.

### 15.4 `midds-rpc` (std)

- Test d'intégration avec un `impl MiddsRuntimeApi for TestApi` minimal
  (stub en mémoire, pas de node).
- Assert sur le shape JSON : `lookup_by_identifier` sur un ID inexistant
  renvoie `null`, pas une erreur.
- Localisation : `crates/midds-rpc/tests/rpc.rs`.

### 15.5 `midds-client` (std)

- Couvert essentiellement par la Couche 5 (subxt::dynamic exécuté contre
  un node `--dev`).
- Un test unitaire utile : `codec_bridge::EncodedCall` roundtrip avec
  `parity_scale_codec::Encode` (sécurité contre une régression du bridge).
- Pas de mock node, pas de subxt mocké — toute la valeur du choix dynamic
  vient de l'exécution réelle, mocker la dégrade.

### 15.6 `midds-codegen` (std bin)

Surface ici (CLI smoke) :

- Smoke CLI : `--help` / `--version` / args manquants / chemin metadata
  inexistant. Garde-fou sur la couche wrapper (clap, `is_url`,
  `from_file_blocking`) — le seul code que `midds-codegen` possède
  réellement.
- Localisation : `crates/midds-codegen/tests/cli_smoke.rs`.

Snapshot codegen complet (déféré côté Allfeat) :

- L'idée originelle (génération réussie depuis une metadata SCALE committée
  + `cargo check` sur le binding produit) demande la metadata réelle de
  `melodie-runtime`. La committer ici contredirait le découplage SDK /
  runtime verrouillé par `CLAUDE.md` ("the runtime side is in
  `../Allfeat`") et la metadata dérive à chaque release runtime, donc le
  rythme de refresh appartient au runtime, pas au SDK.
- Localisation cible : `Allfeat/runtime/melodie/tests/codegen_snapshot.rs`
  (ou un step CI qui regénère + diff les bindings depuis un node
  `melodie-dev`).

### 15.7 `midds-runtime-api` (no_std)

Pas de test direct — declarations only. Implicitement couvert par la
Couche 4 (impl côté runtime) et la Couche 5 (consommé via RPC).

---

## 16. Tests de migration / versioning

La stratégie verrouille des enums top-level versionnés
(`enum MusicalWork { V1(...), V2(...) }`). L'ajout d'une variante est
additif. Mais il faut prouver mécaniquement que c'est non-breakable.

### 16.1 Invariants à prouver à chaque ajout de variante

| Invariant | Description |
|---|---|
| Wire stability | Un `MusicalWorkV1` encodé en SCALE reste decodable comme `MusicalWork::V1` après ajout de V2 |
| Storage stability | `Items<MiddsId, MusicalWork>` ne nécessite aucune migration sur ajout pur de variante |
| Identifier stability | `identifier()` retourne la même valeur pour un V1, indépendamment des variantes ajoutées |

### 16.2 Mécanique

Fichier : `crates/midds-types/tests/version_stability.rs`.

```rust
#[test]
fn v1_payload_stays_decodable() {
    let v1_bytes = include_bytes!("fixtures/musical_work_v1.scale");
    let decoded = MusicalWork::decode(&mut &v1_bytes[..]).unwrap();
    assert!(matches!(decoded, MusicalWork::V1(_)));
}
```

`musical_work_v1.scale` est commit comme **fixture immuable**. Toute
modification accidentelle du wire format V1 fait échouer le test — c'est
exactement le filet recherché.

### 16.3 OnRuntimeUpgrade

Quand une vraie migration deviendra nécessaire (changement structurel
forcé par contrainte business), elle vivra dans
`Allfeat/runtime/melodie/migrations/` avec son test dans
`Allfeat/runtime/melodie/tests/migrations.rs`. Pas dans le SDK.

**Règle** : le SDK garantit le wire format des MIDDS ; le runtime gère
ses propres migrations de storage. Frontière nette.

---

## 17. Artefacts & commit policy

| Artefact | Localisation | Commit ? |
|---|---|---|
| Rapports `target/test-reports/*.{json,md}` | local + CI artifact | **non** (gitignored) |
| `proptest-regressions/` | repo | **oui** (pratique standard proptest) |
| `tests/fixtures/storage_root_*.txt` | repo | **oui** (anti-régression Couche 3) |
| Datasets `crates/midds-fixtures/data/*.json` | repo | **oui** (déterminisme) |
| `crates/midds-types/tests/fixtures/musical_work_v1.scale` | repo | **oui** (wire stability) |
| `crates/midds-codegen/tests/fixtures/metadata.scale` | repo | **oui** (snapshot codegen) |
| `seeded-state.json` (chain dev seedée) | release asset GitHub | **non** (taille) |
| `seed_report.json` | local | **non** (régénérable depuis seed) |

**Règle générale** : si c'est régénérable de manière 100 % déterministe
depuis le code + un seed, on ne commit pas. Sinon on commit. Le `.gitignore`
doit refléter cette règle, pas la contredire.

---

## 18. Items ouverts

- Choix précis de la lib mass injection (subxt direct vs `txwrapper`).
- Format exact du `seed_report.json` (à figer après PR de la Couche 5).
- Politique de rotation des snapshots seeded sur les releases (combien on
  en garde, combien on en publie).
- Intégration `criterion` ou `divan` pour les vrais benchmarks de perf
  utilisateur (Couche 5) — à trancher quand on attaquera l'étape 7.
