# Gestion de collection — conception (MVP-δ)

1er octobre 2026 · exigences EF-COL-01, EF-COL-04, EF-SYN-01 · sources de Bruno au commit `a2dbda9f` (`packages/bruno-electron/src/ipc/collection.js`, noté E, `packages/bruno-app/src/providers/ReduxStore/slices/collections/actions.js`, noté A, `packages/bruno-electron/src/utils/filesystem.js`, noté FS)

Partir de zéro et organiser une collection comme dans Bruno.

- Les fichiers **créés** sont identiques à ceux de Bruno, ce que vérifient les fixtures produites par `tools/oracle`.
- Les fichiers **modifiés** ne le sont que sur la ligne concernée. Bruno, lui, re-sérialise tout le fichier et perd au passage les clés inconnues.
- **Écarts volontaires** : Bruno supprime définitivement et renomme de façon non atomique ; nous non (§ 7). Nous ne remplaçons jamais un élément existant et ne suivons ni ne réécrivons jamais un lien symbolique.

## 1. Ouvrir un dossier qui n'est pas une collection

`inspect_folder(path)` classe le dossier choisi :

| Classe | Condition | Interface |
| --- | --- | --- |
| `collection` | `opencollection.yml` présent | Ouverture normale |
| `empty` | Aucune entrée (hors fichiers cachés du système comme `.DS_Store`) | « Créer une collection ici » : nom proposé = nom du dossier |
| `bru` | `bruno.json` présent | Le dialogue de conversion s'ouvre (`imports.md`) : la collection est convertie en YAML dans un nouveau dossier |
| `other` | Autre contenu | « Créer une collection ici » avec un avertissement : le dossier n'est pas vide, rien d'existant n'est modifié |

Dans l'arbre, un fichier `.yml` n'est un élément que si son `info.type` est un type que Bruno lit : `http`, `graphql`, `grpc`, `websocket` (requêtes), `script` ou `app`. Comme dans Bruno, un autre fichier YAML (une spec OpenAPI rangée dans la collection, un `info.type` absent ou inconnu) reste visible mais en erreur, avec la raison. Il n'est ni ouvert comme requête, ni enregistré, ni compté parmi les requêtes, et la synchro ne l'apparie jamais.

## 2. Créer une collection (E:197-262)

Deux façons de créer :

- `create_collection(parent, name)` crée un **nouveau** dossier `sanitizeName(name)` sous `parent`, comme « Nouvelle collection » dans Bruno.
  - Si le dossier existe et est vide (hors fichiers cachés du système, comme § 1), il est réutilisé.
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

- `seq` = **fin de liste** du dossier parent : au-delà du plus grand `seq` et du nombre de dossiers et de requêtes qu'il contient, ce qui ne dépend pas des trous laissés par les suppressions. Écart voulu avec Bruno, qui prend le nombre de frères plus 1 et donne des `seq` en double après une suppression (§ 7). Les requêtes créées depuis une commande cURL (MVP-β) gardent le nombre de frères plus 1.
- Noms refusés : `collection` et `folder`, et ceux de `extensions.bruno.ignore` de `opencollection.yml`, que l'arbre ne montrerait pas (sans tenir compte de la casse ni de la normalisation Unicode).
- Si le fichier existe déjà, on suffixe ` 1`, ` 2`… sur le nom de fichier seulement, avec une création exclusive. `info.name` garde le nom saisi.
- Les noms pris se comparent après normalisation Unicode NFC et repli de casse (`unicode-normalization`) : `été.yml` écrit en NFD est le même nom que `été.yml` en NFC sur APFS et HFS+, et deux noms qui ne diffèrent que par la casse se confondent sur un volume insensible à la casse.

**Dossier** (E:1254-1274) :

- `sanitizeName(nom)` dans le dossier parent, avec un suffixe ` n` si le nom est pris.
- `folder.yml` est écrit tout de suite par `stringify::folder({meta:{name, seq}, request:{auth:{mode:'inherit'}}})`, avec le `seq` de fin de liste :

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
  - tout nom que l'arbre masquerait (`xc_core::collection::is_hidden`) ;
  - tout nom de `extensions.bruno.ignore`.

## 4. Renommer, dupliquer, supprimer

Règles communes à toutes les actions de ce paragraphe et du suivant :

- **Seuls une requête et un dossier sont réécrits.** Une requête est un fichier dont `info.type` est `http`, `graphql`, `grpc` ou `websocket` ; un dossier se réécrit par son `folder.yml`. Une spec OpenAPI, un fichier illisible, un flux de plusieurs documents YAML ou un fichier dont `info` n'est pas une table ne sont jamais réécrits : `with_info` les refuse (erreur, rien n'est perdu) et les actions les laissent tels quels.
- **Un lien symbolique n'est jamais suivi ni réécrit.** Renommer, déplacer ou dupliquer une requête qui est un lien (même interne), ou un dossier dont `folder.yml` est un lien, est refusé avec un message clair, et aucun chemin ne traverse un dossier lien symbolique ; `opencollection.yml` lien est refusé par `ignore_name`. L'arbre ne lit pas un `folder.yml` dont la cible sort de la collection. `write_atomic` prend la racine et refuse tout fichier ou dossier-lien qui sort d'elle, quel que soit l'appelant ; la synchro (MVP-γ) continue d'écrire la cible d'un lien **interne**. Supprimer un lien envoie le lien, pas sa cible, à la corbeille.
- **Rien n'est jamais remplacé.** Les noms pris se comparent après NFC et repli de casse (§ 3). Le renommage lui-même ne remplace pas : `renamex_np(RENAME_EXCL)` sur macOS, `renameat2(RENAME_NOREPLACE)` sur Linux, `MoveFileExW` sans `MOVEFILE_REPLACE_EXISTING` sur Windows. Si la cible existe, y compris quand elle est apparue entre le choix du nom et le renommage, le suffixe ` n` suivant est essayé. Repli documenté : sur un volume qui ne sait pas renommer sans remplacer, la cible est vérifiée puis le renommage fait, et la course entre les deux reste ouverte. Un renommage qui ne change que la casse ou la normalisation du nom est permis (la cible est alors l'élément lui-même).
- **Pas d'écrasement d'une mise à jour concurrente.** Chaque fichier est relu juste avant d'être écrit ; s'il a changé depuis la lecture initiale, l'action est annulée sans rien écrire (« a changé depuis sa lecture »).
- **Le BOM et les fins de ligne CRLF d'un fichier sont conservés** ; seule la ligne visée change. Un `folder.yml` absent est créé en création exclusive.

**Renommer** une requête change à la fois le nom affiché et le nom du fichier, comme Bruno quand le nom de fichier suit le nom affiché.

- Seule la ligne `info.name` est modifiée, par `xc_core::collection::with_info` (`RequestDoc::apply` n'écrit pas `info.seq` et ne sert pas à un `folder.yml`). L'arbre YAML est conservé, clés inconnues comprises, puis réécrit par l'émetteur : un fichier tel que Bruno ou l'application l'écrit ne change qu'à cette ligne, mais les commentaires et la mise en forme d'un fichier écrit à la main sont normalisés, comme à chaque enregistrement.
- Le fichier est ensuite renommé sans jamais remplacer (règle ci-dessus), avec un suffixe ` n` si le nouveau nom est pris.
- Si le nom de fichier assaini est inchangé, seul `info.name` change.
- Un fichier illisible ou qui n'est pas une requête est renommé sans que son contenu soit touché ; l'arbre l'affiche alors sous le nom de son fichier (ou sous son `info.name` s'il en a un).
- Pour un dossier : `info.name` de `folder.yml` (fichier créé, minimal, s'il manque, comme E:1188-1207), puis renommage du dossier. Un `folder.yml` illisible n'est pas touché : le dossier est renommé et l'arbre l'affiche sous le nom de son dossier.
- Un nom de `extensions.bruno.ignore` est refusé comme un nom caché.

**Dupliquer** une requête :

- copie à l'octet, puis seules les lignes `info.name` et `info.seq` changent ; un fichier qui n'est pas une requête est copié tel quel, sans changer une ligne ;
- nom proposé : « <nom> copie » ;
- `seq` = fin de liste du dossier : au-delà du plus grand `seq` et du nombre de frères, ce qui ne dépend pas des trous laissés par les suppressions ;
- même dossier, nom de fichier suffixé si besoin.

Pour un dossier : copie complète de l'arborescence, sans perte de fichier (Bruno ne recopie que les requêtes http/graphql/grpc), puis seules `info.name` et `info.seq` du `folder.yml` de la copie changent. Les fichiers sont copiés en création exclusive et seul ce que la copie a créé est supprimé en cas d'échec, jamais une destination qui existait déjà. Un lien symbolique dans le dossier fait refuser la copie, qui ne lit jamais hors de la collection et ne perd rien en silence ; la copie incomplète est supprimée.

**Supprimer** une requête ou un dossier :

- toujours vers la **corbeille du système**, après confirmation dans l'interface ;
- `source.yml` est lu avant la corbeille et écrit aussitôt après ; si cette écriture échoue, l'erreur dit que l'élément est bien dans la corbeille ;
- les frères ne sont pas renumérotés, les trous de `seq` sont tolérés par le tri ;
- les onglets ouverts sur l'élément supprimé se ferment ;
- sur macOS, par `NSFileManager` (`trashItemAtURL`) plutôt que par le Finder : aucune autorisation « contrôler le Finder » n'est demandée, un refus ne peut pas bloquer la suppression ; l'élément se récupère en le faisant glisser hors de la corbeille (le Finder ne propose pas toujours « Remettre »).

## 5. Réordonner et déplacer (glisser-déposer)

Position de dépôt : `before`, `after` (d'un frère) ou `inside` (d'un dossier).

- **Réordonner dans un dossier** (U:1504-1600) :
  1. On trie les frères (dossiers et requêtes confondus) par `sortByNameThenSequence`.
  2. On déplace l'élément.
  3. On renumérote de 1 à n.
  4. On écrit seulement les `seq` qui changent, toujours y compris celui de l'élément déplacé : c'est un écart voulu, Bruno peut laisser deux `seq` égaux. Seule la ligne `info.seq` change (requête ou `folder.yml`).
  5. Un frère qui ne peut pas être réécrit (YAML invalide, fichier qui n'est pas une requête, lien symbolique) garde son `seq` et sa place dans la numérotation : il ne fait pas échouer l'action.
- **Déplacer vers un autre dossier** :
  - renommage atomique du fichier ou du dossier entier, sans jamais remplacer (§ 4) ;
  - copie, vérification **du contenu octet par octet** (pas seulement des noms et des tailles), puis suppression de la source seulement si les volumes diffèrent ; en cas d'échec, seul ce que la copie a créé est supprimé, jamais une destination qui existait déjà ;
  - l'élément **garde son nom de fichier** s'il est libre dans le dossier cible (`aux.yml` reste `aux.yml`), sinon le suffixe ` n` le distingue ;
  - `inside` : `seq` en fin de liste, comme pour la copie (au-delà du plus grand `seq` et du nombre de frères), sans renuméroter les frères ; déposer dans son propre dossier met l'élément à la fin ;
  - `before` / `after` : renumérotation du dossier cible comme ci-dessus.
- **Ordre des écritures** : d'abord les `seq` (sur place, avant de renommer, relus un à un juste avant d'être écrits), puis le renommage, en dernier, puis `source.yml` aussitôt. Si le renommage échoue, l'élément reste où il était (son `seq` a pu changer) et `source.yml` n'a pas bougé.
- **Déplacement sans effet** : un dépôt qui ne change ni le dossier ni la position (par exemple `before` du frère suivant) n'écrit rien et renvoie le chemin inchangé.
- Déposer un dossier dans lui-même ou dans un de ses descendants est refusé.
- Un fichier qui n'est pas une requête n'a pas de `seq` : changer sa position dans son dossier est refusé, le déplacer vers un autre dossier reste permis.
- Chaque écriture est synchronisée sur le disque, fichier par fichier (`fsync`) : un réordonnancement de plusieurs centaines de frères y passe l'essentiel de son temps, écart de vitesse assumé au profit de la sûreté.

## 6. Synchro OpenAPI (EF-SYN-01)

Si `.oc-sync/openapi/source.yml` existe, chaque action le met à jour dans la même opération, en dernier et de façon atomique :

- **Renommer ou déplacer une requête** : l'entrée dont le `file` correspond prend le nouveau chemin. Les chemins se comparent exactement, sans repli de casse : ce sont les noms réels que l'import, la synchro et ces actions écrivent, et deux dossiers qui ne diffèrent que par la casse sont distincts.
- **Renommer ou déplacer un dossier** : toutes les entrées dont le `file` commence par l'ancien préfixe.
- **Dupliquer** : la copie n'est pas suivie.
- **Supprimer une requête suivie** : son entrée passe à `ignored: true`, sans `file`, car l'équipe ne la veut plus. Supprimer un dossier fait de même pour toutes ses entrées.

`source.yml` est lu avant l'action : un fichier illisible la refuse avant toute écriture. Si son écriture échoue après le renommage, le déplacement ou la corbeille, l'erreur le dit (« l'élément est bien renommé, déplacé ou dans la corbeille, mais `source.yml` n'a pas pu être mis à jour ») ; la synchro reprend alors le fichier déplacé s'il est le seul candidat de même méthode et de même chemin normalisé.

En plus, connecter une collection à une spec (vue de synchro, première synchro) ajoute `.oc-sync` à `extensions.bruno.ignore` de `opencollection.yml`, comme l'import, pour que Bruno ne l'affiche pas. Seule cette liste change (`xc_core::collection::ignore_name`, appelée par `Plan::apply` avant l'écriture de `source.yml`, sans effet quand `.oc-sync` y figure déjà, refusée quand `opencollection.yml` est un lien symbolique).

## 7. Écarts volontaires avec Bruno

| Bruno | Nous | Raison |
| --- | --- | --- |
| Suppression définitive (`rmSync`, `unlinkSync`) | Corbeille du système | Principe 5, non destructif |
| Renommer : `unlink` puis `writeFile`, re-sérialisation complète (clés inconnues perdues) | Renommage atomique, seule `info.name` change | Non destructif, diff Git minimal |
| Réordonner : re-sérialisation complète, brouillon inclus | Seule `info.seq` change ; le brouillon non enregistré reste un brouillon | Idem |
| Le `seq` de l'élément déplacé n'est pas toujours réécrit | Toujours réécrit | Évite les `seq` en double |
| Nouvelle requête ou nouveau dossier : `seq` = nombre de frères + 1 | `seq` de fin de liste : au-delà du plus grand `seq` et du nombre de frères | Évite les `seq` en double quand une suppression a laissé un trou |
| Dupliquer un dossier perd les fichiers non-requêtes | Copie complète | Non destructif |
| Renumérotation des frères après suppression | Aucune | Diff minimal |

Décisions de sûreté propres à l'application, sans équivalent dans Bruno :

- aucune action ne remplace un élément existant (renommage exclusif, comparaison des noms par NFC et repli de casse) ;
- aucun lien symbolique n'est suivi, renommé, déplacé, dupliqué ni réécrit, et rien n'est écrit hors de la collection ;
- seuls une requête (`info.type` connu) et un `folder.yml` sont réécrits, le reste (spec OpenAPI, YAML illisible) est laissé tel quel ;
- les noms de `extensions.bruno.ignore` sont refusés comme des noms cachés ;
- un fichier modifié entre sa lecture et son écriture annule l'action ;
- une seule écriture à la fois dans l'application (§ 8).

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

Règles communes :

- `root` doit contenir `opencollection.yml` : sinon la commande est refusée avant toute lecture ou écriture.
- Les chemins relatifs sont canoniques : segments non vides, ni `.` ni `..`, ni `\`, pas de `/` final ; un chemin qui sort de la collection, qui est caché ou réservé (même prédicat de casse que l'arbre, `is_hidden`), qui figure dans `extensions.bruno.ignore` ou qui désigne, par son identité de fichier (alias Unicode, nom 8.3, point ou espace final sous Windows), `opencollection.yml`, `environments`, `mocks` ou `.oc-sync`, est refusé. Les chemins renvoyés sont toujours canoniques.
- **Une écriture à la fois** : un verrou (`tokio::sync::Mutex` de l'état Tauri) est pris par toutes les commandes qui écrivent dans une collection, c'est-à-dire celles ci-dessus sauf `inspect_folder`, plus `save_request`, `sync_apply`, `create_request_from_curl` et `import_openapi`. Deux glisser-déposer ou une duplication pendant un enregistrement ne lisent plus les mêmes `seq` ni les mêmes noms libres.
- Les erreurs arrivent sous forme de chaînes en français, y compris les erreurs du système (introuvable, permission refusée, existe déjà, dossier non vide, volume en lecture seule ou plein…) et celles de la corbeille.

Chaque commande relit ensuite l'arbre côté interface (`ws.reload()`).

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

## 10. Changements extérieurs : rechargement à chaud (EF-COL-03)

Un `git pull`, un `git checkout` ou un éditeur externe écrit dans la collection pendant qu'elle est ouverte. L'application le voit et se remet à jour sans que l'utilisateur ne fasse rien, sans jamais perdre un brouillon.

**Surveillance (`crates/watch`).**

- `watch_collection(root)` surveille le dossier de la collection, récursivement, et remplace la surveillance précédente. Elle est démarrée à chaque ouverture ; son échec (limite de fichiers surveillés du système, dossier illisible) est annoncé sans bloquer l'ouverture.
- Les événements bruts sont regroupés en **lots** : un lot se clôt après 200 ms de silence, ou au bout de 2 s si les écritures ne s'arrêtent pas. Un lot est annoncé à l'interface par l'événement `collection-changed` : `{ root, paths, truncated }`, chemins relatifs séparés par `/`, triés, sans doublon.
- Au-delà de 512 chemins, ou si le système perd des événements, ou si le dossier de la collection disparaît, le lot est `truncated` : tout a pu changer, l'interface relit tout.
- Ne déclenchent rien : les lectures (accès), `.git/`, `.oc-sync/`, `node_modules/`, les fichiers temporaires (`*~`, `*.swp`, `*.tmp`, dont les `.xc-*.tmp` de notre écriture atomique, `.#*`, `.DS_Store`). Les chemins sont comparés au dossier canonique (liens symboliques de `/var` sous macOS, préfixe `\\?\` sous Windows). Les liens symboliques ne sont pas suivis.
- Seuls réveillent l'interface : les fichiers `*.yml` / `*.yaml`, les `.env*`, et la création ou la suppression d'un dossier. Une modification de contenu ou de métadonnées d'un chemin sans extension (un dossier qui change de date à chaque écriture d'un fichier) est ignorée.
- Aucune suppression d'écho pour nos propres écritures : relire un fichier qu'on vient d'écrire redonne exactement le même document (test `ef_col_03_a_saved_request_reads_back_identical…`). L'interface ne compare cependant jamais le disque à son propre document, mais à ce que Rust a lu (voir ci-dessous).

**Interface (`Workspace.onDiskChange`).** Un seul traitement à la fois ; les lots reçus entre-temps sont réunis. Le traitement attend la fin d'un enregistrement ou d'une commande de collection en cours, puis :

1. relit l'arbre (`reload`) et ferme les onglets propres dont le fichier a disparu ; ceux qui ont un brouillon restent, marqués « introuvable » ;
2. relit les onglets dont le fichier a changé, ou situé sous un dossier annoncé (tous si le lot est tronqué), et décide, pour chacun, d'après quatre versions : `base` (le fichier tel que Rust l'a lu pour la dernière fois, à l'ouverture ou en relecture après un enregistrement), `saved` (le document de l'interface au dernier enregistrement), le brouillon, le disque. Le disque se compare à `base`, jamais à `saved` : le document de l'interface (valeurs `null`, ordre des clés) ne se sérialise pas comme celui de Rust, et la comparaison verrait un changement extérieur à chaque enregistrement :
   - le disque égale `base` : rien (c'est notre écriture, ou le fichier est revenu à ce qu'on a) ; l'éventuel état « périmé » disparaît ;
   - le disque diffère et le brouillon est identique à `saved` (rien d'écrit depuis l'enregistrement), ou identique au disque (un collègue a fait la même modification) : l'onglet **adopte** le disque ;
   - le disque diffère et le brouillon diffère des deux : l'onglet est **périmé**, le brouillon est gardé ;
3. relit les variables de l'environnement actif si `environments` (le dossier ou un de ses fichiers) ou `.env` ont changé, et rend un environnement disparu à l'environnement par défaut ;
4. dit en une phrase ce qui mérite d'être su (onglets relus, fermés ou périmés) ; rien si rien n'a changé pour l'utilisateur.

**Onglet périmé.** Fil d'Ariane : « Modifié sur le disque » en `info` et bouton **Recharger** ; point de l'onglet en `info`. `⌘S` demande « Écraser le fichier » parce que `save_request` applique le document entier sur le fichier : sans cela, un champ que l'on n'a pas touché reprendrait sa valeur d'avant le changement du disque. **Recharger** remplace le brouillon par le fichier, après confirmation. La synchro OpenAPI applique la même règle : un onglet modifié dont le fichier est réécrit par la synchro devient périmé au lieu d'être écrasé à l'enregistrement.

**Enregistrer, sans se fier à la seule surveillance.**

- Avant d'écrire, `save()` relit le fichier et le compare à `base` : si la surveillance a manqué un changement (lot perdu, fenêtre en veille), la confirmation « Écraser le fichier » est demandée quand même.
- Après l'écriture, le fichier est relu et devient la nouvelle `base`.
- Un compteur d'écritures en cours et un numéro de séquence empêchent qu'une relecture lancée avant une écriture ne l'emporte sur elle ; le traitement d'un lot attend la fin des écritures.

**Rattraper ce que la surveillance n'a pas vu.** Au retour de la fenêtre au premier plan (au plus toutes les 3 s), l'interface relit tous les onglets ouverts. Si un lot est `truncated`, la surveillance est ré-armée (au plus toutes les 5 s) avant la relecture.

**Limites assumées.**

- Pas de fusion à trois voies entre un brouillon et le disque : le choix est entre garder le brouillon et recharger (V2).
- Les changements de `.oc-sync/` ne sont pas annoncés ; l'écran de synchro se relit à son ouverture.
- Sous Linux, inotify pose une surveillance par dossier, y compris sous `.git/` et `node_modules/` que l'on ignore ensuite : une très grosse arborescence peut atteindre la limite du système (`ENOSPC`). Le message « le système a atteint sa limite de fichiers surveillés » est alors affiché, sans bloquer l'ouverture, et « Relire le dossier » (↻) reste disponible.
- Les limites de `notify` s'appliquent : sous Windows, le tampon du système (16 Kio) peut déborder sans qu'une relecture complète soit signalée, d'où la relecture au retour de la fenêtre ; sous macOS, FSEvents peut perdre des événements en rafale, d'où les lots `truncated`.


## 11. Environnements : création, édition, renommage, suppression (EF-VAR-01)

Un environnement est un fichier `environments/<nom>.yml`. Le nom de l'environnement est le nom du fichier sans `.yml`.

**Commandes IPC.**

| Commande | Rôle |
| --- | --- |
| `save_environment(root, name, vars, create) -> bool` | Enregistre les variables ; un fichier absent est refusé, sauf avec `create` (l'interface ne le demande que pour un fichier qu'elle sait disparu) ; `false` : rien n'a changé |
| `create_environment(root, name) -> string` | Crée `name.yml` (juste `name:`), crée `environments/` s'il manque ; renvoie le nom retenu |
| `rename_environment(root, from, name) -> string` | Renomme le fichier, met à jour sa clé `name` et suit l'environnement par défaut ; les deux suites sont tentées même si l'une échoue, et l'erreur dit que le renommage est fait |
| `clone_environment(root, from, name) -> string` | Copie le fichier sous un autre nom |
| `delete_environment(root, name) -> void` | Vers la corbeille ; retire l'environnement par défaut s'il le nommait |
| `set_default_environment(root, name \| null) -> void` | `extensions.bruno.presets.defaultEnvironment` de `opencollection.yml` |

Elles prennent le même verrou d'écriture que les commandes de l'arbre (§ 8).

**Noms.** Ceux d'une requête : caractères interdits remplacés par `-`, ni caché, ni nom de périphérique Windows, ni plus de 255 caractères. Un nom pris reçoit ` 1`, ` 2`… sans jamais remplacer un fichier, la casse ne distinguant pas deux noms. Le nom reçu par `read_environment` et `save_environment` est un nom simple, jamais un chemin (`..`, `/`, `\`, point de tête refusés). Seuls les fichiers `*.yml` de `environments/` dont le nom est utilisable sont listés (ni point de tête, ni dossier). Un environnement lien symbolique n'est ni renommé, ni copié ; un `environments/` lien symbolique n'est traversé par aucune écriture (enregistrer, créer, renommer, dupliquer, supprimer), seule la lecture le suit. Comme pour une requête, enregistrer à travers un lien symbolique de fichier écrit sa cible et laisse le lien en place, si la cible reste dans la collection.

**Fichier fidèle.** Enregistrer ne change que ce qui a changé. Chaque variable reprend sa table du fichier, repérée par son nom : les clés inconnues, l'ordre, la forme des valeurs, `extends`, `color` et `externalSecrets` sont conservés, ainsi que le BOM et les fins de ligne CRLF. Rien n'est écrit, ni normalisé, quand les variables n'ont pas changé. Dès qu'une écriture a lieu (enregistrement, renommage, copie), le fichier est réémis par l'émetteur : les commentaires et la mise en forme d'un fichier écrit à la main sont normalisés, comme pour une requête (§ 7). Une variable renommée garde sa description et son type ; une valeur typée (`number`, `boolean`…) garde son type quand son texte est modifié. Retirer toutes les variables retire la clé `variables`, comme Bruno.

**Secrets (ENF-SEC-01, EF-VAR-03).** Une variable `secret: true` n'a jamais de valeur dans le fichier, même si l'appelant en fournit une. Sa valeur se saisit dans le tableau (cadenas de la ligne, champ mot de passe) et vit dans le trousseau du système (Keychain, Gestionnaire d'identifiants, Secret Service), sous la clé *chemin réel de la collection / environnement / nom* : elle reste sur cette machine et ne suit pas un dossier copié. L'interface ne relit jamais cette valeur : elle sait seulement qu'une valeur est gardée (`secret_names`), et la résolution des variables l'affiche masquée. Enregistrer range une valeur saisie, efface une valeur vidée explicitement (corbeille de la ligne) et laisse intacte celle qu'on n'a pas touchée ; un secret retiré ou redevenu ordinaire est oublié du trousseau, un environnement renommé ou dupliqué emporte ses secrets, un environnement supprimé les oublie. Sans trousseau (Linux sans Secret Service, par exemple), enregistrer une valeur de secret échoue avant que le fichier ne soit touché, avec la raison ; tout le reste continue, et `{{process.env.NOM}}` lit le fichier `.env` de la collection. À l'envoi, les secrets de l'environnement choisi sont lus dans le trousseau et entrent dans la résolution comme les autres variables (requêtes, scripts, runner) ; un secret sans valeur est signalé comme variable non résolue. Une valeur qu'un fichier écrit à la main porterait déjà sur un secret n'est ni lue, ni effacée, ni affichée.

**Interface.**

- **Barre latérale** : la liste des environnements, un bouton `+`, un menu contextuel par ligne (clic droit, touche menu, Maj+F10) : Renommer…, Dupliquer…, Ouvrir par défaut / Ne plus ouvrir par défaut, Supprimer…. L'environnement qui s'ouvre par défaut porte « par défaut ».
- **Zone éditeur** : un tableau de variables (case d'activation, nom, valeur, suppression, ligne fantôme pour en ajouter une), avec l'état du fichier (« Enregistré sur le disque », « Non enregistré », « Modifié sur le disque », « Fichier introuvable »), Enregistrer (⌘S), Annuler les modifications et le même menu que la barre latérale. Un point sur l'onglet et sur le bouton Environnements de la barre d'activité signale un brouillon.
- **Contrôles** : une variable sans nom, ou dont le nom est déjà pris plus haut, empêche d'enregistrer ; un nom nouveau n'accepte que lettres, chiffres, `_`, `-` et `.` comme dans Bruno. Un fichier écrit à la main garde ses noms et ses doublons : les corriger détruirait une donnée. L'aperçu du nom de fichier suit le moteur (sans `.yml`, ni point de tête).
- **Palette** : Nouvel environnement…, Renommer, Dupliquer, Supprimer l'environnement actif, Ouvrir l'environnement actif par défaut.

**Brouillon.** L'édition travaille sur un brouillon qui n'agit sur aucune requête tant qu'il n'est pas enregistré : l'envoi et les variables lisent le disque. Comme pour un onglet (§ 10), le disque se compare à ce que Rust a lu (`base`), jamais au document de l'interface : un fichier changé sans brouillon est adopté, avec un brouillon l'environnement passe à « Modifié sur le disque » (Recharger), et enregistrer redemande confirmation, après avoir relu le fichier avant et après la confirmation (un changement survenu pendant la boîte l'annule). Un fichier qui ne se relit pas (supprimé sans que la surveillance l'ait annoncé, YAML invalide) n'est jamais écrasé ni recréé : l'enregistrement est refusé avec la raison. Un fichier supprimé avec un brouillon reste affiché, « Fichier introuvable » : enregistrer le recrée. Changer d'environnement actif, ou ouvrir une autre collection, avec un brouillon demande confirmation ; renommer l'environnement garde le brouillon.

**Fichier illisible.** Un environnement dont le YAML devient invalide garde sa dernière lecture à l'écran, avec un bandeau qui donne la raison.

**Limites assumées.** Renommer une variable secrète en perd la valeur (le trousseau l'oublie avec l'ancien nom : à ressaisir). `xc run` ne lit pas le trousseau : en CI, les secrets passent par `--env-var nom=valeur` ou l'environnement du processus. Pas de fusion entre un brouillon et le disque (V2).

**Mode démo.** Les mêmes règles s'appliquent à la collection gardée en mémoire (`demo-env.ts`).

## Historique local (EF-UX-02)

Chaque envoi est gardé dans l'historique de la collection : requête, méthode, environnement, code de réponse (ou `ERR` quand aucune réponse n'est venue), durée, taille et heure. Il est affiché dans la barre latérale (les 5 derniers, « Voir tout » jusqu'à 50), un clic rouvre la requête, la corbeille l'efface. Les 200 derniers sont conservés.

- **Hors de la collection** : le fichier vit dans le dossier de données de l'application (`history/<empreinte de la racine>.json`), jamais dans la collection, qui se versionne et se partage.
- **Pas de secret** : l'adresse gardée est celle saisie, `{{variables}}` non résolues ; ni valeur de variable, ni en-tête, ni corps n'y sont écrits.
- Une requête renommée ou supprimée disparaît de la liste (il n'y a plus de fichier à ouvrir). Un envoi ignoré par un script n'est pas gardé.
