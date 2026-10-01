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

- la base est une **copie brute de la spec** ; celle de chaque opération est recalculée à chaque synchro avec le convertisseur courant, sauf pour une opération retirée de la spec, dont la dernière version est gardée dans `removed/` ;
- **aucune** entrée `extensions.bruno.openapi` n'est écrite (si Bruno en a écrit une, elle est conservée telle quelle) ;
- `.oc-sync/` est fait pour être versionné.

```
.oc-sync/openapi/
├── source.yml
├── spec.json        (ou spec.yaml, selon le contenu)
└── removed/
    └── <clé assainie>.yml
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
  - `file` : le fichier de requête de la collection. Un fichier n'est suivi que par une entrée (deux entrées sur le même fichier sont refusées à la lecture), et son chemin ne sort pas de la collection, n'est ni caché ni réservé à aucun niveau (`opencollection.yml`, `folder.yml`, `environments/`…) ;
  - `removed: true` : l'opération a été retirée de la spec et la requête est conservée, marquée dépréciée (EF-SYN-03) ;
  - `ignored: true` : l'équipe a refusé de créer cette nouvelle opération, ou a oublié une requête supprimée. Une entrée `ignored` n'a pas de `file`.
- Les clés `removed` et `ignored` ne sont écrites que si elles valent `true`.
- **`removed/<clé assainie>.yml`** : à la synchro qui retire une opération de la spec, la dernière version de sa requête (l'item de la base, sérialisé par `stringify::item`). C'est la base de l'opération si elle revient (§ 4). Le nom est la clé dont les caractères interdits dans un nom de fichier sont écrits `%XX`, ce qui ne confond jamais deux clés. Le fichier est supprimé à la synchro qui suit la restauration, le rapprochement ou l'oubli de l'opération.

L'import (MVP-β) écrit ce même format. Les instantanés `base/<clé>.yml` du MVP-β disparaissent. Aucune migration n'est nécessaire : le MVP-β n'a pas été publié.

## 3. Appariement des opérations (EF-SYN-01)

**Clé d'opération** : l'`operationId` s'il est non vide, sinon `MÉTHODE chemin` (chaque segment `{…}` du chemin devient `{}`). Un `operationId` porté par plusieurs opérations est suivi de `(MÉTHODE chemin)`, par exemple `dup (GET /a)`, pour que la clé ne dépende pas du rang de l'opération dans la spec. Une clé encore partagée (des variantes de la même opération, avec la même méthode et le même chemin) reçoit ` #2`, ` #3`…

1. **Avec `source.yml`** : la clé de la spec est cherchée dans `operations`.
   - Le fichier de l'équipe est `file`. S'il a disparu (déplacé ou renommé), il est retrouvé quand une seule requête non suivie de la collection a la même méthode et le même chemin normalisé (voir 2.), d'après la spec puis d'après la base : l'opération reprend ce fichier, et `source.yml` est mis à jour à l'application. Sinon, l'opération est **manquante**.
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
   - Chaque opération retirée n'a qu'une candidate, la plus proche ; une nouvelle opération n'est proposée qu'à une seule opération retirée ; au plus 50 rapprochements sont proposés, les plus sûrs d'abord.
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
| `removed` | Dans `operations` mais plus dans la spec | `removed: true`, fichier intact, dernière version gardée dans `removed/` (EF-SYN-03) |
| `restored` | Marquée `removed` et de retour dans la spec | Drapeau retiré, fusion normale (base : la version de `removed/`, sans elle un conflit au moindre écart) |
| `missing` | Dans `operations` et dans la spec, mais fichier introuvable et aucune requête non suivie à reprendre (§ 3) | Oubliée (`ignored`) sauf choix « recréer » |

Les requêtes de la collection qui ne sont dans aucune entrée sont des requêtes de l'équipe : la synchro ne les lit ni ne les touche.

Une spec sans aucune opération est refusée (erreur d'entrée) quand des opérations sont suivies : vide ou tronquée, elle marquerait toutes les requêtes comme retirées. Sans copie brute de la spec de la base (fichier disparu), le plan se fait sans base (`hasBase: false`) : tout écart est un conflit. Un fichier de l'équipe illisible bloque le plan, l'erreur le nomme.

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
| Auth | Spec, proposée, jamais imposée | Fusion 3 voies de l'objet entier ; pour un type que le modèle ne détaille pas (oauth2, digest, awsv4…), la configuration entière est comparée sous forme canonique (clés triées), jamais le seul type |
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
  - Les `{{var}}` et les nombres hors chaînes sont masqués avant l'analyse et restaurés après : le jeton de l'équipe ressort tel qu'il a été écrit (`1.50`, `1E3`, entiers au-delà de 2^53).
  - Sortie indentée à 2 espaces, comme `JSON.stringify(v, null, 2)`.
  - Résultat : `merged` sans conflit, sinon `conflict` sur le corps entier, avec les choix équipe, spec, « combiner » (fusion JSON où l'équipe gagne sur les chemins en conflit) et éditer.
- **form-urlencoded ou multipart du même type des deux côtés** : règles des collections à clé, clé = nom du champ.
- **Autres cas** : `conflict` (équipe, spec, et éditer si le contenu est textuel). Un corps d'un type que le modèle ne détaille pas est comparé, comme l'auth, par sa configuration canonique.

**Sans base** (connexion) :

- méthode, adresse, corps, auth : égaux, ou `conflict` ;
- collections à clé : les éléments seulement dans T sont ajoutés, ceux seulement dans O sont gardés, les valeurs et descriptions qui diffèrent sont gardées (`kept`).

**Paramètres de chemin** : quand la spec renomme un seul segment de l'adresse, à la même position, le paramètre de l'équipe suit le nouveau nom, avec sa valeur. Quand le segment disparaît autrement, le paramètre disparaît avec lui, sauf si l'équipe en avait saisi la valeur : c'est alors un conflit (`team` : garder, `spec` : retirer), jamais une perte silencieuse.

**URL** : si les paramètres query n'ont pas changé, l'URL de l'équipe est conservée à l'octet. Sinon, la query de l'équipe est mise à jour, pas reconstruite : le segment d'un paramètre retiré ou désactivé disparaît, celui d'un paramètre modifié est remplacé sur place, les paramètres activés que la query ne contient pas sont ajoutés à la fin, et ce qu'aucun paramètre ne représente est gardé.

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
   1. **Fichiers modifiés** : via `RequestDoc::apply`, qui ne réécrit que les sections changées et conserve les champs inconnus (y compris dans les éléments d'une liste, d'un corps ou d'une auth réécrits, appariés par nom et type), puis `write_atomic`, qui garde les permissions du fichier et écrit la cible d'un lien symbolique de la collection au lieu de le remplacer. Un fichier à réécrire doit être une requête HTTP (`info.type` : `http`) et ne pas avoir changé depuis le plan, sinon l'application est refusée avant toute écriture.
   2. **Nouveaux fichiers** :
      - item de la spec sérialisé par `stringify::item`, dans le dossier de son tag (créé avec son `folder.yml` s'il n'existe pas ; un dossier existant sans `folder.yml` le reçoit quand la spec en fournit un) ;
      - nommage de l'import (noms uniques, réservés ou masqués) ;
      - `seq` suivant du dossier.
      - Idempotent : un fichier existant au contenu identique est réutilisé, sauf s'il est suivi par une autre opération. S'il n'est suivi que par une opération retirée, la nouvelle opération le reprend : c'est un rapprochement (un `operationId` renommé, par exemple), l'ancienne entrée disparaît et le fichier n'est ni créé ni marqué retiré.
   3. **Dernière version des opérations retirées** : `removed/<clé>.yml`.
   4. **Copie brute** de la spec.
   5. **`source.yml` en dernier**, puis suppression des versions de `removed/` qui ne correspondent plus à une opération retirée.
5. **Écriture voulue** : un fichier réécrit est réémis entièrement avec l'émetteur de style Bruno ; les commentaires YAML, les fins de ligne CRLF et l'indentation non standard sont perdus, comme Bruno le fait à l'enregistrement. Les champs inconnus, eux, sont conservés.
6. **Reprise** : une synchro interrompue avant `source.yml` se rejoue sans dégât.
   - La base n'a pas bougé.
   - Les champs déjà écrits donnent O = T (`same`).
   - Les fichiers déjà créés sont réutilisés.

## 7. CLI (EF-SYN-07)

```bash
xc sync <collection> --check [--source <fichier-ou-url>]
```

Sort avec le code 1 si la spec a divergé de la base, c'est-à-dire s'il existe une opération `updated`, `merged`, `conflict`, `new`, `removed`, `restored` ou `missing`. Un résumé est affiché en français. Code 0 sinon ; code 2 pour une entrée illisible ou une collection non connectée sans `--source`.

```bash
xc sync <collection> --apply [--source <fichier-ou-url>] [--keep-team | --take-spec] [--forget-missing | --recreate-missing]
```

Applique le plan.

- S'il reste des conflits sans `--keep-team` ni `--take-spec` : refus, avec le code 1 et la liste des conflits.
- S'il reste des opérations manquantes (fichier introuvable) sans `--forget-missing` ni `--recreate-missing` : refus, avec le code 1 et la liste des opérations. `--forget-missing` les oublie (`ignored`), `--recreate-missing` recrée leur fichier.
- Les nouvelles opérations sont créées.

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

Pour un conflit qui propose `edit` sur un élément à clé (paramètre, en-tête ou champ de formulaire retiré d'un côté et modifié de l'autre), `base`, `ours` et `theirs` portent la valeur brute, prête à préremplir la saisie ; `result` reste le texte affiché (`valeur (désactivé) — description`). Pour les autres conflits, ces champs portent le texte affiché.

`RequestDoc` (lu et enregistré par `read_request` et `save_request`) : une `auth` ou un `body` de type `other` porte en plus `config`, le texte YAML canonique (clés triées) du reste de sa configuration ; l'interface le renvoie tel quel à l'enregistrement, et un `config` absent est lu comme vide.

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
  - une source fichier est comparée à l'ouverture de la vue ; une source URL ne l'est qu'au clic sur « Relancer la comparaison » (aucun appel réseau non demandé) ;
  - compteurs : conflits, fusions auto, nouvelles, dépréciées ;
  - groupes : « À arbitrer » (une ligne par conflit, avec le champ et l'état « à arbitrer » ou le choix fait), « Fusion automatique », « Nouvelles » (case pour ne pas créer ; leur fichier n'est connu qu'à l'application, la ligne indique « → dossier de son tag »), « Dépréciées », « Manquantes » (recréer ou oublier) ;
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
