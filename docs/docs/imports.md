# Imports (V1, EF-IMP-01)

Chaque import produit la **collection JSON de Bruno** (celle que ses convertisseurs fabriquent), puis `xc-sync` l'écrit en OpenCollection YAML avec le même sérialiseur que l'import OpenAPI (`crates/sync/src/stringify`). Les fichiers sont donc identiques à ceux que Bruno écrirait, relus et réécrits sans diff par l'éditeur. Un import se construit dans un dossier de préparation caché, renommé à la fin : une collection n'est jamais écrite à moitié, et un nom déjà pris reçoit un suffixe (` - 1`…) au lieu d'écraser.

| Source | État | Entrée |
| --- | --- | --- |
| Spec OpenAPI 3.x, Swagger 2.0 | Fait | `xc import`, dialogue « Importer une spec OpenAPI » (voir `synchro-openapi.md`) |
| Commande cURL | Fait | « Nouvelle requête depuis cURL » |
| Collection Postman v2.0 et v2.1 | Fait | `xc import-postman`, dialogue « Importer depuis Postman » |
| Environnement Postman | Fait | `xc import-postman --environment`, même dialogue |
| Insomnia, collections `.bru` | À faire | |

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
