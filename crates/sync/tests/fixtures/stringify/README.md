Chaque cas est un JSON de Bruno (`items/`, `folders/`, `environments/`, `collections/<cas>.root.json` + `<cas>.config.json`) et son `.yml` attendu, écrit par le vrai sérialiseur de Bruno via l'oracle.
Les cas `openapi-*` et `curl-*` sont des sorties réelles des convertisseurs de Bruno, les cas `hand-*` sont faits main.
Régénérer un attendu : `node oracle.js stringify-item items/<cas>.json > items/<cas>.yml` (idem `stringify-folder`, `stringify-environment`),
et pour une collection : `node oracle.js stringify-collection collections/<cas>.root.json collections/<cas>.config.json > collections/<cas>.yml`.
