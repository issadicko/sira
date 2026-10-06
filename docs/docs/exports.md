# Exports (V1, EF-IMP-03)

Une collection s'exporte en **Postman v2.1** ou en **OpenAPI 3.0.3**, toujours en JSON. L'export ne modifie jamais la collection : il la lit, construit le document en mémoire, puis l'écrit dans un fichier. Ce que le format ne sait pas porter n'est pas inventé : c'est écarté et listé (un message par élément).

| Format | État | Entrée |
| --- | --- | --- |
| Postman v2.1 | Fait | `xc export --format postman`, commande « Exporter la collection… » |
| OpenAPI 3.0.3 | Fait | `xc export --format openapi`, même commande |
| cURL | Fait | `xc code --lang curl` et « Générer du code » (voir `generation-de-code.md`) |

```bash
xc export ~/collections/Boutique --format postman --output boutique.postman_collection.json
xc export ~/collections/Boutique --format openapi > boutique.openapi.json
```

Sans `--output`, le document est écrit sur la sortie standard. Avec `--output`, un fichier existant n'est **jamais remplacé** sans `--force`, et un lien symbolique n'est jamais suivi (code de sortie 2). Les éléments écartés sont listés sur la sortie d'erreur (`avertissement : …`) ; l'export réussit quand même. Dans l'application, la commande de la palette choisit le format, demande le fichier (la boîte d'enregistrement du système confirme un remplacement) et affiche à la fin ce qui n'a pas été exporté.

## Postman

Port de `bruno-to-postman.js` (convertisseur de Bruno, même commit que l'import).

- **Arbre** : les dossiers d'abord, triés par nom puis par `seq` comme Bruno (`sortByNameThenSequence`), les requêtes ensuite, par `seq`.
- **Requête** : méthode, en-têtes (les désactivés restent désactivés), adresse découpée comme `transformUrl` de Bruno (schéma, hôte, chemin, paramètres de requête et de chemin), description, corps, authentification, événements.
- **Corps** : JSON, XML et texte en `raw` (avec leur langage), `urlencoded`, `formdata` (texte et fichiers, `contentType` gardé), GraphQL. Un corps sur `GET`, `HEAD` ou `OPTIONS` porte `disableBodyPruning`, pour que Postman l'envoie.
- **Authentification** : Bearer, Basic, API Key, Digest, AWS Signature V4, OAuth 2 (client credentials, password, code d'autorisation avec ou sans PKCE, implicite, paramètres additionnels des étapes). Une requête qui hérite n'écrit rien ; le niveau collection et le niveau dossier écrivent la leur.
- **Scripts** : le script pré-requête devient l'événement `prerequest`, le script post-réponse suivi des tests l'événement `test`, aux trois niveaux. Les appels courants de `bru.*`, `req.*` et `res.*` sont traduits en `pm.*` (variables, `test`, `expect`, `res.status`, `res.body`, `bru.sendRequest`, `bru.cookies.jar`…). Un script plus tordu garde ses appels `bru.*`, à relire dans Postman.
- **Variables** : celles de `opencollection.yml`, puis chaque `{{nom}}` rencontré dans l'export, sans doublon.
- **Écarté et signalé** : les requêtes qui ne sont ni HTTP ni GraphQL (gRPC, WebSocket), les types d'authentification que Postman ne connaît pas ici (NTLM, OAuth 1, EdgeGrid, WSSE).
- **Pas exporté** : les exemples de réponse et les environnements (aucun secret n'est donc écrit depuis eux).

Les valeurs écrites **en clair** dans une requête (un jeton collé dans le champ Bearer, un mot de passe Basic) sont exportées telles quelles : c'est ce que le fichier contient. Utilise des variables (`{{token}}`) pour qu'elles n'y soient pas.

### Écarts avec Bruno

L'adresse sans schéma (`{{baseUrl}}/users?x=1`) garde sa requête dans le dernier segment du chemin, et une barre finale seule reste dans l'hôte : c'est le comportement de `transformUrl`, reproduit tel quel pour qu'un export de Sira et un export de Bruno donnent le même fichier. La table de traduction des scripts est une table de remplacements, pas l'arbre syntaxique que Bruno parcourt.

## OpenAPI

Bruno n'a pas d'export OpenAPI : la forme est celle de Sira, pensée pour que l'import OpenAPI relise le document (aller-retour testé).

- **`info`** : nom de la collection, documentation, version (`info.version` du fichier de collection, `1.0.0` à défaut).
- **`servers`** : le début de chaque adresse est un serveur, soit une variable (`{{baseUrl}}`), soit l'origine écrite en clair (`https://api.example.com:8443`, `localhost:3000` devient `http://localhost:3000`). Les valeurs d'une variable dans l'environnement ouvert par défaut, les autres environnements puis la collection sont autant de serveurs, chacun décrit par le nom de son environnement. Les secrets n'ont pas de valeur dans les fichiers : un serveur dont la variable n'a aucune valeur reste `{nom}`, avec une variable de serveur au défaut vide, et l'export le signale. Une variable à l'intérieur d'une origine (`https://{{tenant}}.example.com`) devient une variable de serveur. Le début le plus employé est global ; les opérations qui en ont un autre portent leurs propres `servers`.
- **`paths`** : `:id` (quand la requête déclare ce paramètre de chemin) et `{{id}}` deviennent `{id}`. L'opération a pour `summary` le nom de la requête, pour `description` sa documentation, pour `tags` le chemin de son dossier (`Users / Admin`) ; seuls les dossiers qui portent une opération sont déclarés dans `tags`, avec leur documentation. L'`operationId` est le nom en camelCase, unique (`_2`, `_3`…).
- **Paramètres** : de chemin (toujours requis), de requête et d'en-tête, activés seulement ; leur type (entier, nombre, booléen, texte) et leur exemple viennent de la valeur écrite. `Accept`, `Content-Type` et `Authorization` ne sont pas des paramètres (OpenAPI les ignore).
- **Corps** : JSON (exemple et schéma déduit de l'exemple ; un JSON illisible, avec une variable nue, est gardé tel quel en exemple et signalé), XML, texte, `application/x-www-form-urlencoded`, `multipart/form-data` (les fichiers en `binary`, `contentType` en `encoding`), GraphQL (`query` et `variables`). Un corps sur `GET`, `HEAD`, `OPTIONS` ou `TRACE` n'est pas décrit par OpenAPI 3.0 : il est écarté et signalé.
- **Réponses** : le statut de l'assertion `res.status eq N` de la requête, `200` à défaut.
- **Sécurité** : l'authentification **effective** de chaque requête (héritage du dossier et de la collection compris). Elle est déclarée dans `components.securitySchemes` — `bearerAuth`, `basicAuth`, `digestAuth`, `apiKey_<nom>`, `awsSigV4`, `oauth2<Flux>` — et le niveau de la collection devient `security` ; une opération ne répète la sienne que si elle diffère (aucune authentification : `security: []`). Deux configurations différentes ne partagent jamais un nom (`_2`). Les adresses OAuth 2 résolvent les variables connues.
- **Écarté et signalé** : requêtes ni HTTP ni GraphQL, méthodes inconnues d'OpenAPI, doublon méthode + chemin (la première requête gagne), types d'authentification sans équivalent (NTLM, OAuth 1…).

Aucune valeur d'authentification n'est écrite : ni jeton, ni mot de passe, ni secret client. Les scripts et les tests ne sont pas décrits (OpenAPI n'en a pas).

Les identifiants des tests sont `ef_imp_03_*` : `crates/sync/tests/export_{postman,openapi}.rs`, `crates/cli/tests/export.rs`, `app/src-tauri/src/lib.rs` (commande `export_collection`) et `app/src/app/core/export.spec.ts`.
