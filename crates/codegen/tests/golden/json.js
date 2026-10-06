const options = {
  method: "POST",
  headers: {
    "Content-Type": "application/json; charset=utf-8",
    "Authorization": "Bearer abc.def"
  },
  body: "{\n  \"name\": \"Aminata \\\"Ami\\\" Diallo\",\n  \"note\": \"café $HOME ${x} 日本語\\n\",\n  \"path\": \"C:\\\\temp\"\n}"
};

fetch("http://127.0.0.1:8765/users", options)
  .then((response) => response.text())
  .then((body) => console.log(body))
  .catch((error) => console.error(error));
