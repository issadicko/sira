# Gestion de collection — conception (MVP-δ)

1er octobre 2026 · exigences EF-COL-01, EF-COL-04, EF-SYN-01 · sources de Bruno au commit `a2dbda9f` (`packages/bruno-electron/src/ipc/collection.js`, noté E, `packages/bruno-app/src/providers/ReduxStore/slices/collections/actions.js`, noté A, `packages/bruno-electron/src/utils/filesystem.js`, noté FS)

Partir de zéro et organiser une collection comme dans Bruno.

- Les fichiers **créés** sont identiques à ceux de Bruno, ce que vérifient les fixtures produites par `tools/oracle`.
- Les fichiers **modifiés** ne le sont que sur la ligne concernée. Bruno, lui, re-sérialise tout le fichier et perd au passage les clés inconnues.
- **Écarts volontaires** : Bruno supprime définitivement et renomme de façon non atomique ; nous non (§ 7).

## 1. Ouvrir un dossier qui n'est pas une collection

`inspect_folder(path)` classe le dossier choisi :

| Classe | Condition | Interface |
| --- | --- | --- |
| `collection` | `opencollection.yml` présent | Ouverture normale |
| `empty` | Aucune entrée (hors fichiers cachés du système comme `.DS_Store`) | « Créer une collection ici » : nom proposé = nom du dossier |
| `bru` | `bruno.json` présent | « Collection au format .bru : lecture prévue en V1 » |
| `other` | Autre contenu | « Créer une collection ici » avec un avertissement : le dossier n'est pas vide, rien d'existant n'est modifié |

## 2. Créer une collection (E:197-262)

Deux façons de créer :

- `create_collection(parent, name)` crée un **nouveau** dossier `sanitizeName(name)` sous `parent`, comme « Nouvelle collection » dans Bruno.
  - Si le dossier existe et est vide, il est réutilisé.
  - S'il existe et n'est pas vide, on suffixe ` 1`, ` 2`… (FS:211).
- `init_collection(dir, name)` crée la collection **dans** un dossier existant (classe `empty` ou `other`), pour « Créer une collection ici ».
  - Refus si `opencollection.yml` existe déjà.

Le nom est validé par `validateName` : non vide, 255 caractères au plus, pas de nom de périphérique Windows, premier caractère ni espace ni `-`.

Fichiers écrits, seulement ceux-là (ni `environments/` ni `.env`) :

`opencollection.yml` = `stringify::collection({meta:{name}}, {opencollection:'1.0.0', name, type:'collection', ignore:['node_modules','.git']})`, soit :

```yaml
opencollection: 1.0.0

info:
  name: <nom>
bundled: false
extensions:
  bruno:
    ignore:
      - node_modules
      - .git
```

`.gitignore`, sans saut de ligne final (FS:11-21) :

```
# Secrets
.env*

# Dependencies
node_modules

# OS files
.DS_Store
Thumbs.db
```

Ne pas écraser un `.gitignore` existant (classe `other`).

## 3. Créer une requête et un dossier

**Requête HTTP vierge** (A:1409-1556) : item Bruno écrit par `stringify::item` dans `<sanitizeName(nom)>.yml` :

```json
{"type":"http-request","name":"<nom>","seq":<n>,
 "request":{"method":"GET","url":"","headers":[],"params":[],
   "body":{"mode":"none","json":null,"text":null,"xml":null,"sparql":null,"multipartForm":[],"formUrlEncoded":[],"file":[]},
   "vars":{"req":[],"res":[]},"assertions":[],"auth":{"mode":"inherit"}},
 "settings":{"encodeUrl":true,"forwardAuthorizationHeader":false}}
```

- `seq` = nombre de dossiers et de requêtes du dossier parent, plus 1 (comme pour cURL au MVP-β).
- Nom refusé : `collection` et `folder`.
- Si le fichier existe déjà, on suffixe ` 1`, ` 2`… sur le nom de fichier seulement, avec une création exclusive. `info.name` garde le nom saisi.

**Dossier** (E:1254-1274) :

- `sanitizeName(nom)` dans le dossier parent, avec un suffixe ` n` si le nom est pris.
- `folder.yml` est écrit tout de suite par `stringify::folder({meta:{name, seq}, request:{auth:{mode:'inherit'}}})` :

```yaml
info:
  name: <nom>
  type: folder
  seq: <n>

request:
  auth: inherit
```

- Noms refusés, sans tenir compte de la casse :
  - à la racine : `environments`, `mocks`, `opencollection.yml` ;
  - partout : `.oc-sync`, `.git`, `node_modules` ;
  - tout nom que l'arbre masquerait (`xc_core::collection::is_hidden`).

## 4. Renommer, dupliquer, supprimer

**Renommer** une requête change à la fois le nom affiché et le nom du fichier, comme Bruno quand le nom de fichier suit le nom affiché.

- Seule la ligne `info.name` est modifiée, via `RequestDoc::apply` ; le reste du fichier est intact.
- Le fichier est ensuite renommé de façon atomique, avec un suffixe ` n` si le nouveau nom est pris.
- Si le nom de fichier assaini est inchangé, seul `info.name` change.
- Pour un dossier : `info.name` de `folder.yml` (fichier créé, minimal, s'il manque, comme E:1188-1207), puis renommage du dossier.

**Dupliquer** une requête :

- copie à l'octet du fichier, puis seules les lignes `info.name` et `info.seq` changent ;
- nom proposé : « <nom> copie » ;
- `seq` = fin de liste du dossier ;
- même dossier, nom de fichier suffixé si besoin.

Pour un dossier : copie complète de l'arborescence, sans perte de fichier (Bruno ne recopie que les requêtes http/graphql/grpc), puis seules `info.name` et `info.seq` du `folder.yml` de la copie changent.

**Supprimer** une requête ou un dossier :

- toujours vers la **corbeille du système**, après confirmation dans l'interface ;
- les frères ne sont pas renumérotés, les trous de `seq` sont tolérés par le tri ;
- les onglets ouverts sur l'élément supprimé se ferment.

## 5. Réordonner et déplacer (glisser-déposer)

Position de dépôt : `before`, `after` (d'un frère) ou `inside` (d'un dossier).

- **Réordonner dans un dossier** (U:1504-1600) :
  1. On trie les frères (dossiers et requêtes confondus) par `sortByNameThenSequence`.
  2. On déplace l'élément.
  3. On renumérote de 1 à n.
  4. On écrit seulement les `seq` qui changent, toujours y compris celui de l'élément déplacé : c'est un écart voulu, Bruno peut laisser deux `seq` égaux. Seule la ligne `info.seq` change (requête ou `folder.yml`).
- **Déplacer vers un autre dossier** :
  - renommage atomique du fichier ou du dossier entier ;
  - copie, vérification, puis suppression de la source seulement si les volumes diffèrent ;
  - suffixe ` n` si le nom est pris ;
  - `inside` : `seq` = max des frères + 1 ;
  - `before` / `after` : renumérotation du dossier cible comme ci-dessus.
- Déposer un dossier dans lui-même ou dans un de ses descendants est refusé.

## 6. Synchro OpenAPI (EF-SYN-01)

Si `.oc-sync/openapi/source.yml` existe, chaque action le met à jour dans la même opération, en dernier et de façon atomique :

- **Renommer ou déplacer une requête** : l'entrée dont le `file` correspond prend le nouveau chemin.
- **Renommer ou déplacer un dossier** : toutes les entrées dont le `file` commence par l'ancien préfixe.
- **Dupliquer** : la copie n'est pas suivie.
- **Supprimer une requête suivie** : son entrée passe à `ignored: true`, sans `file`, car l'équipe ne la veut plus. Supprimer un dossier fait de même pour toutes ses entrées.

En plus, connecter une collection à une spec (vue de synchro, première synchro) ajoute `.oc-sync` à `extensions.bruno.ignore` de `opencollection.yml`, comme l'import, pour que Bruno ne l'affiche pas. Seule cette liste change.

## 7. Écarts volontaires avec Bruno

| Bruno | Nous | Raison |
| --- | --- | --- |
| Suppression définitive (`rmSync`, `unlinkSync`) | Corbeille du système | Principe 5, non destructif |
| Renommer : `unlink` puis `writeFile`, re-sérialisation complète (clés inconnues perdues) | Renommage atomique, seule `info.name` change | Non destructif, diff Git minimal |
| Réordonner : re-sérialisation complète, brouillon inclus | Seule `info.seq` change ; le brouillon non enregistré reste un brouillon | Idem |
| Le `seq` de l'élément déplacé n'est pas toujours réécrit | Toujours réécrit | Évite les `seq` en double |
| Dupliquer un dossier perd les fichiers non-requêtes | Copie complète | Non destructif |
| Renumérotation des frères après suppression | Aucune | Diff minimal |

## 8. Interface IPC (Tauri)

```ts
type FolderKind = 'collection' | 'empty' | 'bru' | 'other';
type DropPosition = 'before' | 'after' | 'inside';
```

| Commande | Rôle |
| --- | --- |
| `inspect_folder(path) -> FolderKind` | § 1 |
| `create_collection(parent, name) -> string` | Chemin absolu de la nouvelle racine |
| `init_collection(dir, name) -> string` | Chemin absolu de la racine (= `dir`) |
| `create_request(root, folder, name) -> string` | Chemin relatif (`/`) du fichier créé ; `folder` relatif, `""` = racine |
| `create_folder(root, parent, name) -> string` | Chemin relatif du dossier créé |
| `rename_item(root, path, name) -> string` | Nouveau chemin relatif |
| `clone_item(root, path, name) -> string` | Chemin relatif de la copie |
| `delete_item(root, path) -> void` | Vers la corbeille |
| `move_item(root, path, target, position) -> string` | `target` = chemin relatif du frère (`before` / `after`) ou du dossier (`inside`, `""` = racine) ; renvoie le nouveau chemin relatif |

Les erreurs arrivent sous forme de chaînes en français. Chaque commande relit ensuite l'arbre côté interface (`ws.reload()`).

## 9. Interface utilisateur

- **Accueil et ouverture** : « Ouvrir un dossier » sur un dossier `empty` ou `other` propose « Créer une collection ici » (dialogue avec le nom). « Nouvelle collection… » (accueil, palette) demande un dossier parent et un nom.
- **Arbre** :
  - un bouton `+` dans l'en-tête de l'îlot Collections, avec le menu Nouvelle requête / Nouveau dossier ;
  - un menu contextuel sur chaque ligne (clic droit, ou touche menu / Maj+F10) : Nouvelle requête, Nouveau dossier (sur un dossier), Renommer (F2), Dupliquer (⌘D), Supprimer (⌘⌫ / Suppr), avec confirmation ;
  - le renommage se fait sur place, dans la ligne (Entrée valide, Échap annule) ;
  - la création ouvre une ligne de saisie sur place dans le dossier visé, puis la requête dans un onglet.
- **Glisser-déposer** :
  - indicateur de dépôt : un filet accent pour `before` / `after`, et la ligne du dossier sur `accent-soft` pour `inside` ;
  - refus visible pour un dépôt impossible ;
  - au clavier : ⌥↑ / ⌥↓ pour réordonner.
- **Onglets** : un onglet ouvert suit le renommage et le déplacement de son fichier (même onglet, même brouillon), et se ferme quand le fichier est supprimé.
- **Palette** : Nouvelle collection…, Nouvelle requête, Nouveau dossier, Renommer, Dupliquer et Supprimer l'élément actif.
- **Style** : le menu et les dialogues suivent DESIGN.md (surface flottante, rayon 9px, ombre unique) ; aucune couleur hors tokens.
