// Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
const options = {
  method: "GET"
};

fetch("http://127.0.0.1:8765/secure", options)
  .then((response) => response.text())
  .then((body) => console.log(body))
  .catch((error) => console.error(error));
