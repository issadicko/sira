const options = {
  method: "POST",
  headers: {
    "Content-Type": "application/x-www-form-urlencoded"
  },
  body: "grant=password&user=x%40y.test&scope=a+b"
};

fetch("http://127.0.0.1:8765/login", options)
  .then((response) => response.text())
  .then((body) => console.log(body))
  .catch((error) => console.error(error));
