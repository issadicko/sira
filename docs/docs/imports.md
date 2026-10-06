# Imports (V1, EF-IMP-01)

Chaque import produit la **collection JSON de Bruno** (celle que ses convertisseurs fabriquent), puis `xc-sync` l'écrit en OpenCollection YAML avec le même sérialiseur que l'import OpenAPI (`crates/sync/src/stringify`). Les fichiers sont donc identiques à ceux que Bruno écrirait, relus et réécrits sans diff par l'éditeur. Un import se construit dans un dossier de préparation caché, renommé à la fin : une collection n'est jamais écrite à moitié, et un nom déjà pris reçoit un suffixe (` - 1`…) au lieu d'écraser.

| Source | État | Entrée |
| --- | --- | --- |
| Spec OpenAPI 3.x, Swagger 2.0 | Fait | `xc import`, dialogue « Importer une spec OpenAPI » (voir `synchro-openapi.md`) |
| Commande cURL | Fait | « Nouvelle requête depuis cURL » |
| Collection Postman v2.0 et v2.1 | Fait | `xc import-postman`, dialogue « Importer depuis Postman » |
| Environnement Postman | Fait | `xc import-postman --environment`, même dialogue |
| Export Insomnia v4 (JSON) et v5 (YAML), environnements compris | Fait | `xc import-insomnia`, dialogue « Importer depuis Insomnia » |
| Collections `.bru` (Bruno) | Fait | `xc import-bru`, dialogue « Convertir une collection .bru », ou ouverture d'un dossier `.bru` |

## Postman

Port de `postman-to-bruno.js` et de `postman-env-to-bruno-env.js` (convertisseurs de Bruno, commit `a2dbda9f`, comme l'étude de la synchro OpenAPI).

```bash
xc import-postman boutique.postman_collection.json ~/collections
xc import-postman --environment recette.postman_environment.json ~/collections/Boutique
```

La première commande crée `~/collections/Boutique/` et affiche son chemin ; la seconde ajoute `environments/Recette.yml` à la collection. Ce qui n'a pas pu être converti est listé sur la sortie d'erreur (élément ignoré ou avertissement) ; le code de sortie est 2 pour un fichier illisible ou un schéma non pris en charge.

### Ce qui est converti

- **Schéma** : collections v2.0 et v2.1 (aussi quand l'export les enveloppe dans `{ "collection": … }`). Un autre schéma est refusé.
- **Dossiers et requêtes** : l'arbre est gardé, dans l'ordre de l'export (`seq`). Deux éléments de même nom dans un dossier deviennent `Nom` et `Nom_1`, comme chez Bruno. Une requête sans méthode est écartée et signalée.
- **Adresse** : `url.raw` (sans fragment `#…`) ou, à défaut, reconstruite depuis `protocol`, `host`, `port`, `path` et `query`. Paramètres de requête (désactivés compris) et paramètres de chemin (`:nom`, `{{variable}}` de `url.variable`).
- **En-têtes** : liste d'objets, de textes `Clé: valeur`, ou texte unique multi-lignes ; les désactivés restent désactivés.
- **Corps** : `raw` (JSON, XML ou texte, d'après `options.raw.language`, à défaut d'après l'en-tête `Content-Type`), `urlencoded`, `formdata` (texte et fichiers, `contentType` conservé), `file`, `graphql` (la requête devient une requête GraphQL).
- **Auth** : `noauth`, `basic`, `bearer`, `apikey` (en-tête ou paramètre d'adresse), `digest`, `awsv4`, `oauth2` (client credentials, password, code d'autorisation avec ou sans PKCE, implicite, nom du jeton, placement du jeton, paramètres additionnels des trois requêtes), `ntlm`, `oauth1`, `edgegrid`. Une requête ou un dossier sans auth hérite ; la collection sans auth n'en a pas.
- **Variables de collection** : dans `opencollection.yml` ; un nom invalide est assaini (`page size` → `page_size`).
- **Scripts** : l'événement `prerequest` devient le script pré-requête, l'événement `test` le script post-réponse, aux trois niveaux (collection, dossier, requête).
- **Réponses enregistrées** : des exemples de la requête (requête d'origine, statut, en-têtes, corps).
- **Réglages** : `disableUrlEncoding`, `followRedirects`, `followAuthorizationHeader`, `maxRedirects` (un nombre négatif ou non numérique est écarté avec un avertissement), `disabledSystemHeaders`.
- **Description** : texte, ou objet `{ content, type }`.

### Environnement

Le fichier d'environnement de Postman (`values` : `key`, `value`, `enabled`, `type`) devient un fichier de `environments/` ; il n'en remplace jamais un existant (` 1` est ajouté au nom). Une variable de type `secret` est importée **sans sa valeur** (`secret: true`) : on la saisit ensuite dans le tableau de l'environnement, elle va dans le trousseau (voir `gestion-collection.md`).

### Scripts : la traduction

Les appels courants de `pm.*` (et du vieux `postman.*`) sont traduits en `bru.*` / `res.*` / `req.*` par la table de remplacements de Bruno (`postman-translations.js`) : variables d'environnement, de collection, globales, `pm.test`, `pm.expect`, `pm.response.*`, `pm.request.*`, `pm.setNextRequest`, cookies, `tests['…'] = …`. Bruno traduit d'abord par un arbre syntaxique et ne retombe sur cette table qu'en cas d'échec ; ici seule la table est portée, donc un script plus tordu (chaînes `pm.response.to.have…` complexes, `pm.sendRequest`, `pm.require`) peut garder des `pm.*` non traduits. Ils sont à relire après l'import ; rien n'est supprimé.

### Écarts avec Bruno

Aucun `uid` n'est produit (l'écriture n'en a pas besoin), les paquets `pm.require` ne sont pas inventoriés, et l'import ne crée pas de `.oc-sync/` (il n'y a pas de spec à synchroniser).

## Insomnia

Port de `insomnia-to-bruno.js` et de `env-utils.js` (convertisseurs de Bruno, même commit). Le format est reconnu tout seul : un export v5 (`type: collection.insomnia.rest/5.0`, en YAML) ou un export v4 (`resources`, en JSON ou en YAML).

```bash
xc import-insomnia boutique.insomnia.json ~/collections
```

La commande crée `~/collections/Boutique/` avec ses environnements et affiche son chemin ; ce qui n'a pas pu être converti est listé sur la sortie d'erreur, et le code de sortie est 2 pour un fichier illisible ou sans collection (`workspace`).

### Ce qui est converti

- **Arbre** : espaces de travail, dossiers (`request_group`) et requêtes de l'export v4 ; `collection` et `children` de l'export v5. Les éléments d'un dossier gardent l'ordre qu'Insomnia affiche (`metaSortKey` en v4, ordre de la liste en v5), sous-dossiers d'abord. Deux requêtes de même nom dans un dossier deviennent `Nom` et `Nom_1`. Un élément v5 qui n'est ni une requête (`method` et `url`) ni un dossier (`children`) est écarté et signalé.
- **Variables** : `{{ _.base_url }}` devient `{{base_url}}` (préfixe `_.` et espaces retirés) partout : adresse, en-têtes, paramètres, corps, auth.
- **Requêtes** : méthode, adresse, en-têtes, paramètres de requête et de chemin (désactivés compris). Le réglage « encoder l'URL » est repris (`settingEncodeUrl` en v4, `settings.encodeUrl` en v5).
- **Corps** : JSON, `x-www-form-urlencoded`, `multipart/form-data` (champs texte), texte, XML et GraphQL (la requête devient une requête GraphQL, ses variables sont conservées).
- **Auth** : `basic`, `bearer`, `digest`, `apikey` (en-tête ou paramètre d'adresse), `iam` (AWS Signature V4) et `oauth2` (client credentials, password, code d'autorisation avec ou sans PKCE, implicite). Les autres types (`hawk`, `ntlm`, `oauth1`, `netrc`, `asap`…) ne sont pas importés : la requête est créée sans auth et signalée. Une auth **désactivée** dans Insomnia reste sans effet après l'import.
- **Environnements** : un environnement est aplati en variables à clés pointées (`nested.list[0]`), les valeurs sont écrites en texte. L'environnement de base d'Insomnia devient un environnement ; chaque sous-environnement est fusionné sur lui (ses clés remplacent celles de la base, les autres sont héritées). Un environnement sans nom s'appelle `Environment N`.

### Écarts avec Bruno

- Bruno écrit **deux fois** les requêtes d'un dossier de l'export v4 (une fois dans le dossier, une fois à la racine) ; ici chaque requête n'apparaît qu'à sa place.
- Bruno garde l'ordre du tableau `resources` ; ici l'ordre est celui d'Insomnia.
- Bruno ne convertit que `basic` et `bearer` ; ici `digest`, `apikey`, `iam` et `oauth2` le sont aussi, et ce qui ne l'est pas est signalé au lieu de disparaître en silence.
- Un dossier garde son nom exact dans `folder.yml` même quand le nom du répertoire doit être assaini.
- Aucun `uid` n'est produit et l'import ne crée pas de `.oc-sync/`.

## Collections `.bru` (Bruno)

Un dossier qui contient `bruno.json` et pas d'`opencollection.yml` est une collection Bruno au format historique. Sira la lit et la **convertit en YAML dans un nouveau dossier** ; la source n'est jamais modifiée. C'est un port de bruno-lang v2 (`bruToJson.js`, `collectionBruToJson.js`, `envToJson.js`) et des fonctions `parseBruRequest`, `parseBruCollection` et `parseBruEnvironment` de bruno-filestore, suivi du même sérialiseur YAML que les autres imports.

```bash
xc import-bru ~/projets/boutique-bru ~/collections
```

La commande crée `~/collections/Boutique Bru/` et affiche son chemin ; ce qui n'a pas pu être converti est listé sur la sortie d'erreur. Dans l'application, ouvrir un dossier `.bru` propose la même conversion (palette de commandes : « Convertir une collection .bru… »).

### Ce qui est converti

- **`bruno.json`** : nom, version, `ignore`, préréglages (`presets`), scripts (`flow`, `additionalContextRoots`), proxy, certificats clients, protobuf, synchro OpenAPI : ils rejoignent `opencollection.yml`.
- **`collection.bru` et `folder.bru`** : en-têtes, auth, variables, scripts, tests et documentation de la collection et de chaque dossier ; le nom et le `seq` d'un dossier viennent de son `folder.bru`.
- **Requêtes `.bru`** : méthode et adresse (bloc `get`, `post`… ou `http`), paramètres de requête et de chemin, en-têtes, les corps (JSON, texte, XML, SPARQL, GraphQL avec ses variables, formulaire, multipart avec fichiers et `@contentType`, fichier), toutes les auths (Basic, Bearer, Digest, API Key, AWS Signature V4, OAuth 1, OAuth 2 avec ses paramètres additionnels, NTLM, WSSE, Akamai EdgeGrid), variables de pré-requête et de post-réponse (le préfixe `@` marque une variable locale, les annotations `@number`, `@boolean` et `@object` donnent une valeur typée), assertions, scripts, tests, documentation, réglages, étiquettes (`tags`) et exemples enregistrés.
- **`environments/*.bru`** : variables (désactivées comprises), `color`, `extends` et secrets externes. Les variables de `vars:secret` n'ont pas de valeur dans le fichier : elles sont importées **sans valeur**, comme pour Postman.
- **Ordre** : chaque requête garde son `seq`.

Les fins de ligne `\r\n`, les clés entre guillemets et les valeurs sur plusieurs lignes (`'''`) sont lues comme Bruno les lit.

### Ce qui n'est pas converti

- Les requêtes `grpc`, `ws` et `app` (Sira n'ouvre que les requêtes HTTP et GraphQL) : elles sont écartées et signalées.
- Un fichier illisible : écarté et signalé avec la ligne en cause. Bruno refuse tout le fichier ; ici, seul ce fichier est perdu, le reste de la collection est converti.
- Un bloc de nom inconnu (fichier écrit par une version plus récente de Bruno) : le bloc est sauté et signalé, la requête est gardée.
- Les fichiers qui ne sont pas des `.bru` (jeux de données, `.env`, images, scripts importés par `require`) : ils ne sont **pas copiés**, et un avertissement en donne le décompte par extension. Les dossiers `node_modules`, `.git` et ceux que `ignore` désigne sont ignorés ; les dossiers cachés aussi.

### Écarts avec Bruno

Les écarts se limitent à la tolérance : un bloc de texte vide (`body:json {` suivi directement de `}`) est lu comme vide, et un bloc inconnu est sauté et signalé. Un réglage `encodeUrl` absent du bloc `settings` vaut `false`, comme chez Bruno.
