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
| `bru.runRequest(chemin)` | exécute une autre requête de la collection (chemin relatif à la racine, `.yml` ajouté) avec les variables du script appelant et lui rend la réponse `{ status, statusText, headers, data, url, responseTime, duration, size }` ; ne rejette jamais (`{ message }` en cas d'échec, `{}` si le fichier n'existe pas), 8 niveaux d'imbrication au plus |
| `axios`, `bru.sendRequest` | `axios(config)`, `.get/.delete/.post/.put/.patch`, `bru.sendRequest(config ou url, rappel)` : envoi par le moteur HTTP, erreurs à la façon d'axios (`isAxiosError`, `code`, `response`, rejet au-delà de 2xx) |
| `bru.cookies` | le pot de cookies, vu à travers l'adresse de la requête (variables résolues) : `get(nom)`, `one`, `all`, `idx`, `count`, `has(nom[, valeur])`, `indexOf`, `find`, `filter`, `each`, `map`, `reduce`, `toObject`, `toString`, `toJSON` (synchrones), `add`/`upsert(cookie)`, `remove`/`delete(nom)`, `clear()` (promesse, ou rappel `(err)`) ; `jar()` donne `getCookie`, `getCookies`, `hasCookie`, `setCookie(url, nom, valeur)` ou `setCookie(url, objet)`, `setCookies(url, liste)`, `deleteCookie`, `deleteCookies`, `clear` pour n'importe quelle adresse (variables résolues). Un cookie est `{ key, value, domain, path, secure, httpOnly, expires }`, `expires` valant `"Infinity"` pour un cookie de session. Sans domaine, un cookie posé prend celui de l'adresse, sans chemin celui que l'adresse donne ; un domaine étranger ou un suffixe public est ignoré. Voir [Cookies](reseau.md#cookies) |
| Modules locaux | `require('./lib/x')` relatif à la racine de la collection, `.js` ajouté, refusé hors de la collection (liens suivis) |

## 3. Dans l'application et en CLI

- **Assertions déclaratives** : évaluées comme chez Bruno, avec chai (voir plus bas).

- **Onglet Scripts de la requête** : trois éditeurs (avant la requête, après la réponse, tests) écrits dans `runtime.scripts` du fichier, sans toucher aux autres clés. Un code vide retire le script.
- **Envoi** : `xc-runner` enchaîne script pré-requête (collection, dossiers, requête), envoi, script post-réponse, assertions et tests, dans l'ordre de Bruno (`sandwich` par défaut, `sequential` selon `extensions.bruno.scripts.flow`). Une erreur avant l'envoi l'annule ; après, assertions et tests s'exécutent quand même.
- **Réponse, onglet Tests** : résumé (assertions et tests de script comptés ensemble), résultats par phase avec « attendu · reçu », erreurs de script, console. Un script qui ignore la requête (`bru.runner.skipRequest()`) ou qui échoue avant l'envoi affiche tout de même sa console et ses tests.
- **Variables** : ce que les scripts écrivent (`bru.setVar`, `setEnvVar`, `setGlobalEnvVar`, variables de collection) reste en mémoire pour les requêtes suivantes de la collection, et apparaît dans la résolution des variables. Rien n'est écrit dans les fichiers : Bruno CLI réécrit l'environnement et la collection, ce n'est pas repris ici.
- **`xc run`** affiche la console, les tests et les erreurs de script de chaque requête, suit `bru.setNextRequest(nom)`, `bru.runner.skipRequest()` et `bru.runner.stopExecution()` ; le code de sortie est 1 si une requête, un test ou une assertion échoue. Détail du runner et des rapports : `runner.md`.

## 4. Ce qui reste

Pas encore : `jwt`, `headerList` ; `req.onFail` est absent aussi du sandbox de Bruno.

## 5. Sandbox (ENF-SEC-02)

Aucun accès au disque, au réseau ni aux processus hors des fonctions que l'hôte pose (variables, journal, `require` local borné à la collection). Contrairement à Bruno, qui n'a aucune limite en production, le sandbox plafonne : 256 Mio de mémoire, 8 Mio de pile (Ajv en demande plus d'1) et 30 s de **calcul** (l'attente d'un `sleep` ou d'une requête ne compte pas). Une boucle infinie est coupée au lieu de figer l'application ; l'annulation de la requête interrompt aussi le script.

## 6. Bibliothèques embarquées

Les bibliothèques de `crates/script/js/libs/` sont construites par `tools/scripts-bundle` (`npm install && npm run build`, esbuild) et **commitées** : ni Node ni réseau ne sont requis pour compiler Sira. Leur reconstruction est la seule raison de rouvrir ce dossier.

## 7. Assertions déclaratives (EF-TST-02)

Évaluées par QuickJS et chai, comme `assert-runtime.js` de Bruno : l'expression gauche est du JavaScript évalué sur `res`, `req`, `bru` et les variables (une erreur devient la valeur : `res.body.a.b` avec `a` absent n'est pas `undefined` mais une erreur) ; l'opérande droit est interpolé (`{{var}}`, `{{process.env.X}}`) puis évalué comme littéral : nombre, `true`, `false`, `null`, `undefined`, texte avec ou sans guillemets où `${…}` est évalué ; un tableau ou un objet JSON reste un texte. `eq` est strict (`"200"` n'est pas `200`), `isTruthy` et `isFalsy` sont les booléens `true` et `false`, les comparaisons exigent des nombres. Les 28 opérateurs sont pris en charge ; `startsWith` et `endsWith` fonctionnent, alors qu'ils échouent dans le sandbox de Bruno (chai-string n'y est pas chargé). Les messages d'échec sont ceux de chai.

Variables « après la réponse » (`runtime.actions`, `set-variable`) : leur expression est évaluée comme celle d'une assertion avant le script post-réponse ; une expression qui échoue donne sa valeur d'erreur à la variable, et seuls les noms invalides sont rapportés dans la console.
