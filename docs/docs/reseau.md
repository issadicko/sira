# Réglages réseau (V1, EF-REQ-04) et cookies (EF-UX-02)

Les règles ont été relevées dans le code de Bruno (`bruno-electron/src/ipc/network`, `bruno-cli/src/runner`, `bruno-requests`) : ce qu'elles disent d'une redirection, d'un délai, d'un proxy, d'une autorité de certification ou d'un certificat client est repris tel quel, sauf écart signalé.

## Réglages d'une requête

L'onglet **Réglages** de la requête écrit le bloc `settings` du fichier, clé par clé (modifier le délai ne touche pas `maxRedirects`) :

| Clé | Sens | Défaut à l'exécution |
| --- | --- | --- |
| `timeout` | Délai en millisecondes, 0 : aucun | aucun |
| `followRedirects` | Suivre les redirections | oui |
| `maxRedirects` | Nombre de redirections suivies au plus | 5 |
| `forwardAuthorizationHeader` | Transmettre `Authorization` à une autre origine | oui |

Une clé absente du fichier reste absente tant qu'on ne la change pas, et l'exécution applique son défaut. Une requête créée dans l'interface porte `forwardAuthorizationHeader: false`, comme dans Bruno. Une valeur de `maxRedirects` qui n'est pas un entier positif ou nul (chaîne, négatif) vaut 5.

## Redirections

- Seuls **301, 302, 303, 307 et 308** sont suivis, et seulement avec un en-tête `Location` valide (relatif ou absolu, `http` ou `https`).
- **301, 302 et 303** rejouent la requête en `GET` sans corps, sans `Content-Type` ni `Content-Length` (une requête `HEAD` reste `HEAD`). **307 et 308** gardent la méthode et le corps.
- Quand l'**origine** change (schéma, hôte ou port), les en-têtes `x-amz-*` sont retirés (la signature AWS ne vaut plus pour la nouvelle adresse) ; si `forwardAuthorizationHeader` est faux, `Authorization` et `Proxy-Authorization` le sont aussi. Un `Host` écrit à la main est retiré aussi, pour ne pas désigner l'ancien serveur.
- Avec N redirections permises, **la (N+1)-ième réponse 3xx est rendue telle quelle**, sans erreur : c'est ce que fait Bruno.
- La réponse est celle du dernier saut ; ses durées sont **la somme** de celles de chaque saut. L'onglet Timeline de la réponse liste les redirections suivies (statut et adresse) et l'adresse finale.
- Le délai s'applique à **chaque saut**.

Écarts avec Bruno : le délai est une échéance par saut (axios mesure l'inactivité de la socket) ; les en-têtes `Content-*` sont retirés quelle que soit leur casse (Bruno ne reconnaît que les graphies exactes `Content-Type` et `content-type`) ; l'import d'une spec par URL suit ses redirections à sa façon (refus de HTTPS vers HTTP), il ne passe pas par ces réglages.

## Où vivent les réglages

| Réglage | Fichier de la collection | Hôte : application | Hôte : `xc run` |
| --- | --- | --- | --- |
| Vérification TLS | non | Réglages › Réseau | `--insecure` |
| Autorité de certification | non | Réglages › Réseau | `--cacert`, `--ignore-truststore` |
| Certificats client | `config.clientCertificates` | `clientCertificates` de `network.json`, à écrire à la main (pas d'écran) | `--client-cert-config` |
| Proxy | `config.proxy` | Réglages › Réseau | collection ou environnement ; `--noproxy` |

Les réglages de la requête (délai, redirections) sont dans son fichier, voir plus haut. La vérification TLS n'est réglable ni par collection ni par requête, comme dans Bruno. Le moteur (`crates/engine`) applique ce que `crates/core/src/network.rs` résout pour chaque envoi : le même chemin sert l'envoi seul, le runner, l'introspection GraphQL, les jetons OAuth 2 et les requêtes lancées par un script (`bru.runRequest`, `bru.sendRequest`, `axios`).

## Écran Réseau de l'application

**Réglages › Réseau** règle la vérification TLS, le fichier d'autorités (avec « garder aussi celles du système ») et le proxy (aucun, système ou manuel). Les modifications s'enregistrent avec le bouton **Enregistrer** : un proxy manuel sans hôte, ou avec un port hors de 1 à 65535, est refusé avant d'écrire quoi que ce soit. Les réglages sont dans `network.json`, dans le dossier de données de l'application ; **le mot de passe du proxy n'y est jamais écrit** : il est gardé dans le trousseau du système, et l'interface sait seulement qu'il existe (elle peut l'oublier ou le remplacer, jamais le relire). Un trousseau indisponible envoie la requête sans mot de passe.

## Vérification TLS

Désactivée, elle ne vérifie **ni la chaîne ni le nom d'hôte** du serveur (Bruno n'offre pas de réglage plus fin). Elle vaut pour chaque saut, pour le serveur cible comme pour un proxy `https`, et pour le tunnel `CONNECT`. Les certificats client continuent d'être envoyés. Avec la vérification désactivée, l'autorité personnalisée n'est même pas lue.

## Autorité de certification

Un fichier PEM, d'un ou plusieurs certificats, s'ajoute à ceux du système et aux racines intégrées. Un fichier introuvable fait échouer l'envoi avec son chemin ; un fichier vide est ignoré. **« Ne pas garder les autorités par défaut »** (`--ignore-truststore`) fait ce que dit son nom : seul le fichier fait confiance. Bruno décrit la même option mais n'obtient pas cet effet (Node garde ses racines intégrées) : ici elle fait ce qu'elle dit.

## Certificats client (mTLS)

Un certificat se déclare dans `opencollection.yml`, sous `config.clientCertificates` :

```yaml
config:
  clientCertificates:
    - domain: api.example.com
      type: pem
      certificateFilePath: certs/client.pem
      privateKeyFilePath: certs/client.key
    - domain: "*.example.org"
      type: pkcs12
      pkcs12FilePath: certs/client.p12
      passphrase: "{{certPassphrase}}"
      disabled: true
```

- Les entrées de la collection sont examinées dans l'ordre, puis celles de l'hôte ; **la première entrée active dont le domaine correspond gagne**. Un type inconnu est ignoré à la lecture.
- Le domaine est un **préfixe** de l'adresse de la requête (schéma facultatif parmi `https://`, `grpc://`, `grpcs://`, `ws://`, `wss://`), où `*` vaut « n'importe quelle suite » ; la casse compte. Le port se met dans le domaine (`example.com:8443`). `example.com` correspond donc aussi à `https://example.com.autre.net` : c'est la règle de Bruno. `*.example.com` ne correspond pas à `example.com`. Les caractères spéciaux d'une expression régulière n'ont pas de sens particulier ici (Bruno les interprète).
- Les chemins sont relatifs à la collection ; le domaine, les chemins et la passphrase acceptent des variables `{{…}}`.
- Un fichier illisible fait échouer l'envoi avec la raison.
- **Pas encore pris en charge** : le PKCS#12 (`pkcs12`/`pfx`) et les clés privées chiffrées. L'envoi échoue en disant quoi faire (`openssl pkcs12 … -nodes`, `openssl pkey …`) plutôt que d'ignorer le certificat.

## Proxy

Dans `opencollection.yml`, sous `config.proxy` :

```yaml
config:
  proxy:
    inherit: false
    config:
      protocol: http
      hostname: proxy.example.com
      port: 8080
      auth:
        username: "{{proxyUser}}"
        password: "{{proxyPassword}}"
      bypassProxy: localhost,*.corp.local
```

Précédence, comme Bruno : `disabled: true` dans la collection, aucun proxy ; `inherit: false`, le proxy de la collection ; sinon le réglage de l'hôte (aucun, système ou manuel). Un bloc dont `inherit` n'est pas un booléen, ou sans `config`, vaut « hérite ». `--noproxy` écarte tout proxy, celui de la collection compris.

- **`http` et `https`** : une requête `http` part en forme absolue vers le proxy ; une requête `https` ouvre un tunnel `CONNECT hôte:port`, puis le TLS vers le serveur (avec la vérification, l'autorité et le certificat client ci-dessus). Un proxy `https` est joint en TLS. **`socks4` et `socks5`** : le nom d'hôte est confié au proxy (SOCKS4a, SOCKS5 « h »). Les identifiants servent en `Proxy-Authorization: Basic`, ou à l'authentification SOCKS5 (SOCKS4 : identifiant seul).
- `protocol`, `hostname`, `port` et les identifiants acceptent des variables ; `bypassProxy` non, comme dans Bruno. Un port vide vaut celui du protocole (80, 443, 1080). Sans `hostname`, il n'y a pas de proxy.
- **`bypassProxy`** suit `proxy-from-env`, par saut : `*` contourne tout ; un nom sans point ni `*` en tête doit être exactement l'hôte ; `.x` et `*.x` couvrent les sous-domaines mais pas `x` ; `*x` couvre aussi `x` ; `nom:port` ne vaut que pour ce port ; séparateurs `,` `;` et espaces. Ni CIDR, ni préfixe d'IP, ni `<local>`.
- **Proxy système** : variables `http_proxy`/`HTTP_PROXY` pour une requête `http`, `https_proxy`/`HTTPS_PROXY` pour une requête `https`, puis `all_proxy`/`ALL_PROXY` ; `no_proxy`/`NO_PROXY` donne les hôtes contournés. Le proxy est choisi d'après le schéma de la requête de départ, pas à chaque saut.

Écarts avec Bruno : le proxy système vient **des variables d'environnement seulement** (pas des paramètres de l'OS, pas de PAC) ; la **boucle locale** (`localhost`, `127.0.0.1`, `[::1]`) ne passe jamais par le proxy système (Bruno l'y envoie) ; SOCKS fonctionne aussi pour le proxy système ; le mot de passe d'un proxy de l'application n'est jamais écrit dans un fichier (trousseau du système) ; un `--cacert` introuvable est une erreur (Bruno l'ignore en ligne de commande).

## Ligne de commande

```bash
xc run ma-collection --insecure
xc run ma-collection --cacert autorites.pem [--ignore-truststore]
xc run ma-collection --noproxy
xc run ma-collection --client-cert-config certificats.json
```

`--client-cert-config` lit `{"enabled": true, "certs": [{"domain": "…", "type": "cert", "certFilePath": "…", "keyFilePath": "…"}]}` ; ces entrées passent après celles de la collection, et une valeur `enabled` fausse ou une liste absente n'ajoute rien (avertissement). `--insecure` avec `--cacert` : le fichier est ignoré (avertissement). La ligne de commande ne lit aucune préférence : sans option, la vérification est active et le proxy est celui de la collection ou de l'environnement.

## Cookies

L'application et `xc run` ont **un pot de cookies en mémoire**, comme Bruno : partagé par toutes les collections et les requêtes, vide à chaque lancement, jamais écrit sur le disque. Chaque saut d'un envoi (redirections comprises) :

- **envoie** les cookies du pot qui correspondent à son adresse (domaine, chemin, `Secure`, `HttpOnly`, expiration) ; le chemin le plus long passe d'abord, puis le cookie le plus ancien ;
- **range** ceux de ses en-têtes `Set-Cookie`. Un cookie qui vise un autre domaine que celui de la réponse, ou un suffixe public (`Domain=com`, `Domain=co.uk`), est ignoré ; `Max-Age=0` ou une date passée supprime le cookie.

Un en-tête `Cookie` écrit à la main dans la requête est complété par ceux du pot, qui l'emportent pour un même nom. Il ne suit pas une redirection vers une autre origine (écart avec Bruno, qui le laisse partir). Un cookie `Secure` part aussi vers `localhost` et les adresses locales en `http`. Les requêtes lancées par un script avec `bru.sendRequest` ou `axios` ne passent pas par le pot ; `bru.runRequest` et le runner, si. Les scripts lisent et modifient le pot avec `bru.cookies` (voir [Scripts](scripts.md)).

- **Réglages › Réseau** : « Envoyer les cookies » et « Garder les cookies reçus » se coupent séparément (les deux sont actifs par défaut).
- **Réglages › Cookies** : le pot par domaine ; ajouter un cookie (nom, valeur, domaine, chemin, expiration UTC, `Secure`, `HttpOnly`, « cet hôte seulement »), modifier, supprimer, supprimer un domaine, tout effacer.
- **`xc run --disable-cookies`** : ni envoi ni enregistrement, le pot reste vide. Sinon le pot d'une exécution passe d'une requête et d'une itération à la suivante.

