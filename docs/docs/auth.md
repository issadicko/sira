# Authentification (V1, EF-AUT-01, EF-AUT-02, EF-AUT-04)

Une requête, un dossier ou la collection portent une auth (`type: …` dans `http.auth`, ou dans le fichier du dossier et `opencollection.yml`). `inherit` prend celle du dossier le plus proche, puis celle de la collection. `prepare` pose ce qui ne dépend que du fichier (Bearer, Basic, API Key) ; ce qui dépend de la requête finale ou de la réponse du serveur est fait à l'envoi par `xc-runner` (`crates/runner/src/auth.rs`).

| Type | Quand | Détail |
| --- | --- | --- |
| Bearer, Basic, API Key | `prepare` | En-tête ou paramètre d'adresse, variables résolues. |
| Digest | à l'envoi | RFC 7616 (MD5, MD5-sess, SHA-256, SHA-256-sess ; `qop=auth`). La requête part sans identifiants ; sur un 401 qui porte un défi `Digest`, elle est renvoyée **une fois** avec la réponse. Un second 401 est rendu tel quel. |
| AWS Signature V4 | à l'envoi | Signature compatible `aws4` (celle de Bruno) : en-têtes `Authorization` et `X-Amz-Date`, plus `X-Amz-Security-Token` avec un jeton de session et `X-Amz-Content-Sha256` pour S3. Service et région déduits de l'hôte quand ils sont vides ; un profil de `~/.aws/credentials` ou `~/.aws/config` remplace les clés saisies. |
| OAuth 2.0 | à l'envoi | Voir ci-dessous. |
| Autres (`ntlm`, `wsse`, …) | jamais | Conservés tels quels dans le fichier, non appliqués. |

Les algorithmes Digest et la signature AWS sont vérifiés contre les exemples des RFC 2617 et 7616 et contre les vecteurs de la suite de test officielle d'AWS (`get-vanilla`, `post-vanilla`, tri de la requête).

## OAuth 2.0

Le modèle reprend exactement ce que Bruno écrit dans OpenCollection YAML (`flow`, `credentials`, `resourceOwner`, `pkce`, `additionalParameters`, `tokenConfig`, `settings`) : un fichier lu puis réécrit sans modification reste identique octet pour octet, et l'éditeur ne touche que les clés qu'il change.

```yaml
auth:
  type: oauth2
  flow: authorization_code          # client_credentials | resource_owner_password_credentials | authorization_code | implicit
  authorizationUrl: "{{auth_url}}/authorize"
  accessTokenUrl: "{{auth_url}}/token"
  callbackUrl: http://localhost/callback
  credentials:
    clientId: "{{client_id}}"
    clientSecret: "{{process.env.CLIENT_SECRET}}"
    placement: basic_auth_header     # ou body
  scope: openid read
  pkce: {}                           # {disabled: true} pour le couper
  additionalParameters:
    accessTokenRequest:
      - {name: audience, value: api, placement: body}   # header | query | body
  tokenConfig:
    id: credentials                  # nom du jeton : {{$oauth2.credentials.access_token}}
    placement:
      header: Bearer                 # ou query: access_token
    source: access_token             # ou id_token
  settings:
    autoFetchToken: true
    autoRefreshToken: true
```

### Obtention et conservation

- **client_credentials** et **password** : une requête `POST` au point d'accès aux jetons, corps `application/x-www-form-urlencoded`. Les identifiants vont dans un en-tête `Authorization: Basic` ou dans le corps selon `placement`.
- **authorization_code** : l'adresse d'autorisation est ouverte avec `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state` (aléatoire s'il est vide) et, avec PKCE, `code_challenge` (S256). Le `state` renvoyé doit être celui envoyé. Le code est échangé contre le jeton avec le `code_verifier`. Le serveur qui refuse (`error`, `error_description`) fait échouer la requête avec ce motif.
- **implicit** : `response_type=token` ; le jeton est lu dans le fragment de l'adresse de retour, sans appel au point d'accès aux jetons.
- **refresh** : à l'expiration, avec `autoRefreshToken` et un jeton de rafraîchissement, `grant_type=refresh_token` est envoyé à l'URL de rafraîchissement (à défaut, celle du jeton) ; un refus retombe sur un jeton neuf. Le jeton de rafraîchissement précédent est gardé quand le serveur n'en renvoie pas.
- Un jeton est tenu pour expiré 10 s avant son expiration. Sans `expires_in`, il n'expire pas.
- Avec `autoFetchToken: false`, aucun jeton n'est demandé à l'envoi : la requête part sans ; le jeton s'obtient depuis l'éditeur.
- Les jetons vivent dans la `Session` de la collection (clé : URL du jeton, client, nom du jeton) : ils survivent d'une requête à l'autre d'un run et d'un envoi à l'autre dans l'application, jamais sur le disque.

### Variables

Chaque champ de la réponse du serveur est lisible par `{{$oauth2.<nom>.<champ>}}` (dans les requêtes et les scripts) et par `bru.getOauth2CredentialVar('$oauth2.<nom>.<champ>')`, comme chez Bruno. La première requête d'une série part avant que le jeton existe : une variable `{{$oauth2.…}}` y reste non résolue (le jeton est pourtant lisible par les scripts de test de cette même requête).

### Application et CLI

- Les flux interactifs (code, implicite) ouvrent **une fenêtre de connexion** dans l'application (`app/src-tauri/src/oauth.rs`) : la navigation vers l'URL de rappel est interceptée, rien n'a besoin d'écouter à cette adresse (`localhost` suffit). Fermer la fenêtre annule l'autorisation.
- L'onglet Auth montre l'état du jeton (valide, expire dans…, expiré), sans jamais en montrer la valeur, avec « Obtenir un token » / « Se connecter » et « Oublier ». La valeur du jeton ne quitte pas le processus Rust.
- `xc run` et les requêtes imbriquées (`bru.runRequest`) gèrent `client_credentials` et `password`. Un flux interactif sans fenêtre échoue en disant ce qui manque ; un serveur de rappel local pour la CLI n'est pas fait.

## Limites connues

- Le mot de passe, le secret client et les clés AWS se référencent par des variables : mettre le secret dans une variable d'environnement marquée secrète (`{{client_secret}}`) garde sa valeur dans le trousseau du système plutôt que dans le fichier versionné (voir `gestion-collection.md`).
- Une requête nécessitant une re-authentification sur un 401 n'est pas rejouée avec un jeton neuf (Bruno ne le fait pas non plus).
- NTLM et OAuth 1.0 sont prévus en V2 (EF-AUT-03).
