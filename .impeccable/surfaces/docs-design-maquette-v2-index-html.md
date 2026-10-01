---
version: 1
slug: "docs-design-maquette-v2-index-html"
primary_target: "docs/design/maquette-v2/index.html"
related_targets: []
---

# Maquette v2 — application desktop (espace de travail, synchro OpenAPI, palette, environnements, Git)

Mode : Operate. Audience : développeurs backend et QA, sessions longues, écran de portable, clavier d'abord.
Tâche : composer, envoyer, lire une requête ; arbitrer une fusion OpenAPI à 3 voies ; gérer variables et commits sans quitter l'outil.
Contraintes : sombre d'abord, clair dérivé au même niveau ; accent violet conservé ; IDE dense ; aucun champ inventé hors cahier des charges.
Références épinglées par le brief (remplacent le tirage concept-seed) : Bruno, Postman, VS Code, Fleet.

## Direction contract

THESIS: Un IDE d'API, pas un SaaS. Refuse l'arrangement « carte flottante sur fond crème avec gros header et KPI » de la v1 : la donnée (URL, JSON, diff) occupe l'écran, le chrome se tait.

OWN-WORLD: Cadre sombre #0D0D10 portant des îlots #16161A arrondis à 10 px séparés de 6 px (Fleet) ; barre d'activité et barre d'état fonctionnelles (VS Code) ; barre d'URL à méthode colorée et pastilles de variables survolables (Postman) ; fil d'Ariane du fichier YAML toujours visible (Bruno). Inter 13 px + JetBrains Mono. Violet = sélection et action ; ambre réservé aux conflits ; méthodes avec leur propre teinte.

STORY: Le développeur voit quel fichier il édite, envoie, lit la réponse et sa timeline sans défiler ; un conflit de spec se lit et se tranche dans un éditeur de fusion, jamais en silence.

FIRST VIEWPORT: Barre de titre 40 px avec centre de commande ⌘K au centre et environnement à droite ; barre d'activité 44 px ; îlot collections 264 px ; îlot éditeur : onglets 34 px, fil d'Ariane, barre d'URL 40 px, requête et réponse côte à côte sur toute la hauteur (≥ 30 lignes de JSON visibles) ; barre d'état 24 px. Action principale : Envoyer, à droite de l'URL.

FORM: Fusion épinglée Bruno × Postman × VS Code × Fleet (brief utilisateur, pas de tirage). Interaction signature : l'échelle de précédence des variables, partagée entre le survol d'une pastille {{var}} et la vue Environnements ; seconde : l'éditeur de fusion 3 voies dont le volet Résultat se recompose à chaque arbitrage. Mouvement : 120–200 ms, ease-out, état uniquement. Seed key : pinned-brief (pas de concept-seed).

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
