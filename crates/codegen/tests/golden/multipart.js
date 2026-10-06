const fs = require('node:fs');

const form = new FormData();
form.append("title", "Photo; \"cover\"");
form.append("note", new Blob(["typed"], { type: "text/plain" }));
form.append("avatar", new Blob([fs.readFileSync("files/a.png")], { type: "image/png" }), "a.png");
form.append("doc", new Blob([fs.readFileSync("files/b.txt")]), "b.txt");

const options = {
  method: "POST",
  headers: {
    "X-Trace": "1"
  },
  body: form
};

fetch("http://127.0.0.1:8765/upload", options)
  .then((response) => response.text())
  .then((body) => console.log(body))
  .catch((error) => console.error(error));
