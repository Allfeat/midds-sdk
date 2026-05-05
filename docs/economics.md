# MIDDS SDK — Modèle économique

> Document de référence pour l'économie du `pallet-midds` : bond, pricing
> dynamique, fenêtre de finalisation, destination des fonds. Pendant de
> `docs/plan.md` (architecture) et `docs/testing.md` (tests).
> Cible : un modèle sain, sybil-résistant par construction, aligné avec la
> tokenomics Allfeat (supply cappée à 1 B AFT, jamais d'émission au-delà).

---

## 1. Contexte et objectifs

Le pallet-midds doit servir un produit qui a **trois caractéristiques
structurantes** :

1. **Permissionless dès J0** — pas de whitelist, n'importe qui peut déposer.
2. **Pas de filtre qualité on-chain pendant plusieurs mois** — le mécanisme
   PoM (Proof of Metadata) arrivera plus tard ; en standalone, le format est
   le seul check.
3. **Promesse marketing** : *"minimal cost per million metadata records"*
   (whitepaper §1.1) — l'ingestion à grande échelle doit rester abordable.

Trois contraintes économiques additionnelles cadrent la conception :

- **Supply cappée à 1 000 000 000 AFT, jamais réémise.** Aucun mécanisme ne
  doit détruire de supply de manière irréversible (pas de burn).
- **Web3 = pas d'anti-spam per-account** — un attaquant crée 10 K wallets
  pour < 1 K AFT (juste l'ED). Toute défense indexée sur l'identité du
  compte est défaite par sybil.
- **Doublons d'identifier autorisés** — plusieurs parties peuvent déposer
  leur version d'un même ISWC. Seuls les doublons exacts (même payload)
  sont rejetés. Le squat de namespace n'existe pas comme menace.

→ La conception économique doit donc fonctionner **au niveau communautaire**
(jamais per-account), être **honnête sur ce qu'elle ne fait pas** (pas
d'anti-sybil absolu sans curation), et **préserver la supply** en
recyclant tout AFT collecté plutôt qu'en le brûlant.

---

## 2. Décisions verrouillées

| # | Sujet | Décision |
|---|---|---|
| 1 | Modèle de paiement | **Bond avec fenêtre refundable de 7 jours**, puis transfert à la Foundation Treasury |
| 2 | Préservation supply | **Pas de burn**. Bond finalisé → Foundation Treasury (recyclage via gouvernance) |
| 3 | Pricing dynamique | **Deux multiplicateurs superposés** : `M_fast` (par bloc, anti-DoS) + `M_slow` (fenêtre 7j glissante, anti-flood) |
| 4 | Cycle de finalisation | 7 jours après deposit, bond auto-converti via `on_initialize` borné, fallback `finalize(id)` permissionless |
| 5 | Identifier non unique | `IdentifierClaims: DoubleMap<Identifier, MiddsId, ()>` — multi-claim natif |
| 6 | Anti-doublon exact | `PayloadHashes: Map<H256, MiddsId>` — hash SCALE-encodé du payload |
| 7 | Distinction sudo remove | **Deux extrinsics séparés** : `force_remove_refund` (typo) et `force_remove_slash` (abus) |
| 8 | Fenêtre 7j | Alignée sur le cycle hebdomadaire de release musicale (vendredi IFPI Global Release Day) |
| 9 | Cible débit V1 | `SlowTargetPerWindow = 200 000` deposits/semaine (~30 K/jour). Calibrage prudent, ajustable par gouvernance |
| 10 | Tx fees runtime | `TransactionByteFee` ÷10 vs actuel (1 µAFT/B), `WeightFeeFactor` inchangé |
| 11 | Migration | **Aucune** — pallet pas en mainnet, reset testnet melodie au déploiement |
| 12 | Anti-sybil | **Non garanti au protocole**. Filtre qualité = off-chain (indexers, front-end) en attendant PoM |

Les décisions précédentes du `plan.md` § 2 (#4 bond simple, #5 formule statique, #10 unicité par identifier) sont **superseded** par celles ci-dessus.

---

## 3. Principe en une phrase

> **Tout le monde paye un bond au deposit. Le bond est intégralement
> remboursable pendant 7 jours, puis transféré à la Foundation Treasury à la
> finalisation. Le montant du bond est multiplié par deux multiplicateurs
> dynamiques qui poussent toute la communauté à étaler ses deposits dans le
> temps plutôt qu'à les bursté.**

Trois propriétés émergent :

1. **Refund réel** (pas vaporware) — le user a 7 jours pour annuler une
   erreur ou se rétracter, avec remboursement intégral.
2. **Pas de supply lock permanent** — au-delà de 7 jours, le bond sort du
   compte du user vers la Treasury, qui peut le redistribuer (rewards
   top-up, grants, opérations).
3. **Auto-régulation** — le pricing dynamique aligne mécaniquement
   honnêtes et attaquants sur le même incentive : **étaler dans le temps**.
   Un acteur qui burst paye plus, qu'il vienne d'un wallet ou de mille.

---

## 4. Cycle de vie d'un MIDDS

```
t0       deposit(item)
         → HOLD bond = base × M_fast(t0) × M_slow(t0)
         → MIDDS publié, lisible immédiatement
         → push (t0 + 7j, MiddsId) dans PendingFinalization

[t0, t0 + 7j]  — fenêtre refundable
         remove_own(id)  → refund 100 % du bond, MIDDS supprimé
         update(id, payload) → modifie le payload, fenêtre inchangée (ancrage sur t0)

t0 + 7j  — finalisation automatique
         on_initialize consomme PendingFinalization[t0 + 7j]
         → release du HOLD vers Treasury
         → DepositInfo.finalized = true
         → MIDDS devient permanent

> t0 + 7j  — gestion sudo uniquement
         force_remove_refund(id) → retiré, dépossesseur indemnisé (typo)
         force_remove_slash(id)  → retiré, pas de refund (abus)
         force_edit(id, payload) → modifié sans toucher l'économie
```

### 4.1 Ancrage de la fenêtre

La fenêtre est ancrée sur `deposited_at` (timestamp original du deposit).
**`update` ne reset pas la fenêtre.** Sinon un user pourrait update tous
les 6 jours pour garder son bond éligible au refund à perpétuité.

### 4.2 Trigger de finalisation

Architecture **eager + fallback** :

- **`on_initialize(n)`** : consomme jusqu'à `MaxFinalizationsPerBlock`
  entries de `PendingFinalization::iter_prefix(n)`. Garantit la
  finalisation auto dans le cas nominal.
- **`finalize(id)` permissionless** : fallback si la queue déborde
  (catch-up manuel, ou via crank tiers / oracle).

Une queue indexée par bloc permet une lecture O(1) des items à finaliser
au bloc courant. Le poids `on_initialize` est borné par
`MaxFinalizationsPerBlock × W::finalize_one()`.

---

## 5. Modèle de pricing dynamique

Le bond effectif au moment du deposit est :

```
final_bond(t) = MiddsBondBase
              + MiddsBondPerByte × encoded_size(item)
final_bond(t) ×= M_fast(t)
final_bond(t) ×= M_slow(t)
```

avec deux multiplicateurs aux dynamiques différentes :

### 5.1 `M_fast` — anti-DoS instantané

| Paramètre | Valeur |
|---|---|
| Cible | 100 deposits par bloc (≈ 17/s à 6s/bloc) |
| Pas d'ajustement | ±12.5 % par bloc |
| Floor | 0.1× |
| Ceiling | 20× |
| Réactivité | Multiplicateur double / divise par 2 en ~6 blocs (~36 s) |

**Effet** : un burst de 1000 deposits dans un bloc fait spiker `M_fast` à
~5–10× ; le bloc suivant sans charge le ramène vers 1× en quelques
minutes. Empêche le block stuffing sans pénaliser le régime nominal.

### 5.2 `M_slow` — anti-flood et incentive d'étalement

| Paramètre | Valeur |
|---|---|
| Fenêtre | 7 jours glissants (rolling window) |
| Cible | 200 000 deposits / semaine (~30 K/jour) |
| Pas d'ajustement | ±5 % par jour |
| Floor | 0.1× |
| Ceiling | 50× |
| Réactivité | Atteint 5× en ~7 jours sous flood soutenu, redescend en ~7 jours après |

**Effet** : la fenêtre voit toute la communauté, peu importe le nombre de
wallets utilisés. Un attaquant patient avec 1000 wallets et un user
honnête avec 1 wallet sont traités exactement pareil.

### 5.3 Profils de coût attendus

Bond effectif pour un MusicalWork typique (~150 B encodés) :

| Profil | M_fast | M_slow | Bond payé |
|---|---|---|---|
| Régime nominal | 1× | 1× | ~500 mAFT (~$0.01) |
| Artiste indé, 5 deposits étalés | 1× | 1× | ~500 mAFT |
| Label, 10K records sur 1 mois | 1× moy | ~1× | ~500 mAFT moy |
| Mass ingest CMO, 1M sur 60j | 1× | ~1.5× | ~750 mAFT moy |
| Burst 1 bloc plein | ~10× | 1× | ~5 AFT (sur ce bloc) |
| Flood patient 1M en 7j | 1× moy | ~10× | ~5 AFT pendant 7j |
| Flood très patient 1M en 60j | 1× | ~1.5× | ~750 mAFT (≈ honnête) |

Le dernier cas est intéressant : un attaquant **vraiment patient**
finit par ressembler à un user légitime, économiquement parlant. À ce
moment-là, la défense bascule sur le filtrage qualité (off-chain, ou
PoM future). C'est le bon transfert de problème.

### 5.4 Pourquoi 7 jours

Aligné sur le rythme métier de l'industrie musicale :

- **Vendredi = Global Release Day** (IFPI standard depuis 2015)
- Les labels préparent leurs releases la semaine, déposent en avance
- Une fenêtre 7j capture exactement le cycle de release hebdomadaire
- Le mécanisme **éduque** la communauté à enregistrer ses MIDDS *avant*
  le release day — comportement opérationnellement sain

### 5.5 Refund et multiplicateur

Le refund pendant la fenêtre (`remove_own`) rend la **base de chaque
couche** à son payeur respectif. La prime de multiplicateur (au-delà du
base bond) banked dans chaque couche est transférée vers la Treasury à la
finalisation, même si l'item est ensuite refundé.

Justification : empêcher l'arbitrage *deposit-burst-cher → remove → re-deposit-creux*. Sans cette règle, un user attendrait que `M_slow` baisse et resoumettrait à moindre coût en récupérant l'intégralité de son bond initial. Avec cette règle, la prime payée pour bursté reste perdue même après refund — ce qui aligne l'incentive sur "ne pas burst en premier lieu".

Implémentation : `remove_own` rend `sponsor.base` au sponsor + `owner.base` au depositor (le cas échéant). Le delta `(M_fast × M_slow − 1) × base` capté par chaque couche est transféré à la Treasury immédiatement, **par couche** — ainsi un sponsor et un depositor qui ont posté chacun leur part ne se partagent pas mutuellement la pénalité de burst.

### 5.6 Bond stratifié et escape hatch web3

Un MIDDS porte deux couches de bond, indépendantes :

- **`sponsor_layer`** (toujours présente) — la base + prime payée au
  `deposit` initial. Sur un self-deposit, le payeur est le depositor ; sur
  un `deposit_on_behalf`, c'est l'opérateur sponsor.
- **`owner_layer`** (`Option`) — apparaît uniquement quand le **depositor
  d'un record sponsorisé** étend la donnée via `update` plain. À cette
  occasion, le depositor paie le `Δbase × M_courant` sur ses propres fonds
  ; sa couche bank sa propre prime.

Cette stratification implémente l'**escape hatch web3** : un artiste
onboardé via une plateforme SaaS (sponsor) peut à tout moment reprendre la
main sur sa donnée sans dépendre de la balance du sponsor. La règle :

- `update` (caller = depositor) sur record sponsorisé → la couche cible
  est `owner_layer` (créée à la première extension solo, étendue ensuite
  sans nouvelle prime).
- `update_on_behalf` (caller = sponsor) → la couche cible est
  `sponsor_layer`. Compatible avec l'existence d'une `owner_layer` —
  sponsor et owner co-existent (Q2.b).
- Sur un shrink où le `Δbase` à libérer dépasse la couche du caller, la
  réduction overflow LIFO sur l'autre couche, qui se voit alors refundée.
- `remove_own_on_behalf` ferme la boucle meta-tx : caller libre (relayer
  tiers, sponsor, ou peu importe), signature owner suffit pour autoriser
  la rétractation. Un owner qui n'a *jamais* tenu d'AFT peut donc
  reprendre tous ses fonds sans se soucier des fees on-chain.

Sur `remove_own` / `remove_own_on_behalf` / `force_remove_refund`, chaque
couche se réconcilie indépendamment avec son `payer` ; sur `finalize` /
`force_remove_slash`, chaque couche transfère son montant complet à la
Treasury. La séparation
financière est intégrale : aucun fonds d'une couche ne paie pour la
pénalité de l'autre.

Préservation de prime per-layer : sur `update`, la base d'une couche
existante évolue par la formule `new_amount = new_base + max(0,
old_amount − old_base)`. La prime banked à la création est sticky et ne
se re-prix pas au multiplicateur courant (anti-arbitrage §5.5). Une couche
créée sous `M < 1` (cas dégénéré, multiplicateurs au plancher) est
sous-payée à la création — son extension ultérieure rebase la couche au
nouveau total base, ce qui peut faire grossir le hold ; ce comportement
miroite la pré-stratification.

---

## 6. Constants runtime

```rust
// runtime/melodie/src/pallets/midds.rs

parameter_types! {
    // Bond nominal (base) — calibré pour anti-spam communautaire
    // ~0.5 AFT par MIDDS en régime nominal (~$0.01 @ $0.02/AFT)
    pub const MiddsBondBase: Balance = 500 * MILLIAFT;
    pub const MiddsBondPerByte: Balance = 10 * MICROAFT;

    // Fenêtre refundable
    pub const MiddsCommitmentWindow: BlockNumber = 7 * DAYS;
    pub const MiddsMaxFinalizationsPerBlock: u32 = 100;

    // Multiplicateur fast
    pub const FastTargetPerBlock: u32 = 100;
    pub const FastAdjustmentRate: Perbill = Perbill::from_parts(125_000_000);
    pub const FastMultiplierMin: FixedU128 = FixedU128::from_rational(1, 10);
    pub const FastMultiplierMax: FixedU128 = FixedU128::from_u32(20);

    // Multiplicateur slow
    pub const SlowWindow: BlockNumber = 7 * DAYS;
    pub const SlowTargetPerWindow: u32 = 200_000;
    pub const SlowAdjustmentRate: Perbill = Perbill::from_percent(5);
    pub const SlowMultiplierMin: FixedU128 = FixedU128::from_rational(1, 10);
    pub const SlowMultiplierMax: FixedU128 = FixedU128::from_u32(50);

    // Destination du bond finalisé
    pub MiddsTreasuryAccount: AccountId =
        PalletId(*b"af/midds").into_account_truncating();
}
```

Tx fees standard runtime (à ajuster dans `transaction_payment.rs`) :

```rust
parameter_types! {
    pub const TransactionByteFee: Balance = MICROAFT;          // ÷10 vs actuel
    pub const OperationalFeeMultiplier: u8 = 5;                // inchangé
    pub const WeightFeeFactor: Balance = 10 * MILLIAFT;        // inchangé
}
```

---

## 7. Surface d'extrinsics

| Extrinsic | Origin | Effet | Refund | Sudo |
|---|---|---|---|---|
| `deposit(item)` | `ProviderOrigin` (signed) | HOLD bond sur `sponsor_layer` (= depositor), insert MIDDS, queue finalisation | — | — |
| `deposit_on_behalf(owner, item, nonce, sig)` | `ProviderOrigin` (operator) + signature owner | HOLD bond sur `sponsor_layer` (= operator), attribution = owner | — | — |
| `update(id, item)` | depositor, ≤ 7j | Sur self-deposit : étend `sponsor_layer`. Sur record sponsorisé : étend ou crée `owner_layer` (escape hatch web3) | — | — |
| `update_on_behalf(id, item, nonce, sig)` | original sponsor + signature owner, ≤ 7j | Étend `sponsor_layer`. Co-existe avec une `owner_layer` déjà créée | — | — |
| `remove_own(id)` | depositor, ≤ 7j | Refund base de chaque couche à son payeur, primes vers Treasury | ✅ partiel | — |
| `remove_own_on_behalf(id, nonce, sig)` | n'importe quel `ProviderOrigin` (relayer) + signature owner, ≤ 7j | Idem `remove_own` mais pilotable par tout relayer ; closes meta-tx loop pour owner sans AFT | ✅ partiel | — |
| `finalize(id)` | n'importe qui, > 7j | Release HOLD de chaque couche vers Treasury | — | — |
| `force_edit(id, item)` | sudo | Update via `sponsor_layer` (governance ne crée jamais d'`owner_layer`) | — | ✅ |
| `force_remove_refund(id)` | sudo, ≤ 7j | Cleanup + refund total à chaque payeur (typo bonne foi) | ✅ total | ✅ |
| `force_remove_slash(id)` | sudo | Cleanup, holds de chaque couche → Treasury (abus signalé) | ❌ | ✅ |
| `force_remove_many(Vec<id>)` | sudo | Batch cleanup, weight linéaire | dépend du flag | ✅ |

### 7.1 Hooks runtime

```rust
fn on_initialize(n: BlockNumberFor<T>) -> Weight {
    let mut weight = T::DbWeight::get().reads(1);

    // Reset compteur fast
    let fast_count = DepositsThisBlock::<T, I>::take();
    Self::adjust_fast_multiplier(fast_count);
    weight += /* ... */;

    // Slow : rotation bucket à minuit
    if n % T::BlocksPerDay::get() == 0 {
        Self::rotate_slow_bucket();
        Self::adjust_slow_multiplier();
        weight += /* ... */;
    }

    // Finalisations dues
    let to_finalize = PendingFinalization::<T, I>::iter_prefix(n)
        .take(T::MaxFinalizationsPerBlock::get() as usize)
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    for id in to_finalize {
        Self::do_finalize(id);
        weight += T::WeightInfo::finalize_one();
    }

    weight
}
```

---

## 8. Layout de stockage

```rust
// Inchangé
Items: StorageMap<_, Blake2_128Concat, MiddsId, T::Midds>;
NextMiddsId: StorageValue<_, MiddsId, ValueQuery>;

// MODIFIÉ : multi-claim sur identifier (remplace IdentifierIndex)
IdentifierClaims:
    StorageDoubleMap<_, Blake2_128Concat, Identifier, Twox64Concat, MiddsId, ()>;

// NOUVEAU : anti-doublon exact
PayloadHashes: StorageMap<_, Identity, T::Hash, MiddsId>;

// MODIFIÉ : payload_hash + finalized
DepositInfo: StorageMap<_, Blake2_128Concat, MiddsId, Deposit<T, I>>;
pub struct Deposit<T, I> {
    pub depositor: T::AccountId,
    pub deposited_at: BlockNumberFor<T>,
    pub amount: BalanceOf<T, I>,
    pub payload_hash: T::Hash,
    pub finalized: bool,
}

// NOUVEAU : queue de finalisation indexée par bloc d'échéance
PendingFinalization:
    StorageDoubleMap<_, Twox64Concat, BlockNumberFor<T>, Identity, MiddsId, ()>;

// NOUVEAU : multiplicateurs dynamiques
DepositsThisBlock: StorageValue<_, u32, ValueQuery>;
FastMultiplier: StorageValue<_, FixedU128, ValueQuery>;

SlowWindowBuckets: StorageValue<_, BoundedVec<u32, ConstU32<7>>, ValueQuery>;
SlowWindowHead: StorageValue<_, u8, ValueQuery>;
SlowMultiplier: StorageValue<_, FixedU128, ValueQuery>;
```

### 8.1 Coût storage par deposit

- 1 write `Items`
- 1 write `IdentifierClaims`
- 1 write `PayloadHashes`
- 1 write `DepositInfo`
- 1 write `PendingFinalization`
- 1 write `NextMiddsId`
- 1 read+write `DepositsThisBlock`
- 1 read+write bucket `SlowWindowBuckets`
- 1 hold sur le balance (1 read+write `Holds`)

→ ~9 writes par deposit. Pris en compte dans le benchmark.

---

## 9. Destination du bond finalisé : Foundation Treasury

### 9.1 Pourquoi pas burn

La supply AFT est **cappée à 1 B et jamais réémise** (whitepaper §2.1).
Brûler du bond réduirait définitivement la supply en circulation, ce qui
n'est pas souhaité — la supply doit pouvoir être recyclée vers les usages
productifs du réseau (rewards, grants, opérations).

### 9.2 Pourquoi Treasury et pas validateurs

Les validateurs sont déjà rémunérés par les **tx fees** (`DealWithFees` dans
`transaction_payment.rs` envoie 100 % au block author). Le bond MIDDS est
un revenu protocole distinct — il finance la maintenance du **registre
lui-même**, pas du consensus.

### 9.3 Modèle de gouvernance Treasury

Le compte Treasury (`MiddsTreasuryAccount`) reçoit tous les bonds finalisés
et les deltas non-refundés. Sa gestion est gouvernée :

- **Top-up du pool community rewards** quand le 260 M initial s'épuise (cf.
  whitepaper §5.3 *buybacks and redistribution* : ce mécanisme implémente
  exactement la promesse de réinjection)
- **Grants** aux contributeurs de l'écosystème (data quality, indexers,
  outils, intégrations)
- **Opérations Foundation** (audits, infrastructure, support)
- **Buybacks** ponctuels si la situation marché le justifie (rachat AFT
  hors marché → réinjection en rewards)

L'allocation précise est hors scope de ce document — pilotée par la
Foundation via un pallet de gouvernance ou multisig dans un premier temps.

---

## 10. Ce que le modèle ne fait PAS

À documenter explicitement pour calibrer les attentes (et les promesses
publiques) :

### 10.1 Pas de défense anti-sybil au protocole

Pendant la phase standalone (avant PoM), un acteur patient avec budget peut
déposer du contenu de qualité douteuse au prix plancher en étalant sur des
semaines. Le filtre qualité est **strictement off-chain** :

- Indexers de référence (Allfeat, partenaires)
- Front-ends qui n'affichent que les MIDDS conformes à des règles publiées
- Heuristiques de réputation depositor (âge du compte, historique)
- Cleanup ponctuel via `force_remove_slash` sur des patterns identifiés

Ce n'est pas un bug — c'est la **conséquence assumée** du choix
permissionless sans curation. PoM apportera la curation on-chain à terme.

### 10.2 Pas de récupération de bond après 7 jours

Intentionnel. La fenêtre courte force un commitment rapide ; après 7 jours,
le record est *permanent* et le bond appartient à la Treasury.
`force_remove_refund` existe pour les cas exceptionnels où la Foundation
veut indemniser une erreur de bonne foi, mais c'est une opération de
gouvernance, pas un droit du depositor.

### 10.3 Pas de refund automatique sur sudo après finalisation

Si un MIDDS finalisé est `force_remove`'d ensuite (abus tardif détecté), le
bond est déjà à la Treasury — aucune restitution n'est faite. Sudo nettoie
juste le storage. Le coût de l'abus a été payé via le bond initial ; la
sanction additionnelle est la perte du record on-chain.

### 10.4 Pas de partage avec les validateurs

Les bonds MIDDS ne vont **jamais** au block author. C'est une séparation
nette : tx fees → consensus, bonds → produit. Évite les incitations
perverses (validateur qui force-include du spam pour son propre bond).

### 10.5 Pas de réservation prioritaire (Coretime-style)

Pas de tier subscription / credits dans la V1. Le marché spot
EIP-1559-style avec deux multiplicateurs est suffisant pour la phase de
bootstrap. Un tier subscription pourra être ajouté post-launch si une
demande institutionnelle s'observe — voir §12.2.

---

## 11. Alignement avec le whitepaper tokenomics

| Section whitepaper | Comment le modèle s'y aligne |
|---|---|
| §1.1 *minimal cost per million* | Bond ~500 mAFT/record en régime nominal = ~$10K/million. Reste compatible avec mass-ingest institutionnel. |
| §2.1 *supply 1 B, jamais émis au-delà* | Bond va à Treasury (recyclage), jamais brûlé. Supply préservée. |
| §2.2 *26 % community rewards (260 M)* | La Treasury MIDDS peut top-up ce pool quand il s'épuisera (~10-14 ans). |
| §4 *vesting / sell pressure* | Le bond crée un *lock circulation pendant 7j minimum* — petite mais réelle réduction de la pression de vente immédiate. |
| §5.3 *buyback and redistribution* | La Treasury MIDDS est l'instrument naturel de cette stratégie. Les bonds finalisés financent les buybacks/redistribution sans nécessiter d'inflation. |

### 11.1 Estimation revenue Treasury à différents niveaux d'adoption

À bond moyen 500 mAFT (régime nominal) :

| Scenario | Deposits/an | Revenue Treasury/an | % supply |
|---|---|---|---|
| Bootstrap an 1 | 100 K | 50 K AFT | 0.005 % |
| Adoption modeste | 1 M | 500 K AFT | 0.05 % |
| Adoption forte | 10 M | 5 M AFT | 0.5 % |
| Saturation cible V1 | 10 M (200 K/semaine) | 5 M AFT | 0.5 % |
| Adoption massive | 100 M | 50 M AFT | 5 % |

À 100 M deposits/an avec un multiplicateur moyen ~2× sous charge, on
parle de ~100 M AFT/an de revenue Treasury — équivalent à ~38 % du pool
community rewards initial chaque année. Largement suffisant pour
pérenniser les incentives au-delà de l'épuisement de l'allocation
initiale, et financer significativement la R&D / opérations Foundation.

---

## 12. UX et communication user-facing

### 12.1 Pitch externe

> *"Le réseau Allfeat ingère jusqu'à ~200 000 MIDDS par semaine au prix
> plancher (~500 mAFT, ~$0.01). Au-delà, le prix monte progressivement
> pour réguler la charge — conseil : enregistrez vos métadonnées en début
> de semaine pour le tarif optimal, comme pour préparer votre setlist
> avant le concert. Vous avez 7 jours après le deposit pour corriger ou
> annuler avec remboursement intégral."*

Volontairement non-technique. Aucune mention de "EIP-1559" ou
"multiplicateur" dans la com publique.

### 12.2 RPC à exposer

```rust
// midds-runtime-api v2
fn current_deposit_price(payload_size: u32) -> Balance;
fn current_multipliers() -> (FixedU128, FixedU128);  // (fast, slow)
fn weekly_target() -> u32;
fn weekly_actual() -> u32;
```

Permet aux dashboards / wallets / front-ends d'afficher en live :
- "Tarif actuel : 520 mAFT pour ce MIDDS"
- "Charge réseau cette semaine : 87 % de la cible"
- Graphique 24h du multiplicateur

C'est ce qui rend le mécanisme **lisible** côté user. Sans cet RPC, le
prix dynamique semble arbitraire ; avec, il devient un signal de marché
compréhensible.

---

## 13. Évolutions futures (hors scope V1)

### 13.1 Tier subscription pour institutionnels

Quand une demande institutionnelle stable est observée (CMOs, agrégateurs
ingérant > 10 K MIDDS/mois sur plusieurs mois), ajouter un mécanisme de
pré-paiement de *credits* :

- Achat de N credits à un prix fixe (= prix moyen de la dernière époque)
- Chaque credit consomme un slot de deposit au prix plancher
- Remise dégressive par volume (ex: -10 % à partir de 10 K credits)
- Lock le prix pour le buyer, garantit du revenue pour la Treasury

À calibrer post-launch, sur données réelles d'usage.

### 13.2 Intégration PoM

Quand le pallet PoM existera :

- Les MIDDS *certifiés* par PoM peuvent éventuellement bénéficier d'une
  réduction sur leur bond (récompense la qualité)
- Le `force_remove_slash` peut être déclenché par un consensus PoM négatif
  plutôt que sudo
- Le pool reward PoM peut être partiellement financé par la Treasury MIDDS

L'interface entre les deux pallets sera spec'd dans `docs/plan-pom.md` (à
créer).

### 13.3 Multi-instance et coûts différenciés

Quand Recording et Release seront ajoutés (chacun comme nouvelle Instance
du pallet), les constantes économiques peuvent diverger :

- Recording probablement plus volumineux que MusicalWork → bond per-byte
  pèse plus
- Release encore plus complexe → peut justifier un base bond plus élevé
- Chaque Instance porte ses propres `parameter_types!`

L'architecture multi-instance le permet sans refacto.

### 13.4 Auto-calibrage du `MiddsBondBase`

Plutôt que de figer `MiddsBondBase` dans le runtime, le passer en
`StorageValue` ajustable selon des observations long-terme (mois,
trimestres). Permet de reculer la cible par gouvernance sans runtime
upgrade — utile si le ratio AFT/USD bouge significativement.

---

## 14. Roadmap d'implémentation

| Phase | Tâches | Effort estimé |
|---|---|---|
| **A. Décision** | Ce document mergé dans `docs/`, `plan.md` § 2 mis à jour pour pointer vers les nouvelles décisions | ✅ ce PR |
| **B. Storage refacto** | `IdentifierClaims`, `PayloadHashes`, `Deposit` struct étendue, `PendingFinalization`, multiplicateurs | 4 h |
| **C. Extrinsics core** | `deposit` (avec multiplicateurs + queue), `update`, `remove_own`, `finalize` | 4 h |
| **D. Extrinsics sudo** | `force_edit`, `force_remove_refund`, `force_remove_slash`, `force_remove_many` | 3 h |
| **E. Hooks runtime** | `on_initialize` (finalisations + ajustement multiplicateurs), helpers | 3 h |
| **F. Tests unitaires** | Cas multi-claim, refund, finalize, prime non-refundée, sudo split | 6 h |
| **G. Property tests** | Invariants storage, monotonie multiplicateurs, conservation supply | 5 h |
| **H. Mass injection** | Profils de charge réalistes (release patterns musique), recalibrage `storage_root_hash` | 3 h |
| **I. Benchmarks** | Re-bench de tous les extrinsics + `on_initialize` paramétré | 5 h |
| **J. Runtime API v2** | `current_deposit_price`, `current_multipliers`, `weekly_target/actual`, lookup multi-claim | 3 h |
| **K. Runtime melodie** | Câblage des nouvelles `parameter_types!`, MiddsTreasuryAccount, transaction_payment update | 2 h |
| **L. Doc & comments** | Mise à jour `plan.md` § 2, commentaires runtime, README crates | 2 h |
| **Total** | | **~40 h ≈ 1 semaine** |

---

## 15. Questions ouvertes (à trancher avant la phase B)

1. **Pourcentage de revenue tx fee redirigé vers Treasury** : actuellement
   100 % au block author. Faut-il en récupérer une fraction (ex: 20 %) pour
   doubler la base de revenue Treasury ? Hors scope direct mais à arbitrer
   en parallèle.

2. **Multi-instance : bond commun ou per-instance ?** Quand Recording arrive,
   doit-il partager `MiddsTreasuryAccount` ou avoir son propre compte ?
   Recommandation : partagé, simplicité.

3. **Métrique de "santé" exposée publiquement** : faut-il un événement on-chain
   `WeeklyMetricsSnapshot { deposits, avg_multiplier, treasury_balance }`
   chaque dimanche, pour faciliter le suivi par les indexers et dashboards ?
   Recommandation : oui, peu de poids et grand bénéfice transparence.

4. **Comportement `update` quand `M_slow` a changé** : si le user update et
   que le nouveau bond calculé est inférieur à l'ancien (multiplicateur
   redescendu), refund-t-on la différence ? Ou on garde le bond initial ?
   Recommandation : refund partiel pour cohérence avec le principe "tu
   payes le prix du marché courant" — mais à arbitrer.

5. **Affichage du tarif côté CLI** : `midds-cli deposit` doit-il faire un
   round-trip RPC pour montrer le prix avant confirmation ? Recommandation :
   oui, avec flag `--max-price` pour fail si le prix dépasse un seuil
   (UX similaire à un slippage limit).
