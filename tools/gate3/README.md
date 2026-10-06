# Gate 3 : `xc run` contre `bru run`

ENF-COMP-03 : les scripts et tests du corpus donnent les mêmes résultats que `bru run`. Ce dossier compare les deux outils sur les 27 collections du corpus Gate 1 (`crates/core/tests/corpus/manifest.json`).

```bash
tools/gate3/setup.sh                    # installe @usebruno/cli 4.2.1 (Node.js 22)
cargo test -p xc-core --test corpus     # récupère le corpus dans target/corpus (git)
cargo build -p xc-cli                   # target/debug/xc
python3 tools/gate3/diff.py             # tout le corpus ; ajouter des identifiants pour en choisir
SHOW=10 python3 tools/gate3/diff.py corrugation missio
```

Chaque collection est copiée dans un dossier temporaire où toute adresse `http(s)://hôte[:port]` des fichiers `.yml`, `.yaml` et `.js` devient `http://127.0.0.1:4599` ; `echo-server.js` répond de façon déterministe (méthode, chemin, requête et corps renvoyés en JSON). Les deux outils tournent avec le même environnement (celui de `defaultEnvironment`, sinon le premier par ordre alphabétique) et leurs rapports JSON sont comparés requête par requête : statut, code HTTP, assertions, tests (dont ceux des scripts pré-requête et post-réponse) et message d'erreur. Un `bru run` qui ne répond pas en 150 s est abandonné.

## État (6 octobre 2026)

**Les 27 collections sont à zéro écart** (statut, code HTTP, assertions, tests avant et après la réponse, présence d'une erreur). Le texte des erreurs (`messages=` dans la sortie) est compté à part : QuickJS et Node, ou le moteur HTTP de chaque outil, ne l'écrivent pas pareil.

Ce que la comparaison a fait corriger : les requêtes qui contiennent `{{?nom}}` sont ignorées (`Skip::Prompts`) ; les requêtes WebSocket et gRPC sont rapportées en erreur, comme chez Bruno, au lieu d'être omises ; un corps multipart n'est plus recopié dans le script post-réponse (40 Mo dépassaient le plafond mémoire de QuickJS).

Ce que le harnais règle pour comparer des choses comparables : `--noproxy` des deux côtés (`crpt-openapi` déclare un proxy injoignable et les deux outils l'attendaient 150 s), un corps écho plafonné à 64 Kio, un jeton OAuth dans chaque réponse d'écho.

Écarts voulus (`crates/runner/tests/fixtures/gate3/divergences.json`, et les requêtes OAuth 2 interactives, comptées sous `interactif=`) : `xc run` refuse d'envoyer une requête dont le jeton OAuth manque ; `bru run` l'envoie sans. Voir `docs/docs/runner.md` § 8.

## Figer le résultat

```bash
FREEZE=1 python3 tools/gate3/diff.py    # réécrit crates/runner/tests/fixtures/gate3/<id>.json d'après bru run
cargo test -p xc-runner --test gate3    # rejoue le runner contre ces rapports (Node n'est jamais lancé)
```

Les rapports figés ont la forme normalisée de `diff.py` (`path`, `status`, `http`, `assertions`, `tests`, `pre`, `post`, `error`). `@usebruno/cli` 4.2.1 (npm) écrit une liste d'itérations `[{ iterationIndex, results, summary }]`, le commit épinglé de `tools/oracle` écrit `{ summary, results }` (ce que fait `xc`) ; `diff.py` accepte les deux.
