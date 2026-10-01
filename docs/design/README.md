> Remplacé pour tout nouveau travail par `maquette-v2/` (prototype interactif : `index.html`, liens `#collections`, `#env`, `#scm`, `#sync`, `#palette`, `?theme=light`) et par `DESIGN.md` à la racine. Ce qui suit décrit la v1.

# Design — maquette et identité visuelle

Quatre écrans de 1440 × 900, en clair et en sombre :

| Fichier | Écran |
| --- | --- |
| `maquette/Main.dc.html` | Espace de travail, thème clair (bascule clair/sombre intégrée) |
| `maquette/MainDark.dc.html` | Espace de travail, thème sombre |
| `maquette/Sync.dc.html` | Synchronisation OpenAPI et arbitrage des conflits, thème clair |
| `maquette/SyncDark.dc.html` | Synchronisation OpenAPI, thème sombre |

## Lire les fichiers `.dc.html`

Ce sont des composants au format « Design Component » : du HTML avec des styles inline, des trous `{{ variable }}` et une classe `Component` dont `renderVals()` fournit les valeurs (tokens de thème, données, handlers). Le runtime `support.js` n'est pas fourni : ces fichiers servent de **spécification visuelle**, pas de code à réutiliser tel quel. Les dimensions, espacements, rayons et couleurs sont exacts.

- `<sc-for list="{{x}}" as="item">` : boucle.
- `<sc-if value="{{cond}}">` : condition.
- `<dc-import name="Main" dark="{{ true }}">` : réutilise un écran avec une prop.

## Typographie

| Usage | Police | Graisses |
| --- | --- | --- |
| Logo, titres, grands chiffres | Bricolage Grotesque | 500, 700 |
| Interface | Figtree | 400, 500, 600, 700 |
| Code, URL, méthodes HTTP | JetBrains Mono | 400, 500, 700 |

Taille de base 13 px. Titres d'écran 21 px, chiffres clés 34 px.

## Tokens de couleur

| Token | Clair | Sombre | Usage |
| --- | --- | --- | --- |
| `bg` | `#F4F2EC` | `#111113` | Fond de l'application |
| `panel` | `#FFFFFF` | `#18181C` | Panneaux, barres |
| `raised` | `#FBFAF7` | `#1F1F24` | Boutons secondaires |
| `sunken` | `#F1EEE7` | `#0E0E10` | Champs, pastilles neutres |
| `line` | `#E3DFD4` | `#2C2C33` | Bordures |
| `ink` | `#1C1B18` | `#EEECE6` | Texte principal |
| `muted` | `#5F5B52` | `#A6A298` | Texte secondaire |
| `accent` | `#5A48E0` | `#9486FF` | Action principale, POST, sélection |
| `accentSoft` | `#EEEBFD` | `#25214A` | Fond d'élément sélectionné, variables |
| `good` | `#1C7A4B` | `#5BD08A` | GET, succès, 2xx, ajouts de diff |
| `warn` | `#A6520A` | `#F5A524` | PATCH, conflits |
| `bad` | `#B8342A` | `#F2766B` | DELETE, erreurs, suppressions de diff |
| `code` | `#FBFAF6` | `#141417` | Fond des blocs de code |

Chaque couleur de statut a une variante `*Soft` pour les fonds de pastilles (voir `tokens()` dans chaque fichier).

## Règles de forme

- Rayons : 8 à 11 px pour les boutons, 12 à 14 px pour les champs, 18 px pour les panneaux, pastilles en `999px`.
- Méthodes HTTP en JetBrains Mono 10,5 px gras, colorées selon le token.
- Variables `{{var}}` affichées comme pastilles `accentSoft` / `accent`.
- Requête dépréciée : barrée, badge « dépréciée », jamais masquée.
- Ton des micro-textes léger et direct (« Tout est vert, beau travail. »).
- Icônes en SVG trait (stroke 2), jamais d'emoji.
- Cibles cliquables d'au moins 34 px de haut dans les listes, 44 px pour les actions principales.
