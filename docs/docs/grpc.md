# gRPC (EF-GRPC-01)

Une requête `type: grpc` appelle une méthode d'un service gRPC : unaire, flux serveur, flux client ou bidirectionnel. Le format est celui de Bruno (bloc `grpc`) ; tout est lu et écrit sans toucher à ce qu'on n'édite pas.

## Le fichier

```yaml
info:
  name: Écho
  type: grpc
  seq: 1

grpc:
  url: "{{grpcBaseUrl}}"
  method: demo.Demo/Echo
  methodType: unary
  protoFilePath: proto/demo.proto
  metadata:
    - name: x-user
      value: "{{user}}"
  message: |-
    {
      "name": "{{user}}"
    }

runtime:
  auth:
    type: bearer
    token: "{{token}}"
```

- **Adresse** : `grpc://` ou `http://` (en clair), `grpcs://` ou `https://` (TLS). Sans schéma, en clair, comme Bruno (`localhost:50051`).
- **Méthode** : `paquet.Service/Méthode`, avec ou sans `/` initial. Elle garde sa casse (elle n'est jamais passée en majuscules comme une méthode HTTP). `methodType` vaut `unary`, `server-streaming`, `client-streaming` ou `bidi-streaming`.
- **Messages** : un texte JSON seul (`message: |-`), ou une liste `{ description, message }` envoyée dans l'ordre. Un message seul sans description garde la forme courte ; ajouter un deuxième passe le champ en liste. Les `{{variables}}` sont résolues dans l'adresse, les métadonnées et chaque message.
- **Métadonnées** : `metadata`, plus celles des dossiers (`request.metadata` de `folder.yml`), les plus proches l'emportant. Elles partent comme en-têtes HTTP/2 en minuscules ; `content-type`, `te` et `host` appartiennent au protocole et ne sont pas remplacés.
- **Authentification** : Bearer, Basic et clé d'API sont posés comme métadonnées (lue dans `grpc.auth` ou, comme dans les fichiers de Bruno, dans `runtime.auth`). Digest, AWS Signature V4 et OAuth 2.0 sont refusés avec un message plutôt qu'ignorés.
- **Réglages** : `settings.timeout` est le délai pour joindre le serveur (30 s sans valeur).

## Le schéma protobuf

Un message JSON ne devient des octets protobuf qu'avec un schéma. Sira le cherche dans cet ordre :

1. le `.proto` de la requête (`protoFilePath`, relatif à la collection) ; une méthode absente de ce fichier est refusée en le nommant ;
2. les `.proto` déclarés par la collection (`config.protobuf.protoFiles`) ;
3. la **réflexion du serveur** (`grpc.reflection.v1`, puis `v1alpha` pour les anciens serveurs), quand aucun fichier ne décrit la méthode.

Les imports sont cherchés dans `config.protobuf.importPaths` (relatifs à la collection, ceux marqués `disabled` sont ignorés), puis dans le dossier du fichier. Un `.proto` illisible est dit avec la ligne en cause. Le JSON suit la correspondance JSON canonique de protobuf : noms de champ en `camelCase` (ou d'origine), entiers 64 bits en chaînes, énumérations par leur nom. Un message qui ne correspond pas au schéma est refusé avant l'envoi, en nommant le type attendu.

## Réseau

La connexion est celle des requêtes HTTP : résolution DNS, **proxy** (toujours par tunnel `CONNECT` ou SOCKS), vérification TLS avec négociation ALPN `h2`, autorité personnalisée, certificat client. Voir `reseau.md`. Les messages reçus sont limités à 64 Mio. La compression gRPC n'est pas négociée : le client annonce `identity`, un message compressé malgré cela est une erreur.

## `xc grpc`

```bash
xc grpc <collection> <requête> [--env nom] [--env-var nom=valeur] [--send json]...
        [--max-time secondes] [--insecure] [--cacert f] [--noproxy]
```

Ouvre la requête, envoie ses messages (ou ceux de `--send`, qui les remplacent), demi-ferme le flux d'envoi, affiche chaque message reçu en JSON (`→` envoyé, `←` reçu) puis le statut. Code de sortie : 0 pour un statut `OK`, 1 pour un autre statut, une erreur de transport ou une connexion impossible, 2 si la ligne de commande, la collection ou un message est invalide (par exemple deux messages sur un appel unaire). `--max-time` annule l'appel (statut `CANCELLED`).

## Ce qui n'y est pas

- Ni scripts, ni assertions, ni tests sur un appel gRPC : Bruno n'en exécute pas non plus. `xc run` rapporte une requête gRPC en erreur (« protocole non pris en charge »), comme `bru run`.
- Pas de compression des messages, pas de métadonnées binaires (`-bin`) décodées, pas d'annulation en cours de route depuis la ligne de commande autre que `--max-time`.
