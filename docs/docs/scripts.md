# Scripts et tests (V1, EF-SCR-01 à 03, EF-TST-01)

Le crate `xc-script` exécute les scripts d'une requête dans un sandbox QuickJS (`rquickjs`) avec l'API de Bruno (`bru`, `req`, `res`, `test`, `expect`, `assert`, `console`, `require`). Il suit le comportement du sandbox « safe » de Bruno, relevé dans son code (`tools/oracle/bruno/packages/bruno-js`, commit épinglé) : un script écrit pour Bruno tourne sans modification.

## 1. Ce que fait le crate

`xc_script::run(Input) -> Output` exécute **une phase** (`Pre`, `Post` ou `Tests`) dans un contexte neuf, comme Bruno : rien ne survit d'une phase à l'autre hors des variables. Le script fusionné de la phase est enveloppé dans la même fonction asynchrone que Bruno (`setTimeout` réduit à une promesse, premier tour de boucle cédé), évalué en mode non strict.

- **Entrée** : la requête (`ScriptRequest`, en-têtes dans l'ordre du fichier), la réponse (`ScriptResponse`, absente avant l'envoi : `res` est alors indéfini), les cartes de variables typées (`Vars` : global, collection, environnement, dossier, requête, OAuth 2, runtime, `process.env`).
- **Sortie** : la requête modifiée (URL, méthode, en-têtes, corps, délai, en-têtes à retirer), le corps posé par `res.setBody()`, les variables avec les cartes **modifiées** (`Dirty`), les résultats de `test()`, les lignes de `console`, la suite demandée au runner (`setNextRequest`, `skipRequest`, `stopExecution`) et l'erreur du script. Une erreur du script garde tout ce qui la précède : variables écrites, tests terminés.
- **Interpolation** : `{{nom}}` avec la précédence de Bruno (global < collection < environnement < dossier < requête < OAuth 2 < runtime), chemins `a.b[0]`, `{{process.env.X}}`, `{{$dynamique}}` fournis par l'hôte ; un nom introuvable reste tel quel, `{{ nom }}` avec espaces ne se résout pas, comme chez Bruno. Les getters (`bru.getVar`, `bru.getEnvVar`…) interpolent à chaque lecture.

## 2. API couverte

| Objet | Couvert |
| --- | --- |
| `bru` | variables (env, globales, collection, runtime ; lecture seule pour dossier, requête, `process.env`), `interpolate`, `setNextRequest`, `runner.skipRequest/stopExecution/setNextRequest`, `sleep`, `cwd`, `getCollectionName`, `isSafeMode`, `getTestResults`, messages d'erreur exacts pour les noms invalides |
| `req` | `getUrl/setUrl`, `getHost/getPath/getQueryString`, `getMethod/setMethod`, en-têtes (`getHeader` sensible à la casse), `getBody/setBody`, `getName`, `getTags`, `getPathParams`, `getAuthMode`, `getTimeout/setTimeout`, `setMaxRedirects`, `disableParsingResponseJson`, `getExecutionMode` ; les propriétés (`req.url`, `req.headers`…) sont des instantanés, comme dans le sandbox de Bruno |
| `res` | `status`, `statusText`, `headers` (minuscules), `body`, `responseTime`, `url` ; `getStatus`, `getHeader`, `getHeaders`, `getBody`, `getSize`, `setBody` ; `res('items[?].id', fn)` (le `get` de `@usebruno/query`, porté) |
| `test`, `expect`, `assert` | chai 4.5, plus `.json`, `.jsonSchema()` (Ajv, Draft-07) et `.jsonBody()` de Bruno ; l'ordre des résultats est celui du règlement des promesses, comme chez Bruno |
| Bibliothèques | `chai`, `moment`, `crypto-js`, `uuid`, `nanoid`, `tv4`, `ajv`, `ajv-formats`, `buffer` / `Buffer`, `btoa`, `atob`, `path.resolve`, `crypto.randomBytes / getRandomValues` ; chargées à la demande |
| Modules locaux | `require('./lib/x')` relatif à la racine de la collection, `.js` ajouté, refusé hors de la collection (liens suivis) |

Pas encore : `bru.runRequest`, `bru.sendRequest`, `axios`, `jwt`, `bru.cookies` (ils demandent le moteur HTTP et le runner), `headerList`, `req.onFail` (absent aussi du sandbox de Bruno).

## 3. Sandbox (ENF-SEC-02)

Aucun accès au disque, au réseau ni aux processus hors des fonctions que l'hôte pose (variables, journal, `require` local borné à la collection). Contrairement à Bruno, qui n'a aucune limite en production, le sandbox plafonne : 256 Mio de mémoire, 8 Mio de pile (Ajv en demande plus d'1) et 30 s de **calcul** (l'attente d'un `sleep` ou d'une requête ne compte pas). Une boucle infinie est coupée au lieu de figer l'application ; l'annulation de la requête interrompt aussi le script.

## 4. Bibliothèques embarquées

Les bibliothèques de `crates/script/js/libs/` sont construites par `tools/scripts-bundle` (`npm install && npm run build`, esbuild) et **commitées** : ni Node ni réseau ne sont requis pour compiler Sira. Leur reconstruction est la seule raison de rouvrir ce dossier.
