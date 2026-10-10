# Git intégré (EF-GIT-01)

Une collection est un dossier de fichiers texte ; Sira lit et écrit ses fichiers, et Git les versionne. La vue **Source Control** (barre d'activité, `Ctrl+Maj+G`) montre ce qui a changé, compare chaque fichier à sa version du dernier commit, valide, récupère et pousse. C'est gratuit, contrairement à Bruno.

## Ce qui est fait par quoi

- **Lecture du dépôt** (dépôt trouvé, branche, contenu d'un fichier au dernier commit) : `gix`, en Rust pur.
- **Tout ce qui écrit dans l'index, l'historique ou le réseau** (état de l'arbre, `add`, `commit`, `pull`, `push`) : l'exécutable `git` du système. `gix` ne sait pas pousser, et passer par `git` reprend d'office ce que l'utilisateur a déjà configuré : assistant d'identifiants, clés SSH, `user.name`, `user.email`, signature des commits. Sans exécutable `git`, la vue le dit (« l'exécutable git est introuvable ») ; rien d'autre n'en dépend.

Aucune question n'est posée sur un terminal (`GIT_TERMINAL_PROMPT=0`) et chaque commande a un délai (30 s en local, 120 s pour le réseau) : une authentification manquante échoue avec un message au lieu de bloquer l'application.

## La vue

- **Barre latérale** : la branche et son retard ou son avance sur la branche amont (« ↑2 à pousser », « ↓1 à récupérer », « à jour avec origin/main »), le message de validation, les boutons *Valider* et *et pousser*, la liste des fichiers modifiés avec leur état (`M` modifié, `A` ajouté ou nouveau, `D` supprimé, `R` renommé, `U` en conflit). Les boutons de l'en-tête récupèrent (`pull`), poussent et relisent l'état.
- **Éditeur** : la comparaison en deux colonnes du fichier choisi, « Dernier commit » à gauche et « Copie de travail » à droite, les lignes identiques loin des changements repliées. Un fichier binaire ou de plus de 2 Mo n'est pas comparé ligne à ligne et le dit. Une requête s'ouvre d'un clic depuis sa comparaison.
- **Pastille** sur l'icône de la barre d'activité (nombre de fichiers modifiés) et branche dans la barre d'état. L'état se relit à chaque changement de fichier (enregistrement, éditeur externe, `git pull` fait ailleurs), au retour dans la fenêtre et après chaque action.
- Un dossier qui n'est dans aucun dépôt propose **Initialiser un dépôt** (`git init`).

## Règles

- **Portée** : seuls les fichiers du dossier de la collection comptent, même si le dépôt est plus grand (la collection peut être un sous-dossier). Les chemins sont relatifs à la collection.
- **Valider** ajoute (`add -A`) puis valide tous les changements de la collection, nouveaux fichiers et suppressions compris, avec le message écrit (`Ctrl+Entrée` dans le champ). Un message vide, aucune modification ou un fichier en conflit bloquent le bouton et disent pourquoi. Le message n'est effacé qu'une fois la validation faite.
- **Valider et pousser** : si la validation réussit mais pas le push (pas de réseau, authentification refusée), la validation reste faite, le message d'erreur est affiché et rien n'est perdu.
- **Récupérer** n'avance que si c'est un simple avancement (`pull --ff-only`) : une branche locale et distante qui ont divergé ne sont **jamais fusionnées en silence**, l'erreur le dit et laisse la résolution à Git. Des modifications locales qui gêneraient la mise à jour sont refusées par Git et signalées ; rien n'est écrasé. Les fichiers récupérés rechargent la collection à chaud.
- **Pousser** crée la branche amont sur `origin` au premier push (`--set-upstream`). Un push refusé parce que le serveur a de l'avance renvoie vers « récupérer ».
- Les erreurs courantes de Git sont expliquées en français (identité manquante, authentification, branches divergentes, pas de dépôt distant), avec le message d'origine en dessous.

## Ce qui n'y est pas

- Pas de choix de fichiers à valider (tout ce qui a changé part), pas d'indexation partielle, pas de changement de branche ni de création de branche, pas d'historique (`log`), pas d'annulation d'un fichier (qui écraserait le travail sans retour), pas de résolution de conflits dans l'application (les fichiers en conflit sont signalés `U`).
- Pas d'authentification gérée par Sira : celle de Git s'applique.
