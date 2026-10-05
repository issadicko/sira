# Bibliothèques des scripts

Construit `crates/script/js/libs/*.js` (chai avec les assertions de Bruno, Ajv, crypto-js, moment, tv4, uuid, nanoid, Buffer) en un fichier par bibliothèque, chargé à la demande par le sandbox.

```bash
cd tools/scripts-bundle && npm install && npm run build
```

Les sorties sont commitées : la compilation de Sira n'a besoin ni de Node ni du réseau.
