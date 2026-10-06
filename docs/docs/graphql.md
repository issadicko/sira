# GraphQL (V1, EF-GQL-01)

Une requête GraphQL est un fichier OpenCollection comme les autres, de `info.type: graphql`, dont le corps vit dans la table `graphql` (jamais dans une table `http`) :

```yaml
info:
  name: Fiche produit
  type: graphql
  seq: 1

graphql:
  method: POST
  url: "{{baseUrl}}/graphql"
  body:
    query: |-
      query Produit($sku: String!) {
        product(sku: $sku) { name price }
      }
    variables: |-
      {
        "sku": "{{sku}}"
      }
  auth: inherit
```

C'est le format de Bruno : un fichier écrit par Bruno s'ouvre tel quel, et inversement. Les clés inconnues du corps sont conservées, et seule la partie modifiée est réécrite (modifier la requête ne touche pas la ligne des variables). Quand la requête et les variables sont vides, la clé `body` est retirée.

## Envoi

Le corps envoyé est `{"query": …, "variables": {…}}` en `application/json` (un `Content-Type` déjà posé est respecté), comme `prepare-request.js` de Bruno :

- les `{{variables}}` sont résolues **dans la requête et dans les variables**, puis les variables sont lues comme du JSON ;
- les commentaires `//` et `/* */` sont admis dans les variables (ils sont retirés avant l'analyse, jamais à l'intérieur d'une chaîne) ;
- des variables vides donnent `{}` ; des variables qui ne sont pas du JSON arrêtent l'envoi avec la raison, sans rien envoyer.

Tout le reste est celui d'une requête HTTP : en-têtes de la collection et des dossiers, auth (dont OAuth 2, Digest et AWS), scripts pré-requête et post-réponse, assertions, tests, variables de session. Pour un script, `req.getBody()` rend `{ query, variables }` (les variables telles que saisies). Le runner et `xc run` lancent les requêtes GraphQL avec les autres ; le code généré (`xc code`, `</>`) porte le corps JSON tel qu'il partirait.

## Schéma, introspection et autocomplétion

Le bouton « Charger le schéma » de l'onglet Corps envoie la requête d'introspection standard (celle de graphql-js, descriptions et directives comprises) à l'adresse de la requête, avec ses en-têtes, son auth et ses variables, **sans lancer de script** (comme Bruno). Le résultat est gardé dans `.oc-sync/graphql/<empreinte de l'adresse>.json`, hors des fichiers de la collection : l'autocomplétion marche donc hors ligne après un premier chargement, et toutes les requêtes qui visent la même adresse partagent le même schéma. Un changement d'environnement qui change l'adresse change de schéma.

Avec un schéma, l'éditeur de la requête :

- complète les champs, les arguments, les variables, les directives, les types et les valeurs d'énumération (Ctrl+Espace ouvre la liste, les descriptions du schéma s'affichent à côté) ;
- souligne les erreurs de validation (champ inconnu, argument manquant, type incompatible) ;
- colore la syntaxe, avec ou sans schéma.

Un serveur qui refuse l'introspection (désactivée en production, ou 401) est dit tel quel : le premier message d'erreur GraphQL, ou la réponse qui n'est pas du JSON. L'éditeur reste utilisable sans schéma.

« Formater » remet la requête en forme (graphql-js) et les variables en JSON indenté. Il **refuse de toucher** à ce qui perdrait du contenu : une requête qui porte des commentaires `#`, des variables qui portent des commentaires `//`.

## Créer une requête GraphQL

« Nouvelle requête GraphQL » (menu de l'arbre, palette de commandes) crée une requête vierge (`POST`, corps vide), comme `newGraphQLRequest` de Bruno ; elle s'ouvre aussitôt. Les imports Postman, Insomnia, `.bru` et cURL créent déjà des requêtes GraphQL quand la source en contient.

## Ce qui reste

- Pas d'abonnements (WebSocket) ni de téléversement de fichiers (spec multipart GraphQL).
- Pas d'explorateur de documentation du schéma (le survol et les suggestions donnent les descriptions).
- Le formatage ne conserve pas les commentaires : il refuse plutôt que de les perdre.
