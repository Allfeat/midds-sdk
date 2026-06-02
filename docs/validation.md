# MIDDS SDK — Spécification des règles de validation V1

> Document de référence **canonique** pour la validation champ par champ de
> chaque type MIDDS. Les règles ci-dessous sont **figées pour V1** : un
> changement ici implique un bump de version payload (`V2`) ou, pour les
> bornes encodées dans le type, un changement de format wire / `MaxEncodedLen`.
>
> Origine : règles métier conçues dans l'ancien frontend `../midds`
> (schémas Zod + validations impératives), **réconciliées** avec les types du
> SDK actuel. Décision de réconciliation : **le SDK actuel fait foi** — ses
> bornes serrées et ses choix de modèle V1 (enums slimmés, `MusicalKey`
> structuré, `duration` en `u32`, `PartyId` sans `Both`, pas de
> `manufacturer_name`) sont délibérés et conservés. L'ancien front sert de
> source pour les **règles numériques / de cardinalité** (BPM, années,
> nombre de voix, cardinalité + unicité + non-auto-référence des œuvres
> sources Medley/Mashup/Adaptation) qui manquaient à la validation
> on-chain et sont ajoutées ici.

---

## 1. Principes

1. **On-chain = format uniquement.** `Midds::validate_format` vérifie
   charset / longueur / structure / bornes numériques / cardinalité. Il **ne
   vérifie jamais les checksums** (chiffre de contrôle ISWC/IPI/ISNI/GTIN) :
   les registres réels publient des codes au check digit faux. Les checksums
   sont *warning-only* et vivent dans `midds-validate`, jamais bloquants
   on-chain.
2. **Les longueurs max sont structurelles.** Tout champ borné est un
   `BoundedVec` / `MiddsString<N>` : dépasser la borne est impossible à
   construire/décoder. `validate_format` ne re-teste donc pas la longueur
   max ; il teste le **non-vide** des champs obligatoires et les **bornes
   numériques / cardinalité minimale**.
3. **Les enums sont fermés.** `Country`, `Language`, `Genre`,
   `RecordingVersion`, `ReleaseType/Format/Packaging/Status`, `CreatorRole`,
   `PitchClass`, `Mode` sont des enums SCALE à tag-byte : l'appartenance est
   garantie par le type, pas par `validate_format`.
4. **Identifiants ASCII only.** Charset imposé par les `validate_*_format`
   de `midds-traits`.
5. **Erreurs sans `String`.** Diagnostic via `MiddsFormatError`
   (`InvalidIdentifierStructure`, `InvalidCharset`, `OutOfBounds`,
   `EmptyMandatoryField`, `CrossFieldInconsistency` — utilisée pour l'unicité
   de la tracklist `Release` — + réservé `DateInconsistency`). **Aucune
   nouvelle variante** n'est introduite
   (ajouter une variante est *breaking* SCALE) : toute violation de borne
   numérique ou de cardinalité minimale réutilise `OutOfBounds` (« longueur
   sous le minimum ou au-dessus du maximum »).

Notation des tableaux : **N** = règle ajoutée à cette stabilisation (absente
de la validation on-chain auparavant) ; **S** = garanti structurellement par
le type (pas de code dans `validate_format`) ; **=** = déjà appliqué.

---

## 2. Identifiants (`midds-traits`)

| Id | Type / borne | Structure imposée on-chain | Origine ancien front |
|---|---|---|---|
| `Iswc` | `MiddsString<11>` | 11 octets : `T` + 10 chiffres ASCII | `^T\d{9}[0-9A-Z]$` — le SDK est **plus strict** (10ᵉ position = chiffre, pas alpha) ; conservé |
| `Isni` | `MiddsString<16>` | 15 chiffres + (chiffre \| `X`) | `^[0-9]{15}[0-9X]$` — identique |
| `Ipi` | `MiddsString<11>` | 9 à 11 chiffres ASCII | ancien : `1..=11` chiffres — le SDK impose **min 9** ; conservé |
| `Isrc` | `MiddsString<12>` | 2 alpha-maj + 3 alphanum-maj + 2 chiffres + 5 chiffres | `^[A-Z]{2}[A-Z0-9]{3}[0-9]{2}[0-9]{5}$` — identique (sans tirets) |
| `Upc` | `MiddsString<13>` | exactement 12 (UPC-A) **ou** 13 (EAN-13) chiffres | ancien : `1..=13` chiffres — le SDK impose **12 ou 13 exactement** ; conservé |
| `OffchainHash` | `MiddsString<64>` | non-vide (≥ 1 octet) ; opaque, CIDv1 par convention client | pas d'équivalent ancien |

Le chiffre de contrôle (ISWC/IPI mod-10 CISAC, ISNI ISO 7064, GTIN mod-10)
n'est **pas** vérifié on-chain. Vérificateurs warning-only :
`midds-validate::checksum`.

---

## 3. Types partagés (`midds-types::shared`)

| Type | Définition canonique | Validation |
|---|---|---|
| `Title` | `MiddsString<256>` (`TITLE_MAX_LEN = 256`) | non-vide quand obligatoire ; longueur **S** |
| `PartyId` | `enum { Ipi(Ipi) \| Isni(Isni) \| Both { ipi: Ipi, isni: Isni } }` — au moins l'un des deux identifiants | structure de chaque identifiant présent (les deux pour `Both`). Le variant `Both` a été **réintroduit** (cf. §7 — un même intervenant peut porter IPI et ISNI simultanément ; représentation native plus fidèle que deux entrées dupliquées) |
| `MusicalKey` | `{ pitch: PitchClass(17), mode: Mode(2) }` = 34 combinaisons | appartenance **S**. `PitchClass` couvre les 12 positions chromatiques avec les orthographes dièse et bémol les plus courantes (`D♭`/`E♭`/`G♭`/`A♭`/`B♭` en plus de `C♯`/`D♯`/`F♯`/`G♯`/`A♯`) ; les enharmoniques théoriques rares (`B♯`,`E♯`,`C♭`,`F♭`) restent hors V1 |
| `WorkRef` | `enum { Midds(u64) \| Iswc(Iswc) }` | si `Iswc` ⇒ structure ISWC |
| `RecordingRef` | `enum { Midds(u64) \| Isrc(Isrc) }` | si `Isrc` ⇒ structure ISRC |
| `Country` | enum fermé ISO 3166-1 alpha-2 (complet, JSON majuscule) | appartenance **S** — superset des 249 codes de l'ancien front |
| `Language` | enum fermé ISO 639-1 alpha-2 (complet, JSON minuscule) | appartenance **S** — superset des 22 langues de l'ancien front |

`CreatorRole` : `Author | Composer | Arranger | Adapter | Publisher` (5,
identique à l'ancien front).

---

## 4. `MusicalWork` V1

| Champ | Type | Req. | Borne canonique | Règle on-chain | |
|---|---|---|---|---|---|
| `iswc` | `Iswc` | oui | 11 | structure ISWC | = |
| `title` | `Title` | oui | ≤ 256 | non-vide | = |
| `creation_year` | `Option<u16>` | non | — | si `Some` ⇒ **`1..=2999`** | **N** |
| `instrumental` | `bool` | oui | — | aucune (défaut `false`) | = |
| `language` | `Option<Language>` | non | — | appartenance | S |
| `explicit_lyrics` | `bool` | oui | — | aucune (défaut `false`) | **N** |
| `bpm` | `Option<u16>` | non | — | si `Some` ⇒ **`20..=300`** | **N** |
| `key` | `Option<MusicalKey>` | non | — | appartenance | S |
| `work_type` | `WorkType` | oui | — | voir ci-dessous | |
| `samples` | `BoundedVec<WorkRef, 64>` (`SAMPLES_MAX`) | non | ≤ 64 | chaque réf valide (`WorkRef`) ; **réfs distinctes** ; **non-auto-référence** (variant ISWC ≠ `iswc` du work) | **N** |
| `creators` | `Creators` | oui | ≤ 32 (`CREATORS_MAX`) | **non-vide** ; chaque entrée : voir `Creator` ci-dessous | = |
| `classical_info` | `Option<ClassicalInfo>` | non | — | voir ci-dessous | |
| `offchain_extension` | `Option<OffchainHash>` | non | ≤ 64 | si `Some` ⇒ non-vide | = |

**`WorkType`** (`Original | Medley(refs) | Mashup(refs) | Adaptation(iswc) |
Rearrangement(iswc)`) :

| Variant | Règle on-chain | |
|---|---|---|
| `Original` | aucune | = |
| `Medley(refs)` / `Mashup(refs)` | `refs.len() >= 2` (`OutOfBounds` si < 2) ; chaque réf : structure ISWC ; **réfs distinctes et ≠ `iswc` du work** (`CrossFieldInconsistency`) ; max 32 (`WORK_REFERENCES_MAX`) | **N** (était : non-vide ≥ 1) |
| `Adaptation(iswc)` | exactement 1 ISWC (**S**) ; structure ISWC ; **≠ `iswc` du work** (`CrossFieldInconsistency`) | **N** (était : structure seule) |
| `Rearrangement(iswc)` | identique à `Adaptation` : exactement 1 ISWC (**S**) ; structure ISWC ; **≠ `iswc` du work** (`CrossFieldInconsistency`). Variant distinct (tag SCALE 4, append-only) pour porter le *type* de dérivation sur le wire | **N** |

**`ClassicalInfo`** (bloc optionnel) :

| Sous-champ | Type | Borne | Règle on-chain | |
|---|---|---|---|---|
| `opus` | `Option<MiddsString<32>>` (`OPUS_MAX_LEN = 32`) | ≤ 32 | si `Some` ⇒ non-vide | = |
| `catalog_number` | `Option<MiddsString<32>>` (`CATALOG_NUMBER_MAX_LEN = 32`) | ≤ 32 | si `Some` ⇒ non-vide | = |
| `number_of_voices` | `Option<u16>` | — | si `Some` ⇒ **`>= 1`** | **N** |

**`Creator`** : `{ roles: BoundedBTreeSet<CreatorRole, 5> (CREATOR_ROLES_MAX),
party: PartyId }`. `roles` est un **ensemble borné** (`BoundedBTreeSet`) :
les doublons sont impossibles à construire, le SCALE itère en ordre canonique
(`Ord` sur les discriminants), la cardinalité maximale est exactement le
nombre de variants `CreatorRole` (5). Validation on-chain : `roles` non-vide
(`EmptyMandatoryField`) ; `party` valide selon `PartyId` (`Ipi`, `Isni`, ou
`Both` — chaque identifiant présent doit passer sa propre structure). **Note
de réconciliation** : l'ancien front fusionnait les rôles par `PartyId` ; le
SDK V1 initial avait inverti ce choix en aplatissant la liste (plusieurs
entrées `Creator` pour le même `PartyId`) — la version stabilisée ci-dessus
revient à la fusion, plus économique en SCALE et plus fidèle au modèle métier.

> Convention *builder-side uniquement* (non bloquante on-chain, à surfacer en
> warning dans `midds-validate`) : `instrumental == true` ⇒ `language` devrait
> être `None` et `explicit_lyrics` devrait rester `false` (une œuvre
> instrumentale n'a pas de paroles).

---

## 5. `Recording` V1

| Champ | Type | Req. | Borne canonique | Règle on-chain | |
|---|---|---|---|---|---|
| `isrc` | `Isrc` | oui | 12 | structure ISRC | = |
| `title` | `Title` | oui | ≤ 256 | non-vide | = |
| `title_aliases` | `BoundedVec<Title, 8>` (`TITLE_ALIASES_MAX = 8`) | non | ≤ 8 × 256 | chaque alias non-vide | = |
| `artist` | `PartyId` | oui | — | structure de l'id | = |
| `featuring` | `BoundedVec<PartyId, 16>` (`FEATURING_MAX = 16`) | non | ≤ 16 | chaque id : structure (artistes en featuring = mêmes `PartyId` que l'artiste principal, pas des `PerformerId`) | **N** |
| `work` | `WorkRef` | oui | — | si `Iswc` ⇒ structure | = |
| `genres` | `BoundedVec<Genre, 8>` (`GENRES_MAX = 8`) | non | ≤ 8 | appartenance | S |
| `sub_genre` | `Option<Genre>` | non | — | appartenance (même taxinomie plate que `genres`) | **N** |
| `record_year` | `Option<u16>` | non | — | si `Some` ⇒ **`1..=2999`** | **N** |
| `version_type` | `Option<RecordingVersion>` | non | — | appartenance | S |
| `performers` | `BoundedVec<Performer, 64>` (`PERFORMERS_MAX = 64`) | non | ≤ 64 | chaque `Performer` : `id` (`PerformerId`) structure ; `instruments` = `BoundedVec<Instrument, 8>` (`INSTRUMENTS_PER_PERFORMER_MAX = 8`), appartenance **S**, liste vide autorisée (« instrument inconnu ») | **N** |
| `producers` | `BoundedVec<Isni, 8>` (`PRODUCERS_MAX = 8`) | non | ≤ 8 | chaque : structure ISNI (ISNI-only par design) | = |
| `duration` | `Option<u32>` | non | — | **aucun plafond** (secondes ; `u32` ≈ 136 ans, choix V1 délibéré) | (cf. §7) |
| `bpm` | `Option<u16>` | non | — | si `Some` ⇒ **`20..=300`** | **N** |
| `key` | `Option<MusicalKey>` | non | — | appartenance | S |
| `places` | `Option<ProductionPlaces>` | non | — | voir ci-dessous | |
| `contributors` | `BoundedVec<PartyId, 32>` (`CONTRIBUTORS_MAX = 32`) | non | ≤ 32 | chaque id : structure | = |
| `offchain_extension` | `Option<OffchainHash>` | non | ≤ 64 | si `Some` ⇒ non-vide | = |

**`ProductionPlaces`** (bloc optionnel) : `recording`, `mixing`, `mastering`
chacun `Option<MiddsString<128>>` (`PLACE_MAX_LEN = 128`) ; si `Some` ⇒
non-vide.

Enums fermés :

- **`Genre`** (25) : `Pop, Rock, HipHop, RnB, Electronic, Dance, Jazz, Blues,
  Classical, Country, Folk, Metal, Punk, Reggae, Latin, World, Soul, Funk,
  Gospel, Soundtrack, Ambient, Experimental, Children, SpokenWord, Other`.
  Taxonomie aplatie volontairement (l'ancien front en exposait ≈160
  hiérarchiques) — granularité fine reportée à une version payload future.
- **`RecordingVersion`** (14) : `Original, RadioEdit, Extended, Remix, Live,
  Acoustic, Instrumental, ACapella, Karaoke, Demo, ReRecorded, Edited, Cover,
  Clean`. `Clean` (version « clean » / sans contenu explicite, pendant
  parental-advisory d'une sortie explicit) est **ajouté en queue** (tag SCALE
  13, append-only) ; les tags des 13 variants précédents sont inchangés.
- **`Instrument`** (77) : instrument joué par un `Performer`. Taxinomie large
  groupée par famille (voix, claviers, cordes pincées, cordes frottées, bois,
  cuivres, percussions à hauteur définie, percussions / batterie, électronique)
  avec des génériques de famille (`Vocals`, `Guitar`, `Keyboards`, `Strings`,
  `Percussion`) et un `Other` final. Un seul tag-byte SCALE comme `Genre` ;
  nouveaux instruments **ajoutés en queue** dans une future version de payload,
  jamais réordonnés.

---

## 6. `Release` V1

| Champ | Type | Req. | Borne canonique | Règle on-chain | |
|---|---|---|---|---|---|
| `upc` | `Upc` | oui | 12 ou 13 | structure UPC/EAN | = |
| `title` | `Title` | oui | ≤ 256 | non-vide | = |
| `title_aliases` | `BoundedVec<Title, 8>` (`TITLE_ALIASES_MAX = 8`) | non | ≤ 8 × 256 | chaque alias non-vide | = |
| `artist` | `PartyId` | oui | — | structure de l'id | = |
| `tracks` | `BoundedVec<RecordingRef, 256>` (`TRACKS_MAX = 256`) | oui | ≤ 256 | **non-vide (≥ 1)** ; chaque réf : structure ; **unicité — pas de doublon de `RecordingRef`** (`CrossFieldInconsistency`) | **N** |
| `producers` | `BoundedVec<Producer, 16>` (`PRODUCERS_MAX = 16`) | non | ≤ 16 | voir ci-dessous | |
| `status` | `ReleaseStatus` | oui | — | appartenance | S |
| `release_date` | `ReleaseDate` | oui | — | `month 1..=12`, `day 1..=31` ; **`year` non contraint** (cf. §7) | = |
| `country` | `Country` | oui | — | appartenance | S |
| `distributor_name` | `MiddsString<128>` (`DISTRIBUTOR_NAME_MAX_LEN = 128`) | oui | ≤ 128 | non-vide | = |
| `release_type` | `ReleaseType` | oui | — | appartenance | S |
| `format` | `ReleaseFormat` | oui | — | appartenance | S |
| `packaging` | `ReleasePackaging` | oui | — | appartenance | S |
| `cover_contributors` | `BoundedVec<MiddsString<128>, 16>` (`COVER_CONTRIBUTORS_MAX = 16`, `…_NAME_MAX_LEN = 128`) | non | ≤ 16 × 128 | chaque nom non-vide | = |
| `offchain_extension` | `Option<OffchainHash>` | non | ≤ 64 | si `Some` ⇒ non-vide | = |

> Pas de champ `manufacturer_name` (présent dans l'ancien front, **supprimé**
> en V1 — décision conservée).

**`Producer`** : `{ isni: Isni, catalog_number: MiddsString<32> }`
(`CATALOG_NUMBER_MAX_LEN = 32`). `isni` : structure ISNI ; `catalog_number` :
**non-vide** (chaque label co-éditeur garde son propre numéro).

Enums fermés :

- **`ReleaseStatus`** (7) : `Official, Promotional, Bootleg, PseudoRelease,
  Withdrawn, Cancelled, Other`.
- **`ReleaseType`** (11) : `Album, Single, Ep, Broadcast, Compilation,
  Soundtrack, Live, Remix, Mixtape, Demo, Other`.
- **`ReleaseFormat`** (11) : `Cd, Vinyl, Cassette, DigitalDownload, Streaming,
  Dvd, BluRay, Sacd, MiniDisc, ReelToReel, Other`.
- **`ReleasePackaging`** (9) : `None, JewelCase, SlimJewelCase, Digipak,
  CardboardSleeve, Gatefold, KeepCase, Box, Other`.

---

## 7. Asymétries et choix V1 assumés

Décisions explicites, figées, à ne pas « corriger » sans bump de version :

1. **`Release.release_date.year` non contraint** (`1..=u16::MAX`), alors que
   `MusicalWork.creation_year` et `Recording.record_year` — tous deux
   `Option<u16>` — sont bornés `1..=2999` lorsqu'ils sont renseignés.
   Justification : une sortie peut être *annoncée pour le futur* (date
   prévisionnelle) ; l'ancien front n'imposait d'ailleurs aucune borne
   d'année sur la date de sortie (`z.date()` libre). Seuls `month`/`day` sont
   contrôlés (contrôle structurel, pas calendaire : 30 février est accepté
   on-chain ; le contrôle calendaire strict est du ressort de
   `midds-validate`).
2. **`Recording.duration` sans plafond** : `Option<u32>` en secondes
   (≈ 136 ans). L'ancien front plafonnait à 65535 s (18:12:15). Le SDK
   conserve `u32` délibérément — pas de plafond on-chain.
3. **`PartyId` avec variant `Both`** : V1 stabilisée = `Ipi | Isni | Both {
   ipi, isni }`. Le variant `Both` avait initialement été supprimé de la V1
   draft (par symétrie avec d'autres choix slimmés), puis **réintroduit** :
   un même intervenant porte fréquemment IPI (CISAC) et ISNI (ISO) ; les
   fusionner dans une seule structure on-chain économise du SCALE par
   rapport à deux `Creator` dupliqués pointant la même personne, et restitue
   la sémantique du domaine. Validation : chaque sous-identifiant présent
   passe son `validate_*_format`.
4. **`Creator` fusionne les rôles dans un `BoundedBTreeSet`** : la V1 draft
   représentait « plusieurs rôles pour la même partie » via une liste plate
   de `Creator` partageant le même `PartyId`. V1 stabilisée = un seul
   `Creator { roles: Set, party }` par intervenant — encodage plus compact,
   pas de doublons à valider, ordre canonique stable côté SCALE.
5. **`MusicalKey` structuré** (`PitchClass × Mode`, 34) au lieu des 42 clés
   plates de l'ancien front. Le `PitchClass` porte les 12 positions
   chromatiques avec leurs orthographes dièse et bémol usuelles (17 variantes
   au total) : `D♭` et `C♯` sont distincts sur le wire — décision motivée par
   la fidélité aux registres (CWR/DDEX transportent l'orthographe). Seules
   les enharmonies théoriques marginales (`B♯`, `E♯`, `C♭`, `F♭`) restent
   non modélisées et pourront entrer via une future version de payload si un
   usage concret apparaît.
6. **Enums slimmés** : `Genre` 25 (vs ≈160), `RecordingVersion` 13 (vs 21),
   `ReleaseFormat` 11 (vs 63), `ReleasePackaging` 9 (vs 17), `ReleaseStatus`
   7 (vs 10), `ReleaseType` 11 (vs 6, redéfini). Granularité fine =
   version payload future, pas un arbre de sous-types.
7. **Bornes de cardinalité/longueur serrées** : l'ancien front était très
   permissif (souvent 512 pour les listes de parties, 128–256 pour les
   chaînes libres). Le SDK retient des bornes optimisées pour le coût
   on-chain (`CREATORS_MAX = 32`, `CREATOR_ROLES_MAX = 5`,
   `PERFORMERS_MAX = 64`, `PRODUCERS_MAX = 8/16`, `CONTRIBUTORS_MAX = 32`,
   `OPUS/CATALOG = 32`, `PLACE = 128`, `DISTRIBUTOR/COVER_NAME = 128`,
   `TITLE_ALIASES = 8`, `GENRES = 8`, `TRACKS = 256`,
   `COVER_CONTRIBUTORS = 16`). Ces valeurs sont la référence ; les chiffres
   de l'ancien front (UI-only) sont obsolètes.
8. **Convention `instrumental ⇒ language = None` / `explicit_lyrics = false`** :
   non bloquante on-chain (le validateur ne la teste pas), à exposer en
   warning côté `midds-validate`.
9. **`MusicalWork.samples` accepte `WorkRef` (MIDDS id *ou* ISWC), pas
   `Medley`/`Mashup`** : la liste des œuvres samplées *par* cette œuvre prend
   les deux formes de référence (`WorkRef::Midds | WorkRef::Iswc`) — un sample
   peut être cité avant que l'œuvre samplée soit enregistrée — alors que les
   refs `Medley`/`Mashup` restent ISWC-only. Conséquence assumée : la
   non-auto-référence d'un sample n'est vérifiable que pour le variant `Iswc`
   (le variant `Midds` pointe un id attribué au dépôt, inconnu à la
   validation). Borne `SAMPLES_MAX = 64`.
10. **`Recording.featuring` en `PartyId`, `sub_genre` unique, `Instrument`
    large** : un artiste en featuring est crédité au même titre que l'artiste
    principal (`artist: PartyId`), pas comme un interprète de session — d'où le
    même type d'identité (IPI / ISNI / les deux), distinct du `Performer` qui
    porte un `PerformerId` (IPN-capable) **et** la liste de ses instruments.
    `sub_genre` est un `Option<Genre>` *unique* (raffinement secondaire tiré de
    la même taxinomie plate que `genres`, pas un arbre hiérarchique).
    `Instrument` est volontairement *large* (≈77 variantes) — choix inverse des
    enums slimmés du point 6 — car l'instrument joué est une donnée de crédit
    fréquente et précise ; le coût reste d'un tag-byte par instrument, plafonné
    à `INSTRUMENTS_PER_PERFORMER_MAX = 8` par performer (liste vide = instrument
    inconnu, aucune cardinalité minimale).

---

## 8. Où chaque règle s'applique

| Couche | Rôle |
|---|---|
| Type (`BoundedVec` / `MiddsString` / enum) | Longueurs max, cardinalités max, appartenance enum — **impossible à violer** par construction/décodage. |
| `Midds::validate_format` (on-chain, bloquant) | Structure des identifiants, non-vide des champs obligatoires, **bornes numériques (`creation_year`, `record_year`, `bpm`, `number_of_voices`)**, **cardinalité minimale (`Medley/Mashup ≥ 2`, `tracks ≥ 1`, `creators ≥ 1`)**, **unicité + non-auto-référence des refs `Medley`/`Mashup`/`Adaptation`/`Rearrangement` et des `samples`**, `release_date` mois/jour. Format uniquement, jamais de checksum. |
| `midds-validate` (std, warning-only, jamais on-chain) | Parsing tolérant, vérification checksums (warnings), conventions non bloquantes (`instrumental⇒language`, contrôle calendaire strict, plafond `duration` métier). |

Invariants inter-champs **appliqués** on-chain (`CrossFieldInconsistency`) :
unicité de la tracklist `Release` (aucun `RecordingRef` en double) ; refs
sources d'un `MusicalWork` `Medley` / `Mashup` / `Adaptation` / `Rearrangement`
distinctes et différentes de l'`iswc` du work lui-même ; `samples` d'un
`MusicalWork` distinctes (aucun `WorkRef` en double) et, pour le variant ISWC,
différentes de l'`iswc` du work.

Invariant réservé non encore utilisé : `MiddsFormatError::DateInconsistency`
(ex. `recording_year > work_year`) — prévu pour un durcissement inter-champs
ultérieur sans nouvelle variante SCALE.
