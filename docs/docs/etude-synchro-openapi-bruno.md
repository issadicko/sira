# Étude — la synchro OpenAPI de Bruno face à notre § 6

1er octobre 2026 · sources de Bruno au commit `a2dbda9f` (30 sept. 2026) · référence : `cahier-des-charges.md` § 4 et § 6

Cette étude répond au point ouvert « Bruno gère-t-il déjà une fusion OpenAPI non destructive ? ». Réponse courte : **Bruno a une synchro OpenAPI, en bêta et désactivée par défaut, mais elle n'est ni à 3 voies ni non destructive.** Notre différenciation tient, à condition de la formuler précisément.

Abréviation : `S` = `packages/bruno-electron/src/ipc/openapi-sync.js`.

## Ce que fait Bruno

**Statut.** Fonction bêta, préférence `beta['openapi-sync']` à `false` par défaut (`bruno-electron/src/store/preferences.js:56`). Une fois activée, le renderer vérifie le hash de la spec distante toutes les 5 minutes. Swagger 2.0 est refusé (S:157).

**Stockage.**

- La configuration est dans `extensions.bruno.openapi[0]` de `opencollection.yml` : `sourceUrl`, `groupBy`, `lastSyncDate`, `specHash`, `autoCheck`, `autoCheckInterval`.
- La spec de référence (la « base ») est copiée **hors du dépôt**, dans le dossier de données de l'application (S:98-133). Un coéquipier qui clone n'a donc pas de base : le diff part d'une spec vide et tout apparaît « ajouté » (S:905, 985).
- `lastSyncDate` et `specHash` sont réécrits dans le fichier versionné à chaque synchro, ce qui crée du bruit et des conflits Git.

**Algorithme.**

- **Clé.** `MÉTHODE:chemin`, le chemin étant dérivé de l'URL de la requête (S:237-246, 741). `operationId` n'est jamais utilisé : renommer un paramètre de chemin donne un retrait suivi d'un ajout.
- **Pas de vraie fusion à 3 voies.** Bruno calcule deux diffs séparés, base → nouvelle spec (S:903-978) et base → collection (S:1085-1251), puis l'utilisateur décide par endpoint entier : garder le sien ou prendre la spec (S:1584-1593). Sans décision, la spec s'applique.
- **Comparaison superficielle.** Elle ne porte que sur les noms et types de paramètres, les noms d'en-têtes, le mode du body et de l'auth, et les chemins de clés du JSON (S:775-859).
- **Écriture (`mergeSpecIntoRequest`, S:672-703).**
  - L'URL suit toujours la spec.
  - Les paramètres et en-têtes **ajoutés par l'équipe sont supprimés** (S:584-597).
  - Les clés du body absentes de la spec sont supprimées (S:538-544).
  - Les scripts, tests, assertions et variables sont conservés.
- **Opérations retirées de la spec.** Les fichiers sont supprimés (`unlinkSync`, S:1480-1521), sans aucune dépréciation.
- **Collision de nom.** Écrasement silencieux (S:1575).
- **Conflits.** Aucune notion de conflit.

**Écriture et reprise.** Les écritures ne sont pas atomiques, les erreurs par fichier sont ignorées et la base avance quand même (S:1513, 1546, 1649). Il n'existe pas d'équivalent de `sync --check` en CLI.

## Comparaison

| | |
| --- | --- |
| Équivalent | Base du dernier import comparée à la nouvelle spec. Nouvelles opérations rangées par tag. Valeurs, scripts et tests de l'équipe préservés. Aperçu avant application. Arbitrage par endpoint (plus grossier que le nôtre). |
| Nous, en plus | Vraie fusion à 3 voies champ par champ, avec des conflits jamais tranchés automatiquement. Clé `operationId` et normalisation des noms de paramètres, rapprochement manuel. Dépréciation au lieu de suppression. Ajouts de l'équipe jamais touchés. Base versionnée dans le dépôt (`.oc-sync/`), sans champ volatil. Écritures atomiques, base réécrite en dernier. `sync --check`. Swagger 2.0. |
| Bruno, en plus (à reprendre en γ) | Masquage des `{{var}}` avant la fusion d'un body JSON, qui sans cela n'est pas du JSON valide. Gardes de sécurité : confinement des chemins, dossiers réservés, http/https seulement, revérification avant suppression. Récupération de la spec via le proxy et les certificats de la collection. Diff texte de la spec brute. Modes « spec seule » et « réinitialiser ». Veille périodique par hash. |

## Recommandations

1. **Garder `.oc-sync/` comme unique lieu de nos métadonnées.** Bruno reconstruit `extensions` à chaque écriture de `opencollection.yml` (`stringifyCollection.ts:234`) et efface toute clé qu'il ne connaît pas.
2. **Préserver `extensions.bruno.openapi` tel quel** quand nous réécrivons `opencollection.yml`, et ajouter au corpus de la Gate 1 une collection qui porte cette extension.
3. **Lire cette extension à l'ouverture** pour préremplir notre source (`sourceUrl`, `groupBy`), en ignorant `lastSyncDate` et `specHash`. La première synchro se fait alors sans base : tout écart `ours ≠ theirs` devient un conflit (la base optionnelle de γ.5 couvre ce cas).
4. **Pour les requêtes créées par Bruno**, qui n'ont pas d'`operationId`, calculer la clé depuis l'URL comme Bruno, puis la rattacher à l'`operationId` de la spec courante.
5. **Documenter** qu'il faut un seul outil de synchro par collection : après une synchro Bruno, notre base est périmée et ses changements seraient lus comme des changements de l'équipe.

## Décisions à prendre (non tranchées)

- **Écrire une entrée minimale `extensions.bruno.openapi`** (`sourceUrl`, `groupBy`, `autoCheck: false`) à l'import ?
  - Pour : Bruno verrait la collection comme connectée.
  - Contre : la synchro de Bruno est destructive pour nos requêtes.
  - Suggestion : non par défaut, proposée en option « compatibilité Bruno ». Jamais de `specHash` ni de `lastSyncDate`.
- **Garder aussi une copie brute de la spec dans `.oc-sync/`**, en plus des instantanés par opération ? Cela permettrait le diff texte et une reconversion si notre convertisseur évolue.
- **Versionner ou ignorer `.oc-sync/`** par défaut. Le versionner est ce qui rend la base partageable, et c'est notre principal avantage sur Bruno.
