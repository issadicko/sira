# [NOM] — client API desktop offline-first

Client API compatible avec les collections Bruno (OpenCollection YAML) : un dossier, un fichier par requête, versionné avec Git, sans compte ni cloud. Moteur en Rust, fenêtre Tauri 2, interface Angular.

État : **MVP-α** (boucle requête testable). Voir `docs/docs/roadmap.md`.

## Structure

| Chemin | Rôle |
| --- | --- |
| `crates/engine` | Envoi HTTP/1.1 et HTTPS (hyper + rustls), timings DNS / TCP / TLS / TTFB / téléchargement, annulation |
| `crates/core` | Lecture et écriture OpenCollection YAML à l'identique de Bruno, résolution des variables, héritage auth et en-têtes, assertions |
| `crates/cli` | Binaire `xc` : `run` et `check` |
| `app/` | Interface Angular 21 zoneless (signals) ; `app/src-tauri` : commandes Tauri |
| `examples/demo` | Collection de démonstration (httpbin.org) |
| `docs/` | Cahier des charges, roadmap, maquettes |
| `DESIGN.md` | Système visuel de la maquette v2 |

## Prérequis

- Rust : la version est fixée par `rust-toolchain.toml` (rustup l'installe au premier `cargo`).
- Node.js 22 et npm.
- Linux uniquement : `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`.

## Lancer

```bash
cd app && npm install
```

```bash
cd app && npm run desktop
```

L'application s'ouvre ; « Ouvrir un dossier » puis choisir `examples/demo` (ou n'importe quelle collection Bruno au format YAML).

Aperçu de l'interface dans un navigateur, avec des données de démonstration en mémoire :

```bash
cd app && npm start
```

## CLI

```bash
cargo run -p xc-cli -- run examples/demo
```

```bash
cargo run -p xc-cli -- run examples/demo httpbin --env public --env-var client=Issa --bail
```

```bash
cargo run -p xc-cli -- check chemin/vers/une-collection-bruno
```

`run` sort avec le code 1 si une assertion ou un envoi échoue, 2 si la collection est illisible. `check` relit et réécrit chaque fichier en mémoire et signale ceux qui ne reviendraient pas à l'identique (porte d'entrée de la Gate 1).

## Tests

```bash
cargo test --workspace
```

Chaque test porte l'identifiant de l'exigence qu'il couvre (`ef_req_01_…`, `enf_comp_02_…`). Les fixtures de `crates/core/tests/fixtures` ont été produites par le sérialiseur de Bruno lui-même (`yaml` 2.3.4, mêmes options), ce qui garantit un aller-retour octet pour octet.

La logique pure de l'interface (recherche floue, registre de commandes, collage cURL, chemins de fichiers, corps de formulaire) a ses tests, même convention (`ef_ux_01_…`, `ef_imp_01_…`, `ef_req_02_…`), lancés par Node.js 22.18 ou plus sans dépendance, et par la CI :

```bash
cd app && npm test
```

C'est un raccourci pour `node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON --test "src/**/*.spec.ts"`.

## Importer

- **Coller un cURL** dans la barre d'URL : une commande qui commence par `curl` n'est pas collée telle quelle, elle est analysée comme Bruno le fait puis appliquée à la requête ouverte (URL, méthode, puis en-têtes, corps et authentification s'ils sont présents). L'onglet passe à « non enregistré ».
- **Nouvelle requête depuis cURL…** (palette ⌘⇧P, ou écran « Aucune requête ouverte ») : crée un fichier de requête dans le dossier choisi de la collection.
- **Importer une spec OpenAPI…** (palette ⌘⇧P, écran d'accueil ou « Aucune requête ouverte ») : OpenAPI 3.0 / 3.1 ou Swagger 2.0, fichier `.yaml`, `.yml`, `.json` ou URL ; aperçu, regroupement par tags ou par chemins, puis nouvelle collection dans le dossier parent choisi. La spec d'origine et un instantané sont gardés dans `.oc-sync/`.

Dans le navigateur (`npm start`), ces actions sont limitées à un aperçu : l'analyse et l'écriture se font dans l'application desktop.

## Raccourcis

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| Palette : requêtes, commandes, environnements | ⌘K | Ctrl+K |
| Palette : toutes les commandes | ⌘⇧P | Ctrl+Maj+P |
| Envoyer | ⌘↵ | Ctrl+↵ |
| Annuler l'envoi | Échap | Échap |
| Enregistrer | ⌘S | Ctrl+S |
| Filtrer les requêtes de l'arbre | ⌘⇧F | Ctrl+Maj+F |
| Ouvrir une collection | ⌘O | Ctrl+O |
| Fermer l'onglet | ⌘W | Ctrl+W |
| Barre latérale | ⌘B | Ctrl+B |
| Empiler requête / réponse | ⌘\ | Ctrl+\ |

Dans la palette : ↑ et ↓ pour naviguer (en boucle), ↵ pour exécuter, Échap pour fermer ; le préfixe `>` ne garde que les commandes. La recherche est floue (lettres dans l'ordre, sans casse ni accents) sur le nom, l'URL et le chemin du fichier des requêtes, dont l'URL vient de l'arbre de la collection sans relire les fichiers.
