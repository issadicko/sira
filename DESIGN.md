---
name: "[NOM] · Client API desktop"
description: "Un IDE d'API sombre d'abord, en îlots : la donnée occupe l'écran, le chrome se tait."
colors:
  frame: "#0c0c0f"
  island: "#151519"
  island-line: "#1f1f25"
  pop: "#1b1b21"
  raised: "#1d1d23"
  hover: "#212128"
  sunken: "#101013"
  line: "#25252c"
  line-strong: "#34343d"
  ink: "#e8e8ec"
  muted: "#a4a4ad"
  faint: "#878791"
  accent: "#9486ff"
  accent-hi: "#aea5ff"
  accent-soft: "rgba(148, 134, 255, .15)"
  accent-line: "rgba(148, 134, 255, .5)"
  on-accent: "#0f0d22"
  good: "#5bd08a"
  good-soft: "rgba(91, 208, 138, .13)"
  good-strong: "rgba(91, 208, 138, .3)"
  warn: "#f5a524"
  warn-soft: "rgba(245, 165, 36, .12)"
  on-warn: "#1d1404"
  bad: "#f2766b"
  bad-soft: "rgba(242, 118, 107, .13)"
  bad-strong: "rgba(242, 118, 107, .3)"
  info: "#6cb6ff"
  info-soft: "rgba(108, 182, 255, .12)"
  m-get: "#5bd08a"
  m-post: "#ada3ff"
  m-put: "#6cb6ff"
  m-patch: "#4fd3c1"
  m-delete: "#f2766b"
  t-key: "#b9b0ff"
  t-str: "#7fd8a0"
  t-num: "#e8c27c"
  t-kw: "#6cb6ff"
  t-com: "#8b8b95"
  frame-light: "#e5e5ea"
  island-light: "#ffffff"
  island-line-light: "#d9d9e0"
  pop-light: "#ffffff"
  raised-light: "#f4f4f7"
  hover-light: "#eeeef2"
  sunken-light: "#f9f9fb"
  line-light: "#e4e4ea"
  line-strong-light: "#cdcdd6"
  ink-light: "#1a1a1f"
  muted-light: "#55555f"
  faint-light: "#70707a"
  accent-light: "#5a48e0"
  accent-hi-light: "#4a38cf"
  accent-soft-light: "rgba(90, 72, 224, .09)"
  accent-line-light: "rgba(90, 72, 224, .45)"
  on-accent-light: "#ffffff"
  good-light: "#17784a"
  warn-light: "#9c4d08"
  warn-soft-light: "rgba(214, 120, 18, .12)"
  on-warn-light: "#ffffff"
  bad-light: "#b3322a"
  info-light: "#1d6abd"
  m-patch-light: "#0b7a6e"
  t-com-light: "#696973"
typography:
  body:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.45
    fontFeature: "'cv11', 'ss03'"
  title:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    letterSpacing: "-0.005em"
  headline:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "12.5px"
    fontWeight: 600
  label:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "11.5px"
    fontWeight: 500
  code:
    fontFamily: "'JetBrains Mono', ui-monospace, 'SF Mono', Menlo, Consolas, monospace"
    fontSize: "12.5px"
    fontWeight: 400
    lineHeight: "20px"
    fontFeature: "'calt' 0, 'liga' 0"
  url:
    fontFamily: "'JetBrains Mono', ui-monospace, monospace"
    fontSize: "13px"
    fontWeight: 400
    fontFeature: "'calt' 0, 'liga' 0"
  method:
    fontFamily: "'JetBrains Mono', ui-monospace, monospace"
    fontSize: "10px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.01em"
rounded:
  xs: "4px"
  sm: "6px"
  chip: "7px"
  md: "8px"
  overlay: "9px"
  island: "10px"
  pill: "13px"
spacing:
  gap: "6px"
  row: "26px"
  control: "28px"
  url: "36px"
  titlebar: "40px"
  activity: "44px"
  sidebar: "264px"
components:
  island:
    backgroundColor: "{colors.island}"
    rounded: "{rounded.island}"
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.sm}"
    padding: "0 12px"
    height: "28px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hi}"
  button-send:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.md}"
    padding: "0 12px 0 14px"
    height: "36px"
  button:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
    padding: "0 10px"
    height: "28px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    rounded: "{rounded.sm}"
    padding: "0 10px"
    height: "28px"
  url-field:
    backgroundColor: "{colors.sunken}"
    textColor: "{colors.ink}"
    typography: "{typography.url}"
    rounded: "{rounded.md}"
    height: "36px"
  tab:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    rounded: "{rounded.sm}"
    padding: "0 4px 0 10px"
    height: "28px"
  tab-selected:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.ink}"
  tree-row:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
    height: "26px"
  tree-row-active:
    backgroundColor: "{colors.accent-soft}"
  var-chip:
    backgroundColor: "{colors.accent-soft}"
    textColor: "{colors.accent}"
    rounded: "{rounded.xs}"
    padding: "0 5px"
    height: "21px"
  var-chip-dynamic:
    backgroundColor: "{colors.info-soft}"
    textColor: "{colors.info}"
  var-chip-env:
    backgroundColor: "{colors.good-soft}"
    textColor: "{colors.good}"
  var-chip-unresolved:
    backgroundColor: "{colors.bad-soft}"
    textColor: "{colors.bad}"
  tag:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.muted}"
    typography: "{typography.label}"
    rounded: "{rounded.xs}"
    padding: "0 6px"
    height: "17px"
  tag-spec:
    backgroundColor: "{colors.info-soft}"
    textColor: "{colors.info}"
  spec-chip:
    backgroundColor: "{colors.warn-soft}"
    textColor: "{colors.warn}"
    rounded: "{rounded.pill}"
    height: "26px"
  status-bar-item:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    typography: "{typography.label}"
    rounded: "{rounded.xs}"
    padding: "0 6px"
    height: "20px"
---

# Design System: [NOM] · Client API desktop

> Ce fichier décrit le monde visuel de la maquette v2 (`docs/design/maquette-v2/`). Pour tout nouveau travail, il **remplace** l'identité et les tokens v1 décrits dans `docs/design/README.md` (fond crème, cartes flottantes) ; ce README et la maquette v1 restent comme archive, non comme référence.

## Overview

**Creative North Star: « L'atelier en îlots »**

Un IDE d'API, pas un SaaS. Un cadre sombre presque noir porte des îlots arrondis séparés de 6 px (à la Fleet) ; à l'intérieur, la donnée occupe tout : URL en chasse fixe, JSON sur toute la hauteur, diffs, fichiers YAML. Le chrome (barre de titre, barre d'activité, barre d'état) est fonctionnel, discret et toujours présent, comme dans VS Code. La densité est celle d'un éditeur de code : lignes de 26 px, contrôles de 28 px, texte de 13 px.

La couleur est un langage d'état, jamais de la décoration. Le violet dit « sélection et action », l'ambre dit « conflit à trancher », le rouge dit « erreur ou production », le vert dit « succès ou valeur d'environnement », le bleu dit « dynamique ou issu de la spec ». Chaque méthode HTTP porte sa propre teinte. Le thème sombre est le thème de référence ; le thème clair est dérivé au même niveau d'exigence, avec les mêmes rôles.

Le mouvement ne sert qu'à signaler un changement d'état (120 à 200 ms, ease-out), et disparaît sous `prefers-reduced-motion`.

**Key Characteristics:**
- Cadre + îlots : profondeur par la valeur tonale, pas par l'ombre.
- Deux familles : Inter pour l'interface, JetBrains Mono pour tout ce qui est donnée (URL, méthodes, chemins, code, valeurs).
- Couleur sémantique stricte, accent violet unique.
- Densité IDE, clavier d'abord, raccourcis visibles en `kbd`.
- Le fichier édité est toujours nommé (fil d'Ariane en mono sous les onglets).

## Colors

Une palette de gris froids très légèrement bleutés, un seul accent violet, et quatre couleurs d'état qui ne servent qu'à dire un état.

### Primary
- **Violet signature** (accent, `#9486ff` sombre / `#5a48e0` clair) : bouton Envoyer et actions primaires, indicateur de sous-onglet actif, cases cochées, curseur de saisie, anneau de focus (via accent-line), pastilles `{{var}}`, nom de collection dans l'arbre, correspondances de recherche. Survol : accent-hi. Fond de sélection : accent-soft (ligne d'arbre active, entrée de palette, barreau gagnant de l'échelle).

### Secondary
- **Ambre de conflit** (warn) : exclusivement les conflits de synchro OpenAPI et ce qui y mène (pastille « Spec OpenAPI : n conflits », badge d'activité, lignes non arbitrées du Résultat, état « à arbitrer »).
- **Rouge d'alerte** (bad) : erreurs (statut 4xx/5xx, test échoué, JSONPath invalide), variable non résolue, environnement de production, suppressions de diff, méthode DELETE.
- **Vert de réussite** (good) : statut 2xx, tests passés, ajouts de diff, variables d'environnement, environnement dev, état « arbitré ».
- **Bleu d'information** (info) : variables dynamiques, éléments issus de la spec (tag « spec », volet Spec de la fusion), environnement de recette, fichier Git modifié.

### Tertiary
- **Teintes de méthode** : GET vert (m-get), POST violet clair (m-post), PUT bleu (m-put), PATCH turquoise (m-patch), DELETE rouge (m-delete), toujours en mono 600, abrégé `DEL` dans l'arbre.
- **Coloration syntaxique** : clés violet pâle (t-key), chaînes vertes (t-str), nombres ocre (t-num), mots-clés bleus (t-kw), commentaires gris italiques (t-com).

### Neutral
- **Cadre** (frame) : le fond de la fenêtre, sous les îlots, la barre de titre et la barre d'état.
- **Îlot** (island) et son liseré (island-line) : chaque surface de travail.
- **Flottant** (pop) : palette, menus, popovers, toast.
- **Relevé** (raised) : onglet sélectionné, en-têtes de tableaux, bouton secondaire, tête du volet Résultat.
- **Survol** (hover) : fond de survol de toute ligne et de tout bouton fantôme.
- **Creusé** (sunken) : champs de saisie, barre d'URL, filtre, blocs de code encadrés, pistes de timeline.
- **Filets** (line, line-strong) : séparateurs internes ; line-strong pour les bordures de champ et de surfaces flottantes.
- **Encre / atténué / pâle** (ink, muted, faint) : trois niveaux de texte, pas un de plus.

### Named Rules
**The Amber Is Conflict Rule.** L'ambre ne signifie qu'une chose : un conflit de spec qui attend un arbitrage. Jamais pour un avertissement générique, jamais pour décorer.

**The Violet Means Act Rule.** Le violet marque ce qui est sélectionné ou ce qu'on peut déclencher. Une surface en accent plein est rare : le bouton primaire, le badge d'activité, la marque.

**The Red Prod Rule.** L'environnement de production est rouge partout où il apparaît (bouton d'environnement teinté bad-soft avec bordure bad-strong, point rouge, tag « production »), pour qu'on ne l'envoie jamais par mégarde.

**The Three Inks Rule.** Le texte n'a que trois niveaux : ink, muted, faint. Pas d'opacité arbitraire sur du texte, sauf l'état désactivé d'une ligne kv (.45).

## Typography

**Body Font:** Inter (avec -apple-system, Segoe UI Variable, system-ui)
**Label/Mono Font:** JetBrains Mono (avec ui-monospace, SF Mono, Menlo, Consolas)

**Character:** Inter neutre et compact pour l'interface, avec les variantes `cv11` et `ss03` actives ; JetBrains Mono pour toute donnée, ligatures et alternates contextuelles désactivées (`"calt" 0, "liga" 0`) pour que `|-`, `=>` ou `!=` restent lus tels qu'écrits.

### Hierarchy
- **Title** (600, 15px, interligne 1.45, -0.005em) : titre de vue (« Environnement dev »), titre markdown de documentation.
- **Headline** (600, 12.5px) : titre d'îlot (pane-title), titre de section, titre de groupe.
- **Body** (400, 13px, 1.45) : tout le texte d'interface ; 12.5px pour les cellules et notes, 12px pour les métadonnées secondaires.
- **Label** (500, 11.5px) : barre d'état, en-têtes de tableaux, groupes de palette, tags (10.5px), compteurs. Jamais en capitales, jamais espacé.
- **Code** (mono 400, 12.5px / 20px) : éditeurs, réponses, diffs, valeurs kv. Numéros de ligne en chiffres tabulaires.
- **URL** (mono 400, 13px) : barre d'URL ; méthode en mono 600 12px dans le sélecteur.
- **Method** (mono 600, 10px, 0.01em) : méthodes dans l'arbre, l'historique et la liste des opérations, colonne fixe de 34px.

### Named Rules
**The Data Is Mono Rule.** URL, méthode, chemin, nom de fichier, valeur de variable, code et durée sont en JetBrains Mono ; libellés, boutons et phrases sont en Inter. Le mélange dans une même ligne est normal (« Résultat » en Inter, chemin du fichier en mono).

**The No Ligature Rule.** Toute surface mono désactive les ligatures. Un client API montre exactement ce qui part sur le fil.

**The Tabular Numbers Rule.** Compteurs, durées, numéros de ligne et badges utilisent `tabular-nums`.

**The Settings Scale Rule.** Les tailles de l'interface se règlent dans Réglages > Apparence (11 à 18px, 13px par défaut) : `html { font-size }` les porte et chaque `font-size` de l'interface s'écrit `calc(N * var(--px))`, où N est la taille en px à 13px et `--px` vaut `1rem / 13` (13px s'écrit `1rem`). Espacements, hauteurs, rayons et icônes restent en px. Les éditeurs ont leurs propres réglages (10 à 24px, 12.5px par défaut) : la barre d'URL, CodeMirror, les valeurs des tableaux et les scripts lisent `--code-font` et `--code-size` (Éditeur de requête), la réponse les redéfinit (Résultat). `--font-mono` ne sert qu'au code hors éditeurs.

## Layout

**Coquille.** Grille de fenêtre en trois rangées : barre de titre 40px, établi, barre d'état 26px (items de 20px). Barre de titre en trois colonnes : feux et espace de travail à gauche, centre de commande ⌘K au centre (240 à 460px), pastille de spec et environnement à droite.

**Établi.** Colonnes : barre d'activité 44px (boutons de 36px, rayon 8px), îlot latéral 264px (redimensionnable), poignée de 6px, îlot éditeur flexible ; marge droite de 6px. Les îlots sont séparés par 6px de cadre visible.

**Îlot éditeur.** Rangée d'onglets 38px (onglets de 28px), fil d'Ariane mono 30px, barre d'URL 36px avec marge basse 10px, puis split requête / réponse côte à côte (requête 46 %, minimum 280px ; réponse minimum 320px ; poignée 7px) ou empilé (bascule de mise en page). Sous-onglets 36px avec indicateur violet de 2px. Corps de volet : 14px de marge, sections espacées de 20px.

**Barre latérale.** Tête 38px, filtre 28px, lignes d'arbre de 26px, retrait de 14px par niveau, guide vertical d'indentation qui n'apparaît qu'au survol de la barre latérale. Sections repliables de 32px.

**Vues.** En-tête de vue minimum 52px. Environnements : tableau + colonne de résolution de 340px. Fusion : volets Équipe / Spec (et Base optionnelle) au-dessus, Résultat en dessous (1 : 1.1), pied d'îlot 52px portant l'action d'application.

**Rythme.** Pas de 2, 4, 6, 8, 10, 12, 14px ; 6px est le pas structurel (écart entre îlots, marges de lignes, gaps de contrôles).

**Responsive.** Fenêtre ≤ 1180px : colonne de résolution à 280px, libellé long de la pastille de spec masqué. Fenêtre ≤ 1100px : barre latérale à 220px, split resserré (260 / 300px), résolution des environnements empilée sous le tableau. Les volets requête et réponse sont des conteneurs (`container-type: inline-size`) : ≤ 620px les compteurs des sous-onglets disparaissent (sauf ok / ko) ; ≤ 560px la taille de réponse, la colonne description des kv et les descriptions de paramètres disparaissent, sous-onglets resserrés ; ≤ 480px le texte du statut et l'icône de durée disparaissent. On retire de l'information secondaire, on ne réarrange jamais l'ordre.

### Named Rules
**The Visible Frame Rule.** Les îlots ne se touchent jamais : 6px de cadre les sépare toujours, et seul le cadre porte la barre de titre et la barre d'état.

**The File Is Named Rule.** Tout éditeur affiche le chemin du fichier sur disque dans le fil d'Ariane mono, avec son état (enregistré / modifié).

## Elevation & Depth

Le système est plat et tonal. La profondeur vient de l'empilement de valeurs : frame < sunken < island < pop / raised < hover (en sombre), avec un liseré island-line de 1px autour de chaque îlot qui passe à line-strong quand l'îlot a le focus. L'ombre est réservée aux surfaces qui flottent réellement au-dessus de l'établi.

### Shadow Vocabulary
- **Flottant** (`box-shadow: 0 12px 28px -10px rgba(0,0,0,.6), 0 2px 6px rgba(0,0,0,.35)` en sombre ; `0 12px 28px -10px rgba(24,24,36,.16), 0 1px 3px rgba(24,24,36,.08)` en clair) : palette, menus, popovers de variable, toast. Rien d'autre.
- **Segment pressé** (`0 1px 2px rgba(0,0,0,.18), inset 0 0 0 1px var(--line)`) : le bouton actif d'un contrôle segmenté.
- **Liseré intérieur** (`inset 0 0 0 1px`) : onglet sélectionné (line), entrée d'activité courante (island-line). C'est un trait, pas une élévation.

### Named Rules
**The Flat Islands Rule.** Les îlots n'ont jamais d'ombre. Seul ce qui flotte (palette, menu, popover, toast) en porte une, et c'est toujours la même.

## Shapes

Des rayons doux et hiérarchisés selon la taille : 4px pour les petites marques (tags, pastilles de variable, kbd, items de barre d'état), 6px pour les contrôles et lignes (boutons, onglets, lignes d'arbre, champs), 7px pour les contrôles segmentés et le centre de commande, 8px pour les grands contrôles et blocs (barre d'URL, Envoyer, tableaux kv, notes), 9px pour les surfaces flottantes, 10px pour les îlots et la palette. Les pastilles d'état sont des pilules (rayon moitié de hauteur). Les points d'état font 7px, ronds ; les pastilles de couleur des volets de fusion 9px à rayon 2px. Les bordures sont toujours de 1px ; la note informative utilise un tireté line-strong. Les icônes sont des SVG au trait (1.6, extrémités rondes), 12 à 16px.

## Components

### Buttons
- **Shape:** 28px de haut, rayon 6px ; variante large 36px, rayon 8px.
- **Primary:** fond accent, texte on-accent, 600 ; survol accent-hi. Le bouton Envoyer est la variante large (min. 128px) avec son raccourci en kbd translucide ; pendant l'envoi il passe en raised avec spinner violet, sans changer de place.
- **Secondary:** fond raised, bordure line-strong, 500 ; survol hover.
- **Ghost:** transparent, texte muted ; survol hover + ink. Boutons icône 28px (24px en petit).
- **Disabled:** texte faint, fond transparent ou raised, bordure line ; curseur interdit.

### Chips
- **Pastilles de variable `{{var}}`:** 21px, rayon 4px, 500, en ligne dans l'URL et le code. Quatre variantes : collection / requête (accent-soft + accent), dynamique (info-soft + info), environnement (good-soft + good), non résolue (bad-soft + bad, soulignée en vague bad-strong). Au survol ou en mise en avant : accent-line + ink, curseur aide, ouvre le popover de précédence.
- **Tags:** 17px, 10.5px 500, rayon 4px ; neutre (raised + muted), nouveau (accent), spec (info), succès, conflit, erreur. Le tag « spec » marque ce qui vient de la spec OpenAPI.
- **Pastille de spec:** pilule ambre de 26px dans la barre de titre, n'existe que s'il y a des conflits.
- **Puce d'état:** pilule 20px, 11.5px 500 ; ambre « non arbitré », vert « arbitré ».

### Cards / Containers
- **Îlot:** fond island, liseré island-line 1px, rayon 10px, pas d'ombre, contenu en colonne, débordement masqué.
- **Tableau kv:** bordure line, rayon 8px, rangées de 30px minimum séparées par des filets, cellules séparées par des filets verticaux, en-tête raised 11.5px muted. Colonnes : case 32px, clé, valeur, description (masquée sous 560px). Ligne désactivée à .45 ; ligne fantôme en faint ; ligne verrouillée (issue de la spec, icône cadenas) en muted.
- **Note:** tireté line-strong, rayon 8px, texte muted 12.5px, lien en accent. **Bannière:** fond raised, rayon 8px.

### Inputs / Fields
- **Style:** fond sunken, bordure line (line-strong pour l'URL et les selects), rayon 6px (8px pour l'URL), caret accent, placeholder faint.
- **Focus:** bordure accent-line + halo `0 0 0 3px` accent-soft. Focus clavier global : contour 2px accent-line décalé de 1px.
- **Error:** bordure bad-strong (JSONPath invalide).
- **Case à cocher:** 14px, rayon 4px, bordure 1.5px line-strong ; cochée en accent avec coche on-accent.

### Navigation
- **Barre d'activité:** icônes 36px en faint ; survol ink + hover ; vue courante : fond island avec liseré island-line (l'îlot « déborde » dans la barre). Badges 16px en pilule : accent pour un compte, ambre (on-warn) pour des conflits, détourés de 2px de frame.
- **Onglets d'éditeur:** 28px, muted ; sélectionné : raised + ink + liseré line. Onglet d'aperçu en italique. Fermeture 18px visible au survol ou si sélectionné ; un onglet modifié montre un point ink de 7px à la place de la croix tant qu'on ne le survole pas.
- **Sous-onglets:** texte muted, sélectionné en ink 500 avec un trait accent de 2px en bas ; compteurs en faint tabulaires, ok / ko colorés.
- **Arbre:** lignes 26px, rayon 6px, survol hover, active accent-soft + nom en 500. Collections en 600 avec icône accent. Dépréciée : barrée, faint, jamais masquée.
- **Barre d'état:** 11.5px muted sur le cadre ; items 20px cliquables (survol hover + ink), item ambre pour les conflits, vert pour un succès.

### Échelle de précédence (signature)
Liste verticale de barreaux reliés par un filet line-strong. Chaque barreau : point de 9px, libellé de portée, source en mono faint à droite, valeur en mono. Le barreau gagnant est sur accent-soft avec point accent et halo ; les barreaux masqués ont leur valeur barrée en muted ; les portées vides se réduisent à un point de 5px. La même échelle sert dans le popover de variable (version compacte) et dans la colonne de résolution des Environnements.

### Éditeur de fusion à 3 voies (signature)
Volets Équipe (pastille accent) et Spec (pastille info), Base optionnelle (pastille faint), Résultat (pastille ink, tête raised). Lignes en conflit teintées accent-soft côté Équipe, info-soft côté Spec ; dans le Résultat, une gouttière de 3px dit l'origine (accent, info, ambre si non arbitré, vert si édité à la main) et les lignes non arbitrées sont sur warn-soft. Au-dessus de chaque conflit du Résultat, une lentille 11.5px propose « Garder l'équipe · Prendre la spec · Combiner les deux · Éditer à la main » ; le choix actif passe en accent sur accent-soft et le Résultat se recompose. Les lignes identiques repliées s'affichent en italique faint sur raised. Le pied d'îlot porte l'état (ambre / vert), une note, Annuler et Appliquer.

### Diff
Deux colonnes avec têtes collantes 30px. Suppressions sur bad-soft avec gouttière `-` rouge, ajouts sur good-soft avec `+` vert, mots changés sur bad-strong / good-strong. Les lignes absentes d'un côté sont comblées par des hachures à -45° (filet line de 1px tous les 6px).

### Surfaces flottantes
- **Palette:** 620px max, à 48px du haut, fond pop, bordure line-strong, rayon 10px, ombre flottante, voile très léger. Champ 44px en 14px, groupes en label faint, entrées 32px, sélection accent-soft, correspondances en accent 600, pied 32px avec raccourcis.
- **Menu:** min. 260px, padding 4px, rayon 9px ; items 30px, coche accent, raccourci ou détail en mono faint à droite.
- **Popover de variable:** 320px, rayon 9px, nom en mono accent, valeur dans un bloc sunken, échelle compacte, pied avec source.
- **Toast:** centré à 40px du bas, rayon 9px, icône verte, `role="status"`.

### Réponse
Barre 36px : sous-onglets, puis statut (pilule mono 600 22px, verte en 2xx, rouge en 4xx / 5xx) et métriques mono tabulaires. Pendant l'envoi : barre de progression violette de 2px en haut, contenu à .35. Timeline : pistes sunken de 10px, phases DNS faint, TCP muted, TLS info, attente accent, téléchargement good.

## Do's and Don'ts

### Do:
- **Do** poser chaque surface de travail dans un îlot island à rayon 10px, séparé des autres par 6px de cadre.
- **Do** écrire URL, méthodes, chemins, fichiers, valeurs et code en JetBrains Mono sans ligatures.
- **Do** réserver l'ambre aux conflits de spec, le violet à la sélection et à l'action, le rouge aux erreurs et à la production.
- **Do** donner à chaque méthode HTTP sa teinte (m-get, m-post, m-put, m-patch, m-delete) en mono 600.
- **Do** limiter le mouvement aux changements d'état : 100 à 200 ms, courbe `cubic-bezier(.16, 1, .3, 1)` pour ce qui entre ; tout est neutralisé sous `prefers-reduced-motion`.
- **Do** garder le texte faint sur island, sunken ou pop, où il dépasse 4.5:1 (5.1:1 sur island sombre, 4.9:1 sur blanc).
- **Do** retirer l'information secondaire quand un volet rétrécit (requêtes de conteneur 620 / 560 / 480px), sans réordonner.
- **Do** dériver le thème clair rôle pour rôle depuis le sombre : mêmes noms de tokens, mêmes usages.

### Don't:
- **Don't** reprendre l'arrangement v1 : fond crème, carte flottante, gros en-tête, tuiles de KPI.
- **Don't** ombrer un îlot, un tableau ou un bouton ; l'ombre est réservée à la palette, aux menus, aux popovers et au toast.
- **Don't** utiliser l'ambre pour un simple avertissement, ni le violet pour un état d'erreur ou de succès.
- **Don't** trancher un conflit en silence : un conflit non arbitré reste visible en ambre jusqu'au choix.
- **Don't** masquer une requête dépréciée : barrée, badge, toujours dans l'arbre.
- **Don't** utiliser d'emoji ni de caractère décoratif comme icône ; les icônes sont des SVG au trait.
- **Don't** poser du texte faint sur le cadre ou sur raised / hover en thème clair (3.9 à 4.5:1).
- **Don't** écrire de libellé en capitales espacées ; les libellés restent en casse normale.
