# Réglages réseau (V1, EF-REQ-04)

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
