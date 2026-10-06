const options = {
  method: "GET",
  headers: {
    "Accept": "application/json",
    "X-Odd": "valé \"double\" 'simple' $dollar \\anti `tick`"
  }
};

fetch("http://127.0.0.1:8765/items?x=1&y=a%20b", options)
  .then((response) => response.text())
  .then((body) => console.log(body))
  .catch((error) => console.error(error));
