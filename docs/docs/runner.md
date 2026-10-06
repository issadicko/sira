# Runner, CLI et rapports (V1, EF-RUN-01, EF-RUN-02, EF-CLI-02)

Le crate `xc-runner` exécute une requête de bout en bout (voir `scripts.md`), puis une collection entière : c'est lui qui sert `xc run` et le runner de l'application. Les rapports reprennent ceux de `bru run`, relevés dans le code de Bruno (`bruno-cli/src/commands/run.js`, `reporters/`, `bruno-common/src/runner`, commit épinglé).

Les jetons OAuth 2 obtenus pendant un run restent dans la `Session` et servent aux requêtes suivantes ; voir `auth.md`.

## 1. Ce que fait un run

`run_collection(Job, &mut Session, rappel) -> RunReport` parcourt les requêtes HTTP choisies dans l'ordre de l'arbre (`seq`, puis nom), chacune avec le pipeline complet, et garde dans la `Session` les variables que les scripts écrivent, d'une requête et d'une itération à l'autre.

- **Sélection** : `select(arbre, chemins)`. Sans chemin, toute la collection ; sinon chaque requête ou dossier (avec ses sous-dossiers), nommé par son chemin relatif (`users/show.yml`, l'extension est facultative), l'un après l'autre. Un chemin inconnu ou un dossier sans requête HTTP est une erreur. Un fichier illisible n'arrête pas le run : il est signalé et compté comme ignoré, comme chez Bruno.
- **Délai** (`--delay ms`) : attente entre deux requêtes, jamais après la dernière. Elle réagit à l'annulation.
- **Sélection** : `--tests-only` ne garde que les requêtes qui ont une assertion active ou un `test(` exécutable dans leur script pré-requête, post-réponse ou de tests (un `test(` en commentaire, dans un texte ou appelé comme méthode, `obj.test(`, ne compte pas ; comme Bruno, les commentaires de ligne partent avant les textes, donc un `//` dans une adresse coupe la ligne). `--tags a,b` ne garde que les requêtes qui portent l'un de ces tags (`info.tags`), `--exclude-tags` écarte celles qui en portent un, l'exclusion l'emporte ; les noms sont séparés par des virgules, sans autre nettoyage. Un fichier illisible n'est jamais écarté en silence : il reste, et son erreur figure au rapport. Aucune requête ne correspond : un avertissement, un rapport vide, code 0. Même sélection que `bru run` sur le corpus (vérifié sur `--tests-only` et `--tags`).
- **Environnement d'un fichier** : `--env-file` lit un environnement hors de `environments/`, au format JSON de Bruno (`{ name?, variables: [{ name, value, enabled?, secret? }] }`) ou en YAML OpenCollection (`extends` laissé de côté, les secrets du trousseau n'y sont pas lus). Le chemin est absolu ou relatif à la collection ; `--env` et `--env-file` ensemble sont refusés (Bruno les fusionne). Le nom de l'environnement (`bru.getEnvName()`) est celui du JSON, sinon celui du fichier. Un fichier `.bru` est refusé avec un message.
- **Délai et redirections par script** : `req.setTimeout(ms)` et `req.setMaxRedirects(n)` d'un script pré-requête valent pour l'envoi de la requête (0 : pas de délai ; 0 saut : la redirection n'est pas suivie).
- **Arrêt au premier échec** (`--bail`) : une requête qui échoue (erreur d'envoi ou de script pré-requête, assertion, test de n'importe quelle phase, erreur de script) arrête le run ; ce qui restait à exécuter est listé comme ignoré (`skipReason: bail`) et le motif suit l'ordre de Bruno : `request failure`, `assertion failure`, `pre-request test failure`, `post-response test failure`, `test failure`.
- **Sauts et arrêts** : `bru.setNextRequest(nom)` saute à la première requête de ce nom (avant ou après), `null` termine le run, `bru.runner.skipRequest()` ignore la requête en cours, `bru.runner.stopExecution()` arrête le run et marque le reste ignoré (`skipReason: stopExecution`). Au-delà de 10 000 sauts le run s'arrête comme une boucle sans fin (échec).
- **Variables à saisir** : une requête dont l'adresse, les en-têtes, le corps, l'authentification, les scripts ou une variable contiennent `{{?nom}}` est ignorée (statut `skipped`, `response.statusText` : `Prompt variables detected in request. CLI execution is not supported for requests with prompt variables.` puis `Prompts: nom, …`), comme `bru run` : un run ne peut pas demander une valeur. Le reste du run continue ; l'envoi seul depuis l'application laisse la variable non résolue.
- **Annulation** : un drapeau levé d'un autre fil arrête le run avant la requête suivante et interrompt le script ou l'envoi en cours.

## 2. Itérations pilotées par des données (EF-RUN-02)

`--data fichier` rejoue toute la sélection une fois par ligne du fichier :

- **CSV** (RFC 4180) : en-têtes en première ligne, champs entre guillemets, `""` pour un guillemet, retours à la ligne permis dans un champ, BOM ignoré, lignes vides ignorées. Les valeurs sont du texte. Une ligne plus longue que l'en-tête est refusée.
- **JSON** : un tableau d'objets (un objet seul vaut une itération). Les valeurs gardent leur type : `{{id}}` s'interpole, `bru.getVar('id')` rend un nombre.

Les champs d'une ligne deviennent des **variables runtime** au début de l'itération ; les autres variables runtime survivent d'une itération à l'autre, comme dans Postman. Le délai s'applique aussi entre deux itérations. Dans les rapports, chaque résultat porte son `iterationIndex`, et le JSON ajoute `iterations` (données et résumé de chacune) dès qu'il y a des données.

## 3. `xc run`

```bash
xc run <collection> [chemins...] [--env nom | --env-file fichier.json|yml] [--env-var nom=valeur] [--bail] [--delay ms]
       [--data fichier.csv|json] [--tests-only] [--tags a,b] [--exclude-tags c]
       [-o fichier -f json|junit|html] [--reporter-json f] [--reporter-junit f] [--reporter-html f]
       [--reporter-skip-all-headers] [--reporter-skip-headers nom...] [--reporter-skip-request-body]
       [--reporter-skip-response-body] [--reporter-skip-body]
```

Sorties : le détail de chaque requête au fil de l'eau (statut, durée, console, tests, assertions, erreurs), puis un résumé (réussies, en échec, ignorées, tests, assertions, durée) et le motif d'un éventuel arrêt.

**Code de sortie** : 0 si tout passe ; 1 si une requête, un test, une assertion ou un script échoue, ou si le run s'emballe ; 2 si la ligne de commande est invalide (environnement introuvable, chemin inconnu, fichier de données illisible ou vide, dossier du rapport inexistant) ou la collection illisible. Bruno distingue davantage de codes (3 à 13) ; ici 2 couvre toute entrée invalide.

## 4. Rapports (EF-CLI-02)

Chaque format est écrit **après** le run, y compris quand il a échoué, et plusieurs peuvent l'être à la fois. `-o` et `-f` en choisissent un ; `--reporter-*` en ajoute et remplace `-o` pour son format.

- **JSON** : `{ summary, results }` comme `bru run --format json`. Chaque résultat porte `test.filename`, `request` (méthode, URL, en-têtes, corps), `response` (statut, `statusText`, en-têtes, corps, `responseTime` en ms, `size`), `status` (`pass`, `error` ou `skipped` : un test en échec ne change pas ce statut), `error`, `assertionResults`, `testResults`, `preRequestTestResults`, `postResponseTestResults`, `runDuration` en secondes. Une erreur de script post-réponse ou de test devient un résultat en échec marqué `isScriptError`, que le résumé compte comme échec sans l'ajouter aux totaux. Le corps JSON est rendu en JSON, un corps non UTF-8 est remplacé par `<N octets binaires>`.
- **JUnit** : un `<testsuite>` par requête, avec un `<testcase>` par assertion puis par test (pré-requête, requête, post-réponse), `<failure>` sur un échec ; une requête en erreur n'a qu'un cas « Test suite has no errors » avec `<error>` ; une requête ignorée porte `<skipped>`. Avec des itérations, le nom de la suite est suffixé de ` [itération N]`.
- **HTML** : une page autonome (ni script ni ressource externe, thème clair ou sombre selon le système) : verdict, compteurs, puis chaque requête dépliable (ouverte si elle a échoué) avec ses assertions, tests, en-têtes et corps (tronqués à 100 000 caractères). Tout le texte est échappé. Bruno embarque son rapport dans un modèle JavaScript ; celui-ci est de la structure statique, lisible partout (pièce jointe de CI, courriel).
- **Retraits** : `--reporter-skip-all-headers`, `--reporter-skip-headers`, `--reporter-skip-request-body`, `--reporter-skip-response-body` et `--reporter-skip-body` ôtent en-têtes et corps de tous les formats, pour ne pas écrire de secrets dans des artefacts de CI.

Le résumé reprend `getRunnerSummary` de Bruno : une requête est « en échec » si un test, une assertion ou un script a échoué, « en erreur » si elle n'a pas abouti sans autre échec, « ignorée » si un script ou l'arrêt l'a écartée (`skippedByBail` compte celles de `--bail`).

## 5. Image Docker

Le CLI est un binaire unique, sans Node ni Chromium. Le `Dockerfile` le compile (`rust:1.94`) et ne garde que lui sur une image `distroless/cc` exécutée sans privilèges :

```bash
docker build -t sira .
docker run --rm -v "$PWD:/collection" sira run /collection --env prod --reporter-junit /collection/junit.xml
```

Le dossier monté doit être inscriptible pour recevoir les rapports. La CI construit l'image, vérifie que le binaire démarre et qu'il lit la collection de démonstration montée (`xc check`).

## 6. Dans l'application

La vue **Runner** (icône ▶ de la barre d'activité, commande « Ouvrir le runner », ou « Exécuter le dossier… » dans le menu d'un dossier de l'arbre) pilote le même `run_collection`, avec `execution_mode: runner` :

- **Barre latérale** : ce qui s'exécute (la collection ou un dossier, avec le nombre de requêtes), l'attente entre deux requêtes, l'arrêt au premier échec, le fichier de données (le nombre d'itérations et les colonnes sont annoncés dès le choix), et les retraits du rapport exporté (sans en-têtes, sans corps).
- **Run en direct** : chaque requête apparaît à mesure qu'elle se termine, la requête en cours et l'attente entre deux requêtes aussi ; une ligne de résumé (réussies, en échec, ignorées, vérifications, durée) et un filtre « Échecs ». La raison d'un échec est lisible sans déplier la ligne ; déplier montre l'URL, l'erreur, les assertions, les tests par phase et la console, comme l'onglet Tests d'une réponse. Avec des données, les résultats sont groupés par itération.
- **Annuler** (Échap) : le drapeau d'annulation interrompt les scripts et abandonne l'envoi en cours ; ce qui avait fini reste affiché, la requête abandonnée n'est pas comptée.
- **Exporter** : le rapport du dernier run en HTML, JUnit ou JSON, dans un fichier choisi.
- **Variables** : celles que les scripts posent rejoignent celles de la collection, comme après un envoi isolé (et inversement : le run part des variables courantes).

Côté Tauri, `start_run` annonce l'avancement par l'événement `run-event` (`begin`, `iteration`, `started`, `finished`, `waiting`, `warning`) et rend le résumé à la fin ; `cancel_run`, `inspect_run_data` et `export_run` complètent. Seul le dernier rapport est gardé en mémoire pour l'export. Le mode démo simule un run (une requête `DELETE` y échoue, pour montrer un échec).

## 7. Ce qui reste

- `--global-env`, `--workspace-path`, `--sandbox`, `--secrets-env-file`, `--parallel`, `--iteration-count` : options de `bru run` non reprises (pas d'espace de travail, un seul bac à sable, itérations en série d'après le fichier de données).
- Les requêtes gRPC et WebSocket ne sont pas exécutées par le runner : elles figurent au rapport en erreur (« protocole non pris en charge par le runner : websocket »), comme `bru run` les compte en erreur, au lieu d'être omises sans rien dire. Les requêtes GraphQL s'exécutent avec les requêtes HTTP (voir `graphql.md`).
- Bruno ne lance un dossier que sans ses sous-dossiers, sauf `-r` ; ici un dossier est toujours parcouru récursivement.
- Le nom d'hôte des suites JUnit vient de `HOSTNAME`, `COMPUTERNAME` ou `/etc/hostname` ; à défaut, `localhost`.

## 8. Gate 3 : mêmes résultats que `bru run`

ENF-COMP-03 est tenue sur les 27 collections du corpus (1 578 fichiers de Gate 1, plus de 1 300 requêtes) : pour chacune, le statut, le code HTTP, les assertions, les tests (dont ceux des scripts pré-requête et post-réponse) et la présence d'une erreur de chaque requête sont ceux de `@usebruno/cli` 4.2.1.

- **Mesure** : `tools/gate3/diff.py` lance `bru run` et `xc run` sur une copie de chaque collection dont les adresses pointent un serveur d'écho local, et compare requête par requête (`tools/gate3/README.md`). Le texte des erreurs est compté à part : QuickJS et Node n'écrivent pas les erreurs JavaScript de la même façon, seul le statut compte.
- **Test** : `crates/runner/tests/gate3.rs` rejoue le même run avec le runner et un serveur d'écho écrit en Rust, et compare au rapport de Bruno figé dans `crates/runner/tests/fixtures/gate3/<id>.json`. Il ne lance jamais Node ; `FREEZE=1 python3 tools/gate3/diff.py` régénère les fixtures.
- **Écarts corrigés** : les requêtes WebSocket et gRPC sont rapportées en erreur au lieu d'être omises ; un corps multipart (jusqu'à plusieurs dizaines de Mo) n'est plus recopié dans le script post-réponse, où `req.getBody()` garde la valeur d'avant l'envoi comme chez Bruno ; une requête qui demande des variables à saisir est ignorée (voir plus haut).
- **Écarts voulus**, listés dans `fixtures/gate3/divergences.json` ou propres au flux : un flux OAuth 2 interactif (`authorization_code`, implicite) ne peut pas s'achever en ligne de commande, `xc run` le dit en erreur et n'envoie pas la requête, alors que `bru run` l'envoie sans jeton ; de même une requête de type `websocket` dont l'adresse est `http://` part en HTTP chez Bruno et est refusée ici. Envoyer une requête sans le jeton qu'elle annonce serait une défaillance silencieuse.
