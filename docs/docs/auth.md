# Authentification (EF-AUT-01 à EF-AUT-04)

Une requête, un dossier ou la collection portent une auth (`type: …` dans `http.auth`, ou dans le fichier du dossier et `opencollection.yml`). `inherit` prend celle du dossier le plus proche, puis celle de la collection. `prepare` pose ce qui ne dépend que du fichier (Bearer, Basic, API Key) ; ce qui dépend de la requête finale ou de la réponse du serveur est fait à l'envoi par `xc-runner` (`crates/runner/src/auth.rs`).

| Type | Quand | Détail |
| --- | --- | --- |
| Bearer, Basic, API Key | `prepare` | En-tête ou paramètre d'adresse, variables résolues. |
| Digest | à l'envoi | RFC 7616 (MD5, MD5-sess, SHA-256, SHA-256-sess ; `qop=auth`). La requête part sans identifiants ; sur un 401 qui porte un défi `Digest`, elle est renvoyée **une fois** avec la réponse. Un second 401 est rendu tel quel. |
| AWS Signature V4 | à l'envoi | Signature compatible `aws4` (celle de Bruno) : en-têtes `Authorization` et `X-Amz-Date`, plus `X-Amz-Security-Token` avec un jeton de session et `X-Amz-Content-Sha256` pour S3. Service et région déduits de l'hôte quand ils sont vides ; un profil de `~/.aws/credentials` ou `~/.aws/config` remplace les clés saisies. |
| NTLM | à l'envoi | NTLMv2 sur une seule connexion. Voir ci-dessous. |
| OAuth 1.0 | à l'envoi | Signature RFC 5849 de la requête finale. Voir ci-dessous. |
| OAuth 2.0 | à l'envoi | Voir ci-dessous. |
| Autres (`wsse`, …) | jamais | Conservés tels quels dans le fichier, non appliqués. |

Les algorithmes Digest et la signature AWS sont vérifiés contre les exemples des RFC 2617 et 7616 et contre les vecteurs de la suite de test officielle d'AWS (`get-vanilla`, `post-vanilla`, tri de la requête).

## NTLM (EF-AUT-03)

```yaml
auth:
  type: ntlm
  username: "{{user}}"
  password: "{{process.env.NTLM_PASSWORD}}"
  domain: CORP          # facultatif
```

Les trois champs sont ceux de la spec OpenCollection. NTLM lie l'échange à **une connexion** : la requête part d'abord **sans corps** avec le message de négociation (`Authorization: NTLM …`), le serveur répond 401 avec son défi, puis la requête **entière** (corps compris) part sur la même connexion avec la réponse au défi.

- **NTLMv2** (MS-NLMP) : `NTOWFv2` (MD4 du mot de passe en UTF-16LE, HMAC-MD5 sur l'utilisateur en majuscules et le domaine), horodatage et paires AV du serveur repris, réponse LM vide quand le serveur donne un horodatage. Le nom du poste annoncé est celui de la machine. Vérifié contre les vecteurs de test de MS-NLMP (hachage, `NTOWFv2`, réponses LMv2 et NTLMv2) et contre un serveur de test qui contrôle la preuve.
- **Pas de signature ni de chiffrement de session** : HTTP n'en demande pas, la réponse d'authentification suffit ; les drapeaux de signature, de chiffrement et d'échange de clé du serveur ne sont pas repris.
- Un serveur qui ne demande pas NTLM (autre statut que 401, ou aucun défi NTLM) a sa réponse rendue telle quelle ; un mot de passe refusé rend le 401 du serveur ; un défi illisible est une erreur qui le dit.
- Les redirections suivent la règle habituelle, avec un nouvel échange à chaque saut. Pas d'authentification NTLM d'un proxy (le proxy a ses propres identifiants Basic) ni de Negotiate/Kerberos.
- Non pris en charge sur WebSocket et gRPC : refusée avec un message plutôt qu'ignorée.

## OAuth 1.0 (EF-AUT-03)

```yaml
auth:
  type: oauth1
  consumerKey: "{{consumer_key}}"
  consumerSecret: "{{process.env.CONSUMER_SECRET}}"
  accessToken: "{{access_token}}"
  accessTokenSecret: "{{process.env.TOKEN_SECRET}}"
  signatureMethod: HMAC-SHA256     # HMAC-SHA1 (par défaut), HMAC-SHA256, HMAC-SHA512, RSA-SHA1, RSA-SHA256, RSA-SHA512, PLAINTEXT
  placement: header                # header (par défaut), query ou body
  realm: Photos
  includeBodyHash: true
  privateKey:                      # méthodes RSA-* seulement
    type: file                     # file (chemin absolu, ou relatif à la collection) ou text (le PEM)
    value: /home/ada/.keys/oauth.pem
```

Les champs sont ceux de la spec OpenCollection (`callbackUrl`, `verifier`, `timestamp`, `nonce` et `version` aussi) ; les champs vides sont absents du fichier. Sira **signe** avec les jetons que la requête donne : l'obtention des jetons (étapes de la RFC 5849 §2.1 et §2.2) se fait hors de l'application.

- **Signature** sur la requête finale (calculée à l'envoi, après les scripts et l'interpolation) : méthode, URL de base normalisée (schéma et hôte en minuscules, port par défaut omis), paramètres de l'adresse et d'un corps `application/x-www-form-urlencoded`, paramètres `oauth_*`, encodage en pourcentage de la RFC 5849 §3.6, tri par nom puis valeur. Un corps JSON ou texte n'entre pas dans la signature.
- **Méthodes** : HMAC-SHA1, 256 et 512 (clé `secret consommateur&secret du jeton`), RSA-SHA1, 256 et 512 (PKCS#1 v1.5, clé PEM PKCS#8 ou PKCS#1 sans mot de passe) et PLAINTEXT. Vérifiées contre l'exemple d'OAuth Core 1.0 (`tR3+Ty81lMeYAr/Fid0kMTYa/WM=`) et, pour RSA, par la vérification de la signature avec la clé publique.
- **Emplacement** : en-tête `Authorization: OAuth realm="…", oauth_…` (remplace un `Authorization` existant), paramètres ajoutés à l'adresse (`query`) ou à un corps de formulaire (`body`, refusé avec un autre type de corps).
- **Horodatage et nonce** : automatiques (secondes Unix, 16 octets aléatoires) quand le fichier ne les fixe pas. `includeBodyHash` ajoute `oauth_body_hash` (SHA-1, 256 ou 512 selon la méthode) pour un corps qui n'est pas un formulaire.
- Une clé privée introuvable, illisible ou absente avec une méthode RSA, une méthode de signature ou un emplacement inconnus font échouer la requête en le disant. La clé privée est mieux gardée hors de la collection (elle est versionnée) : un chemin absolu le permet.
- Non pris en charge sur WebSocket et gRPC : refusée avec un message.

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
- NTLM et OAuth 1.0 : voir leurs sections ; ils ne s'exportent ni vers Postman ni vers OpenAPI (signalés à l'export) et la génération de code ne les transcrit que par une note.
