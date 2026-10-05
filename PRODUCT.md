# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

Application desktop Tauri 2 (webview système) sur Windows, macOS et Linux. Les maquettes sont du HTML/CSS ; aucune version web ou mobile n'est prévue.

## Stack

Rust (workspace Cargo `core`, `engine`, `script`, `sync`, `watch`, `cli`, `app`) + Tauri 2. Front Angular zoneless avec signals, éditeur CodeMirror 6. Maquettes de conception : HTML/CSS/JS statiques dans `docs/design/`.

## Users

Développeurs backend et QA qui conçoivent, testent et documentent des API REST (puis GraphQL, WebSocket, gRPC) au quotidien, en équipe, avec Git comme seul moyen de partage. Ils passent des heures par jour dans l'outil, à côté de leur IDE, souvent sur un seul écran d'ordinateur portable.

## Product Purpose

Client API desktop offline-first et Git-friendly qui atteint la parité fonctionnelle avec Bruno sur le cœur d'usage, puis le dépasse sur la performance native (Rust) et la synchronisation OpenAPI non destructive. Le succès : une collection Bruno s'ouvre, s'exécute et se réenregistre sans aucun diff, et l'outil reste instantané sur de grosses collections.

## Positioning

Même format que Bruno (OpenCollection YAML, un fichier par requête, aucun compte, aucun cloud), mais moteur natif Rust plutôt qu'Electron, et une synchro OpenAPI à 3 voies (base / équipe / spec) qui fusionne au lieu d'écraser : un conflit n'est jamais tranché automatiquement.

## Operating Context

- Collections = dossiers versionnés avec Git ; rechargement à chaud quand Git ou un éditeur externe modifie un fichier.
- Environnements (dev, recette, prod), variables à portées multiples avec précédence identique à Bruno, secrets dans le trousseau OS ou `.env` local.
- Spec OpenAPI de l'équipe backend importée puis resynchronisée à chaque évolution du contrat.
- CLI `run` et `sync --check` en CI.

## Capabilities and Constraints

- Exigences identifiées `EF-*` et `ENF-*` dans `docs/docs/cahier-des-charges.md` (source de vérité).
- Aucun champ propriétaire dans les fichiers de requête ; métadonnées dans `.oc-sync/`.
- Zéro télémétrie, aucun appel sortant non demandé.
- Interface en français et anglais, thèmes clair et sombre, raccourcis clavier, palette de commandes.
- Nom du produit : **Sira** (décidé le 1er octobre 2026), « le chemin » en dioula et en bambara.

## Brand Commitments

- Accent violet conservé comme signature (`#5A48E0` clair / `#9486FF` sombre).
- Ton des micro-textes léger et direct, tutoiement.
- Icônes SVG au trait, jamais d'emoji.
- Requête dépréciée : barrée, badge « dépréciée », jamais masquée.
- Références d'interface revendiquées : Bruno, Postman, VS Code, JetBrains Fleet.

## Evidence on Hand

Aucune donnée réelle, aucun client, aucun benchmark publié. Les exemples de maquette (API Paiements, montants en XOF) sont des données synthétiques de démonstration.

## Product Principles

1. Le disque est la source de vérité : l'interface montre toujours quel fichier on édite.
2. Non destructif : aucune opération automatique n'écrase le travail de l'équipe ; en cas de doute, un conflit visible.
3. Rapide par défaut : l'interface ne fige jamais, même sur une réponse de 10 Mo.
4. Offline-first : tout fonctionne sans réseau, hors l'envoi de la requête.
5. Clavier d'abord : chaque action fréquente a un raccourci et passe par la palette.
