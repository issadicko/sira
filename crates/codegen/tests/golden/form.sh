curl --request POST \
  --url 'http://127.0.0.1:8765/login' \
  --header 'Content-Type: application/x-www-form-urlencoded' \
  --data-raw 'grant=password&user=x%40y.test&scope=a+b'
