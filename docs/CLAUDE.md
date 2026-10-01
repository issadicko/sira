# Handoff — Client API desktop offline-first (parité Bruno)

Tu reprends un projet cadré dont le Lot 0, le MVP-α et le MVP-β sont livrés ; le MVP-γ (synchro OpenAPI à 3 voies) est le prochain lot (voir « Avancement » dans `docs/roadmap.md` et `docs/etude-synchro-openapi-bruno.md`). Ton rôle est d'implémenter le produit décrit dans `docs/cahier-des-charges.md`, en suivant l'interface de `design/`.

## À lire, dans cet ordre

1. `docs/cahier-des-charges.md` : la source de vérité. Exigences identifiées `EF-*` (fonctionnelles) et `ENF-*` (non fonctionnelles), lots MVP / V1 / V2 / V3, gates, risques.
2. `design/README.md` : identité visuelle, tokens clair et sombre, description des écrans.
3. `design/maquette-v2/` et `../DESIGN.md` : maquette de référence (inspirée de Bruno, Postman, VS Code et Fleet) et système visuel. La v1 (`design/maquette/`) est remplacée.
4. `docs/roadmap.md` : lots, avancement, définition du MVP testable.

## Décisions déjà prises (ne pas rediscuter sans validation)

- Stack : Rust (workspace Cargo `core`, `engine`, `script`, `sync`, `cli`, `app`) + Tauri 2. Front Angular zoneless avec signals, CodeMirror 6.
- Format natif : **OpenCollection YAML**, strictement conforme à la spec publique de Bruno. Aucun champ propriétaire dans les fichiers de requête. Nos métadonnées vont dans `.oc-sync/`.
- Scripts : QuickJS via `rquickjs`, API compatible Bruno (`bru`, `req`, `res`).
- Synchro OpenAPI : fusion à 3 voies (base / ours / theirs), jamais d'écrasement silencieux, conflits arbitrés par l'utilisateur.
- Offline-first, zéro télémétrie, secrets hors fichiers (trousseau OS).

## Par où commencer

Le **Lot 0 (Fondations)**, sans interface :

1. Workspace Cargo et crates vides avec CI (build, tests, clippy, fmt) sur Windows, macOS, Linux.
2. `core` : lecture et écriture OpenCollection YAML, test aller-retour sans diff (`ENF-COMP-02`).
3. `engine` : envoi HTTP avec timings DNS, TCP, TLS, TTFB (`EF-RES-02`).
4. `cli` : `run` minimal sur une requête.
5. Bench de performance en CI (`ENF-PERF-*`).

Gate 1 à franchir avant le MVP : un corpus de collections Bruno publiques est relu puis réécrit sans aucun diff.

## Conventions de code

- Pas de commentaires au milieu du code : le code doit se lire seul. Commentaires limités au strict nécessaire.
- Clean Code, KISS, DRY, OCP, YAGNI.
- Style simple et concis, pas de sur-ingénierie.
- Documentation, messages de commit et échanges en français.
- Chaque exigence implémentée a au moins un test qui porte son identifiant (ex. `ef_req_01_...`).

## Agents

- Toute analyse ou étude confiée à des agents (exploration du code, revue, vérification, recherche) utilise en priorité des modèles légers, choisis selon la complexité de la tâche : **Haiku** pour les tâches simples (recherche de fichiers, lecture ciblée, vérifications mécaniques), **Sonnet 5.5** pour les tâches complexes (revue de code, analyse croisée, vérification adversariale). Un modèle plus lourd ne s'utilise que si ces deux-là ne suffisent pas, en le justifiant.

## Points ouverts

- Nom du produit : non défini (`[NOM]` dans la maquette).
- Taille de l'équipe : le planning (22 semaines jusqu'à la V1) suppose deux développeurs à temps plein, hypothèse non confirmée.
- Bruno gère-t-il déjà une fusion OpenAPI non destructive ? À vérifier dans la doc Bruno avant de le présenter comme différenciant.
- Crate YAML : `serde_yaml` est archivé, choisir une alternative maintenue au Lot 0.
