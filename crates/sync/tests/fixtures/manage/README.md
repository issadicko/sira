Fichiers que Bruno écrit quand on crée une collection, une requête ou un dossier (`docs/docs/gestion-collection.md` § 2 et § 3), produits par le vrai sérialiseur de Bruno via l'oracle (`tools/oracle`). Les tests ne lancent jamais Node.

- `collection*.root.json`, `collection*.config.json` : `{meta:{name}}` et `brunoConfig` de `renderer:create-collection` ; `collection*.yml` : `opencollection.yml` attendu (`node oracle.js stringify-collection <cas>.root.json <cas>.config.json`).
- `request-*.json` : item de `newHttpRequest` (requête HTTP vierge) ; `request-*.yml` attendu (`node oracle.js stringify-item`).
- `folder*.json` : racine de dossier de `renderer:new-folder` (`folder`) ou minimale, sans authentification, de `renderer:rename-item` quand `folder.yml` manque (`folder-minimal*`) ; `folder*.yml` attendu (`node oracle.js stringify-folder`).
- `gitignore` : `DEFAULT_GITIGNORE` de `packages/bruno-electron/src/utils/filesystem.js`, sans saut de ligne final, écrit sous le nom `.gitignore`.
