# HashiCorp Vault (EF-VAR-03)

Un secret qui vit dans Vault s'utilise comme une variable, sans jamais être écrit dans un fichier de la collection :

```
{{$vault.secret/app/db#password}}
```

Le nom se lit `montage/chemin#champ` : ici le champ `password` du secret `app/db` du montage `secret`. La variable s'écrit partout où une variable peut aller (adresse, en-têtes, paramètres, corps, authentification, métadonnées gRPC, messages WebSocket) et dans la valeur d'une variable d'environnement ou de collection, qui peut donc elle-même renvoyer à Vault. Le fichier ne contient que la référence, jamais la valeur.

## Quand et comment c'est lu

L'interpolation ne sait pas aller chercher une valeur sur le réseau. Elle rend d'abord ces noms « sans valeur » ; Sira les lit alors dans Vault, les donne comme variables et prépare de nouveau la requête (jusqu'à 4 tours, pour les références imbriquées). Conséquences :

- **Une lecture refusée fait échouer la requête**, avec le nom de la variable et la raison de Vault (`permission denied`, secret ou champ introuvable avec la liste des champs, Vault injoignable). Le nom n'est jamais envoyé tel quel au serveur.
- **Un secret est lu une fois pour tous ses champs**, et gardé 60 secondes : plusieurs requêtes d'un run n'interrogent Vault qu'une fois, un secret changé est relu au bout d'une minute. Le jeton obtenu par un AppRole est gardé pour la session.
- **Moteurs KV versions 1 et 2** : la version 2 est essayée d'abord (`montage/data/chemin`), la version 1 quand ce chemin n'existe pas. Un champ qui n'est pas du texte est rendu en JSON.
- Les requêtes qui ne passent pas par le runner (génération de code) laissent la référence telle quelle : aucune valeur de Vault ne sort dans un extrait.

La connexion à Vault est celle des requêtes HTTP : proxy (celui des réglages, ou de la collection), autorité de confiance, vérification TLS. Les redirections ne sont pas suivies.

## Réglages

**Dans l'application** (Réglages, section *Vault*) : l'adresse (`http://` ou `https://`), l'espace de noms (Vault Enterprise), la méthode de connexion et un bouton **Tester la connexion** (`auth/token/lookup-self` : nom du jeton, politiques, durée de vie restante).

- **Jeton** : gardé dans le trousseau du système, jamais dans un fichier ni dans l'interface (elle sait seulement qu'il existe).
- **AppRole** : identifiant de rôle, montage (`approle` par défaut) et `secret_id` (trousseau). Le jeton est obtenu à la première lecture.

**Environnement** : ce que les réglages ne donnent pas vient de l'environnement du processus, comme avec le client `vault` : `VAULT_ADDR`, `VAULT_NAMESPACE`, `VAULT_TOKEN` (puis le fichier `~/.vault-token`), et pour un AppRole `VAULT_ROLE_ID` et `VAULT_SECRET_ID`. L'ordre est : réglages de l'application, identifiants du trousseau, variables d'environnement, `~/.vault-token`.

## Ligne de commande

`xc run`, `xc ws` et `xc grpc` n'ont pas d'option propre : ils lisent `VAULT_ADDR`, `VAULT_TOKEN` (ou `~/.vault-token`) et `VAULT_NAMESPACE`, comme n'importe quel outil Vault. Sans adresse, une requête qui réclame une variable Vault échoue avec « Vault n'est pas configuré ».

## Ce qui n'y est pas

- Pas d'autre méthode de connexion que le jeton et l'AppRole (ni OIDC, ni LDAP, ni Kubernetes) : un jeton obtenu autrement s'utilise tel quel.
- Pas de moteurs de secrets dynamiques (base de données, AWS…), ni d'écriture dans Vault : lecture de champs KV seulement.
- Pas de sélecteur de secret dans l'éditeur : la référence s'écrit à la main.
- Les secrets de Vault ne sont pas masqués dans la réponse ni dans l'historique des envois, comme ceux du trousseau : les rapports `xc run` peuvent les contenir, utiliser les options de masquage des rapports.
