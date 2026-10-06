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

Première passe sur les collections qui ont fini :

| Collection | Requêtes | Écarts |
| --- | --- | --- |
| `jsonplaceholder`, `nanomon`, `mockdata.dev` | 15, 10, 94 | aucun |
| `dz4j` | 110 | `oauth/get-access-token` : Bruno ignore une requête qui contient `{{?Code}}` (variable à saisir) ; **corrigé** (`Skip::Prompts`, `ef_run_01_a_request_with_prompt_variables_is_skipped_like_bru_run`). `playlist/upload-cover` : même échec (fichier introuvable, chemin `..\assets\…` de Windows), seul le texte du message diffère |
| `ba-docs` | 102 | `Mixer/meta/mixer.set_volume.yml` : même échec (pas d'`info.type`), seul le texte du message diffère |

À faire pour fermer la Gate :

1. **Terminer la passe** sur les 22 autres collections. `crpt-openapi` fait attendre `bru run` jusqu'au délai de 150 s : trouver pourquoi (une requête que le serveur local ne termine pas ? une adresse qui n'est pas réécrite ?) et vérifier que `xc run` y répond comme Bruno.
2. **Les scripts** : les collections qui en ont vraiment sont `corrugation` (221 tests), `opencollection` (41), `missio` (scripts, tests et 19 assertions), `hyxi-cloud-api`, `senior-accounting-officer-stubs`, `cubby`, `oauth`, `latech`, `unlockit`, `syfomotebehov`. Chaque écart restant est un écart d'exécution de `xc-script` ou du runner à corriger, ou un écart de message à normaliser (QuickJS et Node n'écrivent pas les erreurs JavaScript pareil : comparer le statut avant le texte).
3. **Figer le résultat** : écrire pour chaque collection le rapport normalisé de Bruno dans `crates/runner/tests/fixtures/gate3/<id>.json` (les tests ne lancent jamais Node, comme pour l'oracle), puis un test Rust `crates/runner/tests/gate3.rs` qui réécrit le corpus de la même façon, lance le runner contre un serveur local identique et compare au fixture. Le corpus se récupère déjà avec le code de `crates/core/tests/corpus.rs` (à partager par `#[path]`).
4. Forme du rapport JSON : `@usebruno/cli` 4.2.1 (npm) écrit une liste d'itérations `[{ iterationIndex, results, summary }]`, le commit épinglé de `tools/oracle` écrit `{ summary, results }` (ce que fait `xc`). `diff.py` accepte les deux.
5. Documenter le résultat dans `docs/docs/runner.md` et la feuille de route (ligne « Gate 3 »).
