# WebSocket (EF-WS-01)

Une requête `type: websocket` ouvre une connexion, envoie des messages, reçoit ce que le serveur pousse, et se ferme. Le format est celui de Bruno (bloc `websocket`) ; tout est lu et écrit sans toucher à ce qu'on n'édite pas.

## Le fichier

```yaml
info:
  name: Salon
  type: websocket
  seq: 1

websocket:
  url: "{{wsBase}}/chat"
  headers:
    - name: X-Demo
      value: "{{user}}"
  auth:
    type: bearer
    token: "{{token}}"
  message:
    - title: Saluer
      selected: true
      message:
        type: json
        data: '{"user":"{{user}}"}'
    - title: Ping
      selected: false
      message:
        type: text
        data: ping

settings:
  timeout: 5000
  keepAliveInterval: 30000
```

- **Messages** : un seul message (`message: { type, data }`) ou une liste d'entrées `{ title, selected, message: { type, data } }`. Celles dont `selected` est vrai partent à « Envoyer » ; les autres se tirent une à une. `type` est `text`, `json` ou `xml` : le contenu part tel quel, comme texte, le type ne sert qu'à l'éditeur. Un message sans titre, seul et coché, garde la forme courte ; ajouter un deuxième message passe le fichier en liste, retirer le dernier le ramène à la forme courte. Une édition ne réécrit que le champ modifié (un message intact garde sa forme, les clés inconnues survivent).
- **Adresse** : `ws://` ou `wss://`. `http://` et `https://` valent la même chose (comme dans un navigateur) ; sans schéma, `ws://`. Les `{{variables}}` sont résolues dans l'adresse, les en-têtes, l'authentification et chaque message, comme pour une requête HTTP (en-têtes de la collection et des dossiers compris).
- **Réglages** : `timeout` est le délai pour joindre le serveur et finir la mise à niveau (30 s sans valeur) ; `keepAliveInterval` est l'intervalle, en millisecondes, entre deux trames ping (0 ou absent : aucune).
- **Authentification** : Bearer, Basic et clé d'API (en-tête ou paramètre d'adresse) sont posés sur la mise à niveau. Digest, AWS Signature V4 et OAuth 2.0 demandent un échange avec le serveur : ils sont refusés avec un message plutôt qu'ignorés.

## Réseau

La connexion est celle des requêtes HTTP : résolution DNS, **proxy** (la mise à niveau passe toujours par un tunnel `CONNECT` ou SOCKS, même en `ws://`), vérification TLS, autorité personnalisée, **certificat client** (PEM, PKCS#12) pour `wss://`, et **pot de cookies** (les cookies du pot partent avec la mise à niveau, ceux que le serveur pose y sont gardés). Voir `reseau.md`.

Une mise à niveau refusée (HTTP 401, 403…) dit le statut et le début du corps. Une trame de fermeture reçue donne son code et sa raison ; une coupure sans fermeture est une erreur. Un message plus gros que la limite (64 Mio) ferme la connexion avec une erreur.

## `xc ws`

```bash
xc ws <collection> <requête> [--env nom] [--env-var nom=valeur] [--send texte]... [--no-send]
      [--idle secondes] [--max-time secondes] [--insecure] [--cacert f] [--noproxy] [--disable-cookies]
```

Ouvre la requête, envoie ses messages cochés puis ceux de `--send`, affiche ce qui arrive (`→` envoyé, `←` reçu : texte, binaire en hexadécimal, ping, pong, fermeture), et ferme (code 1000) après `--idle` secondes (2 par défaut) sans rien recevoir. `--max-time` borne la durée totale. Code de sortie : 0, 1 si la connexion échoue ou casse, 2 si la ligne de commande ou la collection est invalide. Les secrets du trousseau sont lus comme pour `xc run`.

## Dans l'application

Ouvrir une requête WebSocket de l'arbre affiche l'adresse, la liste des messages (cochés = envoyés par « Envoyer ») et le journal de la session : connexion, messages envoyés (→) et reçus (←), pings, fermeture avec code, erreurs. « Connecter » ouvre la session, « Déconnecter » envoie une fermeture normale ; un message libre peut être composé et envoyé sans toucher au fichier. Les variables et secrets sont résolus comme pour `xc ws`.

## Ce qui n'y est pas

- Ni scripts, ni assertions, ni tests sur une connexion WebSocket : Bruno n'en exécute pas non plus. `xc run` rapporte une requête WebSocket en erreur (« protocole non pris en charge »), comme `bru run`.
- Les messages binaires se reçoivent (hexadécimal) mais ne s'écrivent pas : un message du fichier est du texte.
