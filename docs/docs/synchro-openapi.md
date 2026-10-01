# Synchro OpenAPI à 3 voies — conception (MVP-γ)

1er octobre 2026 · référence : `cahier-des-charges.md` § 4 et § 6, `etude-synchro-openapi-bruno.md`, maquette `design/maquette-v2/` (vue « Synchro OpenAPI »), `../DESIGN.md` (« Éditeur de fusion à 3 voies »).

Ce document est le contrat d'implémentation du MVP-γ : stockage, règles de fusion, écriture et interface IPC.

## 1. Vocabulaire

- **Base** : la spec telle qu'elle était à la dernière synchro réussie (ou à l'import).
- **Équipe** (*ours*) : les fichiers de requête de la collection, tels qu'ils sont sur le disque.
- **Spec** (*theirs*) : la nouvelle version de la spec (fichier ou URL).
- **Opération** : une méthode + un chemin de la spec. Elle correspond à un fichier de requête.

## 2. Stockage `.oc-sync/openapi/`

Décisions du 1er octobre 2026 :

- la base est une **copie brute de la spec** ; celle de chaque opération est recalculée à chaque synchro avec le convertisseur courant ;
- **aucune** entrée `extensions.bruno.openapi` n'est écrite (si Bruno en a écrit une, elle est conservée telle quelle) ;
- `.oc-sync/` est fait pour être versionné.

```
.oc-sync/openapi/
├── source.yml
└── spec.json        (ou spec.yaml, selon le contenu)
```

`source.yml`, écrit avec l'émetteur YAML du projet (style Bruno), sans champ volatil (ni date, ni hash) :

```yaml
source: https://api.example.com/openapi.json
groupBy: tags
spec: spec.json
operations:
  - key: listPets
    file: pets/List pets.yml
  - key: GET /pets/{}
    file: pets/Info for a specific pet.yml
  - key: deletePet
    file: pets/Delete a pet.yml
    removed: true
  - key: createInvoice
    ignored: true
```

- **`source`** est soit l'URL, sans identifiants ni paramètres secrets (même nettoyage qu'au MVP-β), soit le chemin de la spec relatif à la racine de la collection, avec des `/`.
- **`spec`** est le nom du fichier de la copie brute.
  - Une spec JSON est réindentée avec `xc_core::pretty::pretty_json`, pour que le diff Git soit lisible. Cette réindentation préserve les jetons.
  - Une spec YAML est copiée octet pour octet.
- **`operations`** suit l'ordre de la spec et garde, pour chaque opération :
  - `key` : la clé d'opération (`operationKey` de `xc_sync::openapi::to_bruno`) ;
  - `file` : le fichier de requête de la collection ;
  - `removed: true` : l'opération a été retirée de la spec et la requête est conservée, marquée dépréciée (EF-SYN-03) ;
  - `ignored: true` : l'équipe a refusé de créer cette nouvelle opération, ou a oublié une requête supprimée. Une entrée `ignored` n'a pas de `file`.
- Les clés `removed` et `ignored` ne sont écrites que si elles valent `true`.

L'import (MVP-β) écrit ce même format. Les instantanés `base/<clé>.yml` du MVP-β disparaissent. Aucune migration n'est nécessaire : le MVP-β n'a pas été publié.

## 3. Appariement des opérations (EF-SYN-01)

1. **Avec `source.yml`** : la clé de la spec est cherchée dans `operations`.
   - Le fichier de l'équipe est `file`. S'il a disparu, l'opération est **manquante**.
2. **Sans `source.yml`** (connecter une collection existante, par exemple importée par Bruno) : il n'y a pas de base.
   - Chaque opération de la spec est rapprochée d'une requête de la collection dont la méthode est la même et dont le chemin normalisé est le même.
   - La normalisation de l'URL de la requête se fait dans cet ordre :
     - retirer un ou plusieurs segments `{{…}}` en tête ;
     - retirer schéma et hôte si l'URL est absolue ;
     - retirer la query ;
     - remplacer chaque segment `:nom` ou `{nom}` par `{}` ;
     - retirer le `/` final.
   - La spec est normalisée de la même façon.
   - Un rapprochement ambigu (plusieurs candidats) n'est pas fait : l'opération est traitée comme nouvelle.
3. **Rapprochement manuel** : quand un chemin change sans `operationId`, l'opération retirée et la nouvelle opération sont proposées comme candidates.
   - Mêmes critères de proposition : même méthode, et même nom de requête, ou même ensemble de noms de paramètres, ou même nombre de segments dont au moins la moitié sont égaux.
   - L'utilisateur peut les rapprocher ; le plan est alors recalculé.
   - Une opération rapprochée garde son fichier. Sa base est celle de l'ancienne clé. Sa clé devient la nouvelle.

## 4. Classement des opérations

| Statut | Condition | Effet par défaut |
| --- | --- | --- |
| `unchanged` | Aucun changement | Rien |
| `updated` | Seuls des changements de la spec, appliqués | Écrit |
| `kept` | Seuls des changements de l'équipe, conservés | Rien |
| `merged` | Changements des deux côtés, sans conflit | Écrit |
| `conflict` | Au moins un conflit | Écrit après arbitrage |
| `new` | Dans la spec, sans entrée dans `operations` (ou sans appariement) | Fichier créé dans le dossier de son tag (EF-SYN-02) |
| `removed` | Dans `operations` mais plus dans la spec | `removed: true`, fichier intact (EF-SYN-03) |
| `restored` | Marquée `removed` et de retour dans la spec | Drapeau retiré, fusion normale |
| `missing` | Dans `operations` et dans la spec, mais fichier introuvable | Oubliée (`ignored`) sauf choix « recréer » |

Les requêtes de la collection qui ne sont dans aucune entrée sont des requêtes de l'équipe : la synchro ne les lit ni ne les touche.

## 5. Règles de fusion par champ (EF-SYN-04, propriété des champs du § 6)

Notation : B = base, O = équipe, T = spec. Fusion 3 voies standard d'une valeur :

- O = T : on prend la valeur (`same` si elle diffère de B, sinon rien) ;
- O = B : on prend T (`applied`) ;
- T = B : on garde O (`kept`) ;
- sinon : `conflict`.

Un conflit n'est jamais tranché automatiquement.

| Champ | Propriétaire | Règle |
| --- | --- | --- |
| Méthode | Spec | Fusion 3 voies de la valeur |
| Adresse (URL sans la query) | Spec | Fusion 3 voies de la valeur. Les paramètres de chemin suivent les segments `:nom` de l'adresse fusionnée ; leurs valeurs sont reprises par nom. |
| Paramètres query et path, clé (type, nom) | Spec pour l'existence et la description, équipe pour la valeur et l'activation | Voir « Collections à clé » |
| En-têtes, clé = nom sans casse | Idem | Idem. Un en-tête ajouté par l'équipe n'est jamais touché. |
| Corps | Spec, mais un corps édité par l'équipe compte comme O | Voir « Corps » |
| Auth | Spec, proposée, jamais imposée | Fusion 3 voies de l'objet entier |
| Nom, `seq`, docs, scripts, tests, assertions, variables, settings, tags, exemples, dossier | Équipe | Jamais modifiés |

**Collections à clé** (paramètres, en-têtes, champs de formulaire) :

- **Ajouté par la spec** (pas dans B ni O) : ajouté, à la fin, dans l'ordre de T (`applied`).
- **Ajouté par l'équipe** (pas dans B, dans O) : gardé tel quel, pas de changement.
- **Ajouté des deux côtés** : on garde O (`kept`).
- **Retiré par la spec** (dans B et O, pas dans T) :
  - retiré si O = B pour cet élément (`applied`) ;
  - sinon `conflict` (équipe = garder, spec = retirer).
- **Retiré par l'équipe** (dans B, pas dans O) :
  - reste retiré si T = B ;
  - sinon `conflict` (équipe = laisser retiré, spec = rajouter T).
- **Présent partout** :
  - valeur et activation : priorité à l'équipe, prendre T si O = B, sinon garder O, jamais de conflit ;
  - description : fusion 3 voies.
- **Ordre** : celui de O, puis les ajouts de la spec.

**Corps** :

- **Valeurs** : (type, contenu) forment une valeur.
- **O = B ou T = B** : règle standard.
- **Les deux ont changé, et B, O et T sont en JSON** : fusion 3 voies du JSON.
  - Les objets sont fusionnés clé par clé, récursivement.
  - Les tableaux et les scalaires sont fusionnés comme des valeurs.
  - Une clé retirée d'un côté et modifiée de l'autre est un conflit.
  - Ordre des clés : celui de O, puis les nouvelles clés de T.
  - Les `{{var}}` hors chaînes sont masqués avant l'analyse et restaurés après.
  - Sortie indentée à 2 espaces, comme `JSON.stringify(v, null, 2)`.
  - Résultat : `merged` sans conflit, sinon `conflict` sur le corps entier, avec les choix équipe, spec, « combiner » (fusion JSON où l'équipe gagne sur les chemins en conflit) et éditer.
- **form-urlencoded ou multipart du même type des deux côtés** : règles des collections à clé, clé = nom du champ.
- **Autres cas** : `conflict` (équipe, spec, et éditer si le contenu est textuel).

**Sans base** (connexion) :

- méthode, adresse, corps, auth : égaux, ou `conflict` ;
- collections à clé : les éléments seulement dans T sont ajoutés, ceux seulement dans O sont gardés, les valeurs et descriptions qui diffèrent sont gardées (`kept`).

**URL** : si les paramètres query n'ont pas changé, l'URL de l'équipe est conservée à l'octet. Sinon, la query est reconstruite à partir des paramètres query activés, comme le fait l'interface (`app/src/app/core/url.ts`, `urlFromParams`).

**Identifiants de changement**, stables dans un plan :

- `<clé>::method`, `<clé>::url`, `<clé>::auth` ;
- `<clé>::param/query/<nom>`, `<clé>::param/path/<nom>` ;
- `<clé>::header/<nom en minuscules>` ;
- `<clé>::body`, `<clé>::body/form/<nom>`.

## 6. Plan, arbitrage et écriture (EF-SYN-05, EF-SYN-06)

1. **`plan`** lit la spec (fichier ou URL), la base (copie brute convertie avec le `groupBy` de `source.yml`) et les fichiers de l'équipe. Il calcule tout **sans rien écrire**. Le plan est gardé en mémoire côté Rust sous un identifiant.
2. **Arbitrage** : chaque conflit reçoit un choix parmi :
   - `team` (garder l'équipe) ;
   - `spec` (prendre la spec) ;
   - `both` (combiner, corps JSON seulement) ;
   - `edit` (valeur saisie : adresse, valeur d'un paramètre ou d'un en-tête, contenu textuel du corps).
3. **Autres décisions** : sauter une nouvelle opération (`skip`, elle devient `ignored`) ; pour une opération manquante, choisir `recreate` ou `forget` (`forget` par défaut).
4. **`apply`** refuse s'il reste un conflit sans choix. On ne peut donc pas avancer la base en laissant un conflit qui disparaîtrait à la synchro suivante. Sinon, il écrit dans cet ordre :
   1. **Fichiers modifiés** : via `RequestDoc::apply`, qui ne réécrit que les sections changées et conserve les champs inconnus, puis `write_atomic`.
   2. **Nouveaux fichiers** :
      - item de la spec sérialisé par `stringify::item`, dans le dossier de son tag (créé avec son `folder.yml` s'il n'existe pas) ;
      - nommage de l'import (noms uniques, réservés ou masqués) ;
      - `seq` suivant du dossier.
      - Idempotent : un fichier existant au contenu identique est réutilisé.
   3. **Copie brute** de la spec.
   4. **`source.yml` en dernier.**
5. **Reprise** : une synchro interrompue avant `source.yml` se rejoue sans dégât.
   - La base n'a pas bougé.
   - Les champs déjà écrits donnent O = T (`same`).
   - Les fichiers déjà créés sont réutilisés.

## 7. CLI (EF-SYN-07)

```bash
xc sync <collection> --check [--source <fichier-ou-url>]
```

Sort avec le code 1 si la spec a divergé de la base, c'est-à-dire s'il existe une opération `updated`, `merged`, `conflict`, `new`, `removed` ou `restored`. Un résumé est affiché en français. Code 0 sinon ; code 2 pour une entrée illisible ou une collection non connectée sans `--source`.

```bash
xc sync <collection> --apply [--source <fichier-ou-url>] [--keep-team | --take-spec]
```

Applique le plan.

- S'il reste des conflits sans l'une des deux options : refus, avec le code 1 et la liste des conflits.
- Les nouvelles opérations sont créées ; les manquantes sont oubliées.

## 8. Interface IPC (Tauri)

Les arguments sont en camelCase côté JS ; les erreurs arrivent sous forme de chaînes en français.

```ts
type GroupBy = 'tags' | 'path';
type ChangeKind = 'applied' | 'kept' | 'same' | 'merged' | 'conflict';
type Choice = 'team' | 'spec' | 'both' | 'edit';
type OpStatus = 'unchanged' | 'updated' | 'kept' | 'merged' | 'conflict' | 'new' | 'removed' | 'restored' | 'missing';

interface SyncStatus {
  connected: boolean; source: string | null; groupBy: GroupBy | null;
  operationCount: number; removedCount: number;
}
interface SpecRef { title: string; version: string }
interface SyncChange {
  id: string; field: 'method' | 'url' | 'param' | 'header' | 'body' | 'auth'; label: string; reason: string;
  kind: ChangeKind; base: string | null; ours: string | null; theirs: string | null; result: string | null;
  choices: Choice[];
}
interface SyncOperation {
  key: string; name: string; method: string; path: string; file: string | null;
  status: OpStatus; changes: SyncChange[];
}
interface SyncPairing { removed: string; added: string; reason: string }
interface SyncPlan {
  id: string; source: string; groupBy: GroupBy; hasBase: boolean; from: SpecRef | null; to: SpecRef;
  summary: { unchanged: number; updated: number; kept: number; merged: number; conflicts: number;
             conflictFields: number; created: number; removed: number; restored: number; missing: number };
  operations: SyncOperation[]; suggestions: SyncPairing[];
}
interface SyncDecisions {
  choices: Record<string, { choice: Choice; value?: string }>;
  skip: string[]; recreate: string[];
}
interface Hunk { changeId: string; ours: [number, number] | null; theirs: [number, number] | null;
                 base: [number, number] | null; result: [number, number] | null }
interface OpView { ours: string; theirs: string; base: string | null; result: string; hunks: Hunk[] }
interface SyncReport { written: string[]; created: string[]; removed: string[]; ignored: string[] }
```

| Commande | Rôle |
| --- | --- |
| `sync_status(root) -> SyncStatus` | Lit `source.yml` |
| `sync_plan(root, source: string \| null, pairings: [string, string][]) -> SyncPlan` | `source` null = source enregistrée ; `pairings` = rapprochements manuels (clé retirée, clé ajoutée) |
| `sync_op_view(planId, key, decisions) -> OpView` | Les quatre fichiers YAML complets d'une opération (équipe, spec, base, résultat selon les décisions) et, pour chaque changement non trivial, les plages de lignes (1-based, inclusives) à surligner |
| `sync_apply(planId, decisions) -> SyncReport` | Écrit (§ 6) ; erreur s'il reste un conflit sans choix ; le plan est ensuite oublié |

`TreeItem` de type request gagne `deprecated: boolean`, vrai pour les fichiers marqués `removed` dans `source.yml`.

## 9. Interface utilisateur

L'interface suit la maquette v2, vue « Synchro OpenAPI », et DESIGN.md, « Éditeur de fusion à 3 voies ».

- **Barre d'activité** : entrée « Synchro OpenAPI » (icône `merge`), avec un badge ambre qui compte les conflits non arbitrés du plan courant.
- **Barre latérale** :
  - en tête : source et versions (`v2.3.0 → v2.4.0`), et « Relancer la comparaison » ;
  - compteurs : conflits, fusions auto, nouvelles, dépréciées ;
  - groupes : « À arbitrer » (une ligne par conflit, avec le champ et l'état « à arbitrer » ou le choix fait), « Fusion automatique », « Nouvelles » (case pour ne pas créer), « Dépréciées », « Manquantes » (recréer ou oublier) ;
  - rapprochements proposés.
- **Éditeur** :
  - en-tête : méthode, chemin, champ et raison ;
  - navigation entre conflits (⌥↑ / ⌥↓) ;
  - bouton « Base » ;
  - volets Équipe et Spec (Base en option) au-dessus, Résultat en dessous ;
  - lignes identiques repliées ;
  - au-dessus de chaque conflit du Résultat, une lentille « Garder l'équipe · Prendre la spec · Combiner les deux · Éditer à la main », limitée aux choix proposés.
- **Pied d'îlot** :
  - état ambre (« n conflits à arbitrer avant d'appliquer ») ou vert ;
  - la note « Rien n'est écrit sur le disque avant ta validation. La base .oc-sync est réécrite en dernier. » ;
  - Annuler, et « Appliquer n changements », désactivé tant qu'il reste un conflit.
- **Collection non connectée** : état vide « Connecter une spec OpenAPI » (fichier ou URL), puis plan sans base.
- **Après application** : état « Spec synchronisée », arbre relu.
- **Barre d'état** : « Spec : n conflits » en ambre, ou « Spec à jour » en vert.
- **Arbre** : une requête dépréciée est barrée et porte une infobulle « Retirée de la spec ». Une requête en conflit dans le plan courant porte l'icône `alert` en ambre.
- **Couleurs** : l'ambre est réservé aux conflits ; le bleu d'information marque ce qui vient de la spec.
