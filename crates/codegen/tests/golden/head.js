const options = {
  method: "HEAD"
};

fetch("http://127.0.0.1:8765/items", options)
  .then((response) => response.text())
  .then((body) => console.log(body))
  .catch((error) => console.error(error));
