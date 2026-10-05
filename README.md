# Sira — client API desktop offline-first

Client API compatible avec les collections Bruno (OpenCollection YAML) : un dossier, un fichier par requête, versionné avec Git, sans compte ni cloud. Moteur en Rust, fenêtre Tauri 2, interface Angular.

État : **MVP complet** (α boucle requête et rechargement à chaud, β imports cURL et OpenAPI, palette, CodeMirror, γ synchro OpenAPI à 3 voies, δ gestion de collection, gestion des environnements). Voir `docs/docs/roadmap.md`.

## Structure

| Chemin | Rôle |
| --- | --- |
| `crates/engine` | Envoi HTTP/1.1 et HTTPS (hyper + rustls), timings DNS / TCP / TLS / TTFB / téléchargement, annulation |
| `crates/core` | Lecture et écriture OpenCollection YAML à l'identique de Bruno, résolution des variables, héritage auth et en-têtes, assertions |
| `crates/sync` | Imports à l'identique de Bruno (cURL, OpenAPI 3.0 / 3.1 / Swagger 2.0, sérialiseur OpenCollection YAML) et synchro OpenAPI à 3 voies non destructive (`.oc-sync/`) |
| `crates/watch` | Surveillance du dossier de la collection (`notify`) : changements du disque regroupés en lots de chemins relatifs |
| `crates/cli` | Binaire `xc` : `run`, `check`, `import` et `sync` |
| `app/` | Interface Angular 21 zoneless (signals) ; `app/src-tauri` : commandes Tauri |
| `examples/demo` | Collection de démonstration (httpbin.org) |
| `docs/` | Cahier des charges, roadmap, études, maquettes |
| `tools/oracle` | Exécute le vrai code de Bruno pour produire les fixtures attendues |
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

```bash
cargo run -p xc-cli -- import https://petstore3.swagger.io/api/v3/openapi.json ~/collections --group-by tags
```

`run` sort avec le code 1 si une assertion ou un envoi échoue, 2 si la collection est illisible. `check` relit et réécrit chaque fichier en mémoire et signale ceux qui ne reviendraient pas à l'identique (`--diff` montre la première ligne qui change). `import` crée une collection à partir d'une spec OpenAPI (fichier ou URL) et affiche son chemin.

```bash
cargo run -p xc-cli -- sync chemin/vers/la-collection --check
```

```bash
cargo run -p xc-cli -- sync chemin/vers/la-collection --apply --keep-team
```

`sync --check` sort avec le code 1 si la spec a divergé de la collection (à brancher en CI). `sync --apply` fusionne la nouvelle spec à 3 voies sans écraser ce que l'équipe a saisi ; s'il reste des conflits, il refuse sauf avec `--keep-team` ou `--take-spec`. Règles : `docs/docs/synchro-openapi.md`.

## Tests

```bash
cargo test --workspace
```

Chaque test porte l'identifiant de l'exigence qu'il couvre (`ef_req_01_…`, `enf_comp_02_…`). Les fixtures de `crates/core/tests/fixtures` ont été produites par le sérialiseur de Bruno lui-même (`yaml` 2.3.4, mêmes options), ce qui garantit un aller-retour octet pour octet. Celles de `crates/sync/tests/fixtures` (cURL, OpenAPI, sérialiseur, import) viennent du vrai code de Bruno via `tools/oracle` : chaque import doit produire les mêmes octets que Bruno.

La logique pure de l'interface (recherche floue, registre de commandes, collage cURL, chemins de fichiers, corps de formulaire, réglages) a ses tests, même convention (`ef_ux_01_…`, `ef_imp_01_…`, `ef_req_02_…`, `ef_ux_03_…`), lancés par Node.js 22.18 ou plus sans dépendance, et par la CI :

```bash
cd app && npm test
```

C'est un raccourci pour `node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON --test "src/**/*.spec.ts"`.

## Créer et organiser une collection

- **Ouvrir un dossier** (⌘O) vide ou non vide propose « Créer une collection ici » : seuls `opencollection.yml` et `.gitignore` sont écrits, identiques à ceux de Bruno. **Nouvelle collection…** (accueil, palette) crée le dossier sous un parent choisi.
- **Arbre** : bouton `+` (à la racine), menu contextuel (clic droit, touche Menu, Maj+F10), renommage et création sur place, dupliquer, supprimer **vers la corbeille du système** après confirmation, « Déplacer vers… ».
- **Glisser-déposer** avant, après ou dans un dossier ; ⌥↑ / ⌥↓ pour réordonner. Seule la ligne `info.seq` (ou `info.name`) des fichiers concernés change.
- Un fichier n'est jamais écrasé ni écrit à travers un lien symbolique ; un renommage ou un déplacement met à jour `.oc-sync/openapi/source.yml` pour que la synchro suive la requête.

## Rechargement à chaud

Un `git pull`, un `git checkout` ou un éditeur externe qui modifie la collection est pris en compte sans rien cliquer : l'arbre, les environnements et les variables sont relus au bout d'une fraction de seconde.

- Un onglet **sans brouillon** adopte le fichier modifié ; un message le dit.
- Un onglet **avec brouillon** garde ton brouillon et passe à « Modifié sur le disque » (point bleu, bouton **Recharger**). L'enregistrer demande confirmation, parce qu'il remplacerait le changement du disque ; **Recharger** le remplace, après confirmation, par le fichier.
- Un fichier supprimé ferme son onglet propre ; un onglet avec brouillon reste ouvert, barré, pour ne pas perdre le travail.
- `.git/`, `.oc-sync/`, `node_modules/` et les fichiers temporaires d'éditeur sont ignorés. Si le système ne peut plus surveiller le dossier, un message l'indique et « Relire le dossier » (↻) reste disponible.
- L'enregistrement relit le fichier avant d'écrire : un changement que la surveillance aurait manqué demande la même confirmation. Au retour de la fenêtre au premier plan, les onglets ouverts sont relus.

## Environnements

La vue **Environnements** (barre d'activité) liste les fichiers de `environments/` et permet de les gérer sans quitter l'application.

- **Créer, renommer, dupliquer, supprimer** : le bouton `+` de l'en-tête, ou le menu contextuel d'une ligne (clic droit, touche menu, Maj+F10). Un nom pris reçoit un suffixe au lieu de remplacer un fichier ; la suppression passe par la corbeille du système. **Ouvrir par défaut** choisit l'environnement que la collection ouvre au démarrage (`opencollection.yml`), qui suit un renommage et se vide à la suppression.
- **Variables** : un tableau éditable (activer, nom, valeur, supprimer, ajouter). Enregistrer (⌘S) ne change que ce qui a changé : types de valeurs, descriptions, `extends`, `color` et clés inconnues du fichier restent tels quels, comme le BOM et les fins de ligne ; les commentaires et la mise en forme d'un fichier écrit à la main sont normalisés dès qu'une écriture a lieu. Un nom manquant ou en double empêche d'enregistrer.
- **Secrets** : une variable `secret: true` n'a jamais sa valeur dans le fichier ; elle s'affiche « hors fichier ». La saisie dans le trousseau est prévue en V1 ; `{{process.env.NOM}}` lit déjà le fichier `.env`.
- **Brouillon** : les modifications n'agissent sur aucune requête tant qu'elles ne sont pas enregistrées (un point sur l'onglet et sur la barre d'activité les signale). Changer d'environnement ou ouvrir une autre collection les fait confirmer ; un fichier modifié sur le disque entre-temps ne sera écrasé qu'après confirmation (voir « Rechargement à chaud »).

## Importer

- **Coller un cURL** dans la barre d'URL : une commande qui commence par `curl` n'est pas collée telle quelle, elle est analysée comme Bruno le fait puis appliquée à la requête ouverte (URL, méthode, puis en-têtes, corps et authentification s'ils sont présents). L'onglet passe à « non enregistré ».
- **Nouvelle requête depuis cURL…** (palette ⌘⇧P, ou écran « Aucune requête ouverte ») : crée un fichier de requête dans le dossier choisi de la collection.
- **Importer une spec OpenAPI…** (palette ⌘⇧P, écran d'accueil ou « Aucune requête ouverte ») : OpenAPI 3.0 / 3.1 ou Swagger 2.0, fichier `.yaml`, `.yml`, `.json` ou URL ; aperçu, regroupement par tags ou par chemins, puis nouvelle collection dans le dossier parent choisi. La source de la spec et une copie brute (la base de la synchro) sont gardées dans `.oc-sync/openapi/`, à versionner avec la collection ; `.oc-sync` est ajouté à la liste `ignore` de la collection pour que Bruno ne l'affiche pas.
- **Synchroniser avec la spec** (vue « Synchro OpenAPI » de la barre d'activité, ou palette) : compare la nouvelle version de la spec à la base et à la collection, fusionne champ par champ, et te laisse arbitrer chaque conflit (garder l'équipe, prendre la spec, combiner, éditer) avant d'écrire quoi que ce soit. Une collection existante (créée par Bruno par exemple) peut être connectée à une spec : la première synchro se fait sans base.

Dans le navigateur (`npm start`), ces actions sont limitées à un aperçu : l'analyse et l'écriture se font dans l'application desktop.

## Réglages

Le bouton réglages de la barre d'activité, la commande « Ouvrir les réglages » de la palette ou ⌘, (Ctrl+, ailleurs) ouvrent un écran dans la zone éditeur : les sections à gauche, la section choisie à droite. Pour l'instant, **Apparence** :

- le thème clair ou sombre ;
- la famille et la taille de la police de l'**interface** (11 à 18 px, 13 par défaut), de l'**éditeur de requête** (barre d'URL, corps, valeurs des tableaux, scripts ; les éditeurs de la synchro le suivent) et du **résultat** (corps, en-têtes et timeline de la réponse), de 10 à 24 px, 12,5 par défaut.

Les familles sont des préréglages hors ligne (Inter et JetBrains Mono, embarquées, ou la police du système) ou le nom d'une police installée, par exemple « Fira Code » : aucune police n'est téléchargée, et une police introuvable laisse la police par défaut. Les changements s'appliquent tout de suite et restent sur l'ordinateur, dans le `localStorage` de l'application (clé `xc-settings`, comme `xc-theme` pour le thème), jamais dans une collection ; « Rétablir les valeurs par défaut » existe pour chaque groupe.

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
| Réglages | ⌘, | Ctrl+, |
| Arbre : naviguer, ouvrir ou replier un dossier | ↑ ↓ ← → Début Fin | idem |
| Arbre : renommer | F2 | F2 |
| Arbre : dupliquer | ⌘D | Ctrl+D |
| Arbre : supprimer (corbeille) | ⌘⌫ | Suppr |
| Arbre : réordonner | ⌥↑ / ⌥↓ | Alt+↑ / Alt+↓ |

Dans la palette : ↑ et ↓ pour naviguer (en boucle), ↵ pour exécuter, Échap pour fermer ; le préfixe `>` ne garde que les commandes. La recherche est floue (lettres dans l'ordre, sans casse ni accents) sur le nom, l'URL et le chemin du fichier des requêtes, dont l'URL vient de l'arbre de la collection sans relire les fichiers.

## Licence

Au choix, sous licence [MIT](LICENSE-MIT) ou [Apache 2.0](LICENSE-APACHE).
