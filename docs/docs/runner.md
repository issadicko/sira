# Runner, CLI et rapports (V1, EF-RUN-01, EF-RUN-02, EF-CLI-02)

Le crate `xc-runner` exécute une requête de bout en bout (voir `scripts.md`), puis une collection entière : c'est lui qui sert `xc run` et, bientôt, le runner de l'application. Les rapports reprennent ceux de `bru run`, relevés dans le code de Bruno (`bruno-cli/src/commands/run.js`, `reporters/`, `bruno-common/src/runner`, commit épinglé).

## 1. Ce que fait un run

`run_collection(Job, &mut Session, rappel) -> RunReport` parcourt les requêtes HTTP choisies dans l'ordre de l'arbre (`seq`, puis nom), chacune avec le pipeline complet, et garde dans la `Session` les variables que les scripts écrivent, d'une requête et d'une itération à l'autre.

- **Sélection** : `select(arbre, chemins)`. Sans chemin, toute la collection ; sinon chaque requête ou dossier (avec ses sous-dossiers), nommé par son chemin relatif (`users/show.yml`, l'extension est facultative), l'un après l'autre. Un chemin inconnu ou un dossier sans requête HTTP est une erreur. Un fichier illisible n'arrête pas le run : il est signalé et compté comme ignoré, comme chez Bruno.
- **Délai** (`--delay ms`) : attente entre deux requêtes, jamais après la dernière. Elle réagit à l'annulation.
- **Arrêt au premier échec** (`--bail`) : une requête qui échoue (erreur d'envoi ou de script pré-requête, assertion, test de n'importe quelle phase, erreur de script) arrête le run ; ce qui restait à exécuter est listé comme ignoré (`skipReason: bail`) et le motif suit l'ordre de Bruno : `request failure`, `assertion failure`, `pre-request test failure`, `post-response test failure`, `test failure`.
- **Sauts et arrêts** : `bru.setNextRequest(nom)` saute à la première requête de ce nom (avant ou après), `null` termine le run, `bru.runner.skipRequest()` ignore la requête en cours, `bru.runner.stopExecution()` arrête le run et marque le reste ignoré (`skipReason: stopExecution`). Au-delà de 10 000 sauts le run s'arrête comme une boucle sans fin (échec).
- **Annulation** : un drapeau levé d'un autre fil arrête le run avant la requête suivante et interrompt le script ou l'envoi en cours.

## 2. Itérations pilotées par des données (EF-RUN-02)

`--data fichier` rejoue toute la sélection une fois par ligne du fichier :

- **CSV** (RFC 4180) : en-têtes en première ligne, champs entre guillemets, `""` pour un guillemet, retours à la ligne permis dans un champ, BOM ignoré, lignes vides ignorées. Les valeurs sont du texte. Une ligne plus longue que l'en-tête est refusée.
- **JSON** : un tableau d'objets (un objet seul vaut une itération). Les valeurs gardent leur type : `{{id}}` s'interpole, `bru.getVar('id')` rend un nombre.

Les champs d'une ligne deviennent des **variables runtime** au début de l'itération ; les autres variables runtime survivent d'une itération à l'autre, comme dans Postman. Le délai s'applique aussi entre deux itérations. Dans les rapports, chaque résultat porte son `iterationIndex`, et le JSON ajoute `iterations` (données et résumé de chacune) dès qu'il y a des données.

## 3. `xc run`

```bash
xc run <collection> [chemins...] [--env nom] [--env-var nom=valeur] [--bail] [--delay ms] [--data fichier.csv|json]
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

## 6. Ce qui reste

- L'interface du runner dans l'application (EF-RUN-01 côté fenêtre : choisir un dossier, suivre le run en direct, annuler).
- `--tests-only`, `--tags` / `--exclude-tags`, `--env-file`, `--global-env`, `--sandbox`, `--insecure`, `--cacert` : options de `bru run` non reprises (les réglages réseau viennent avec le lot « réglages réseau »).
- Les requêtes GraphQL, gRPC et WebSocket ne sont pas exécutées par le runner (GraphQL avec le lot EF-GQL-01).
- Bruno ne lance un dossier que sans ses sous-dossiers, sauf `-r` ; ici un dossier est toujours parcouru récursivement.
