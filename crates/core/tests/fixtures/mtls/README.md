# Certificat client de test

Matériel jetable pour les tests de `xc-core` (certificat auto-signé `CN=client.test`, valable cent ans) : ce ne sont pas des secrets.
Toutes les passphrases sont `secret`, sauf `nopass.p12` (passphrase vide).

| Fichier | Contenu |
| --- | --- |
| `cert.pem`, `key.pem` | le certificat et sa clé PKCS#8 en clair |
| `modern.p12` | `openssl pkcs12 -export` (OpenSSL 3 : PBES2, AES-256, MAC SHA-256) |
| `legacy.p12` | `openssl pkcs12 -export -legacy` (3DES, SHA-1) |
| `nopass.p12` | PKCS#12 sans passphrase |
| `key-encrypted.pem` | `openssl pkcs8 -topk8 -v2 aes-256-cbc` |
| `key-encrypted-3des.pem` | `openssl pkcs8 -topk8 -v1 PBE-SHA1-3DES` (ancien schéma PBES1, refusé avec un message) |
| `key-legacy-encrypted.pem` | `openssl rsa -aes256 -traditional` (ancien format `Proc-Type`, refusé avec un message) |
