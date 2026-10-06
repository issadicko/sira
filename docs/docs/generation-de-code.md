# Génération de code (V1, EF-GEN-01)

Une requête se transcrit en code dans neuf langages. L'extrait reproduit **ce que Sira enverrait** : mêmes méthode, adresse, en-têtes (collection, dossiers, requête, auth) et corps, variables remplacées. Il se génère depuis la requête ouverte, brouillon non enregistré compris.

| Langage | Bibliothèque | Identifiant |
| --- | --- | --- |
| cURL | `curl` | `curl` |
| JavaScript | `fetch` (navigateur ou Node 18 et plus) | `javascript` |
| Python | `requests` | `python` |
| Go | `net/http` | `go` |
| Java | OkHttp | `java` |
| Kotlin | OkHttp | `kotlin` |
| Dart | `package:http` | `dart` |
| PHP | extension cURL | `php` |
| C# | `HttpClient` (C# 9 et plus, instructions de niveau supérieur) | `csharp` |

## Dans l'application

Le bouton `</>` de la ligne du chemin de la requête, ou « Générer du code… » dans la palette de commandes, ouvre le dialogue : choisir le langage, lire l'extrait, le copier (⌘↵). Le dernier langage choisi est retenu.

Les variables sans valeur dans l'environnement actif restent entre accolades (`{{marchand}}`) et le dialogue les liste. **Les secrets du trousseau n'en sortent pas** : l'extrait porte un repère `<token>` à leur place, jamais la valeur (ENF-SEC-01). Les variables écrites par les scripts de la session (`bru.setVar`) sont prises en compte.

## En ligne de commande

```bash
xc code ~/collections/Boutique "Produits/Creer.yml" --lang python --env prod --env-var canal=web
```

Le premier argument est le dossier de la collection, le second le chemin de la requête dans la collection. L'extrait est écrit sur la sortie standard ; une variable sans valeur est signalée sur la sortie d'erreur (code de sortie 0). Un langage ou une requête inconnus donnent le code 2. La ligne de commande ne lit pas le trousseau : un secret reste entre accolades, à passer par `--env-var`.

## Ce que chaque extrait fait

- **Corps** : JSON, texte, XML et formulaire encodé sont transcrits tels qu'envoyés (octet pour octet), avec leur `Content-Type`. Un formulaire multipart garde ses champs : texte, et fichiers par leur chemin (les fichiers ne sont pas lus). Le client choisit la frontière, l'extrait n'écrit donc pas de `Content-Type` pour lui.
- **Méthodes sans corps** : OkHttp refuse `POST`, `PUT` et `PATCH` sans corps ; l'extrait passe un corps vide. `HEAD` s'écrit `--head` en cURL et `CURLOPT_NOBODY` en PHP.
- **Auth** : Basic, Bearer, API Key sont des en-têtes (ou un paramètre d'adresse) et figurent donc dans l'extrait. Digest s'écrit `--digest` en cURL, `HTTPDigestAuth` en Python et `CURLAUTH_DIGEST` en PHP, et se signale en commentaire ailleurs. AWS Signature V4 s'écrit `--aws-sigv4` en cURL et se signale en commentaire ailleurs. OAuth 2.0 : un commentaire demande d'ajouter l'en-tête `Authorization` avec le jeton obtenu (Sira le demande au moment de l'envoi, pas à la génération).
- **Échappement** : chaque langage échappe à sa façon (`$` en Kotlin et en Dart, `\u000a` interdit en Java, apostrophes en shell…).

## Limites connues

- Les requêtes gRPC et WebSocket ne sont pas transcrites. Une requête GraphQL l'est comme une requête HTTP : le corps JSON `{"query": …, "variables": …}` tel qu'il partirait (voir `graphql.md`).
- Un nom d'en-tête répété n'est gardé qu'une fois en Python et en Dart (dictionnaires) ; l'extrait le dit en commentaire.
- Un caractère hors ASCII dans un en-tête : OkHttp (`addUnsafeNonAscii`) et les autres l'acceptent, Dart le refuse (commentaire dans l'extrait).
- Le mode démo du navigateur ne produit que du cURL simplifié.

## Vérification

Les extraits de référence sont dans `crates/codegen/tests/golden/`. En plus de leur comparaison en CI, ils ont été exécutés contre un serveur local pour huit des neuf langages (cURL, JavaScript, Python, Go, Java, Kotlin, Dart, PHP) sur neuf requêtes : GET avec en-têtes et caractères spéciaux, JSON multiligne avec guillemets, barres obliques inverses, `$`, emoji et texte japonais, formulaire encodé, multipart avec deux fichiers, PUT vide, DELETE, HEAD, PATCH en texte avec fins de ligne `\r\n`, Digest. Le serveur contrôle méthode, chemin, en-têtes et corps reçus. C# n'a pas été compilé (pas de SDK .NET sur la machine de développement).
