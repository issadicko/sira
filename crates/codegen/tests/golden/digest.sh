# Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
curl --request GET \
  --url 'http://127.0.0.1:8765/secure' \
  --digest \
  --user 'ada:p@ss'
