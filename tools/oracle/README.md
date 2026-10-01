# Oracle Bruno

Exécute le vrai code de Bruno (convertisseurs OpenAPI et cURL, sérialiseur OpenCollection YAML) pour produire les fixtures attendues des tests Rust. Les tests ne lancent jamais Node : les sorties de l'oracle sont committées dans `crates/*/tests/fixtures`.

Bruno est épinglé au commit `a2dbda9f` (30 sept. 2026). Prérequis : Node.js 22 et git.

```bash
tools/oracle/setup.sh
```

```bash
node tools/oracle/oracle.js openapi spec.yaml tags
```

Commandes : `openapi <spec> [tags|path]`, `import <spec> <dossier> [tags|path]`, `curl <fichier>`, `stringify-item <item.json>`, `stringify-folder <root.json>`, `stringify-collection <root.json> <brunoConfig.json>`, `stringify-environment <env.json>`.
