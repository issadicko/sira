# Documentation Markdown (EF-DOC-01)

Chaque requête, chaque dossier et la collection elle-même portent une documentation en Markdown, écrite dans la clé `docs` de leur fichier, au format d'OpenCollection :

| Niveau | Fichier | Où l'écrire dans l'application |
|---|---|---|
| Requête | le fichier de la requête | onglet **Docs** de la requête, boutons *Édition* et *Aperçu* |
| Dossier | `folder.yml` du dossier | menu du dossier › **Documentation…** |
| Collection | `opencollection.yml` | menu `+` de l'arbre › **Documentation…**, ou la palette (`Documentation de la collection…`) |

## Dans le fichier

Une documentation de dossier ou de collection s'écrit en table, comme le fait Bruno :

```yaml
docs:
  content: |-
    # Clients

    Tout sur les clients.
  type: text/markdown
```

Les deux formes sont lues pour les trois niveaux : le texte seul (`docs: Notes`) ou la table `{ content, type }`. **La forme existante est conservée** quand on enregistre : une requête dont `docs` est un texte reste un texte, une table garde son `type`. Sans documentation existante, un dossier ou une collection reçoit la table, une requête le texte. Vider la documentation retire la clé ; le reste du fichier (variables, scripts, clés inconnues, fins de ligne CRLF, BOM) est conservé, et un texte inchangé n'écrit rien.

Un dossier sans `folder.yml` en reçoit un à la première documentation enregistrée, avec son nom (`info.name`, `info.type: folder`). Rien n'est créé tant que le texte est vide.

## Aperçu

L'aperçu comprend le Markdown courant : titres, paragraphes, **gras**, *italique*, ~~barré~~, `code`, blocs de code (avec leur langue), listes à puces, numérotées et imbriquées, citations, tableaux avec alignements, règles horizontales et liens.

Il n'interprète **aucun HTML** : `<script>` ou `<b>` restent du texte. Les liens ne s'ouvrent que pour `http://`, `https://` et `mailto:` (dans une autre fenêtre, sans `opener`) ; `javascript:`, `data:` et les chemins relatifs sont montrés comme du texte simple.

Le dialogue d'un dossier ou d'une collection demande avant d'abandonner un texte non enregistré ; `Ctrl+↵` enregistre.

## Exporter la documentation

L'export de la collection propose le format **Documentation Markdown**, dans l'application (menu d'export) et en ligne de commande :

```
xc export ma-collection --format markdown --output documentation.md
```

Le fichier contient, dans l'ordre de l'arbre : le titre et la documentation de la collection, puis chaque dossier (un titre par niveau, jusqu'au sixième) avec sa documentation, et chaque requête avec son nom, une ligne `` `GET {{baseUrl}}/clients` `` (méthode ou type `GRAPHQL`, `GRPC`, `WS`, puis adresse telle qu'écrite, variables non résolues) et sa documentation. Ni les en-têtes, ni les corps, ni l'authentification, ni les environnements n'y figurent. Une requête illisible est écartée et signalée sur la sortie d'erreur ; un fichier existant n'est remplacé qu'avec `--force`.

## Pas encore

Documentation à plusieurs onglets, images et liens relatifs vers d'autres fichiers de la collection, génération d'un site HTML, coloration des blocs de code.
