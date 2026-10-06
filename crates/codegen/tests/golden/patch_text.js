const options = {
  method: "PATCH",
  headers: {
    "Content-Type": "text/plain"
  },
  body: "quoi ? ça va\r\ntrès bien"
};

fetch("http://127.0.0.1:8765/items/7", options)
  .then((response) => response.text())
  .then((body) => console.log(body))
  .catch((error) => console.error(error));
