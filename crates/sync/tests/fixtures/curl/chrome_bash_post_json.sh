curl 'https://api.example.com/v1/users' \
  -H 'accept: application/json' \
  -H 'content-type: application/json' \
  -H 'origin: https://app.example.com' \
  -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36' \
  --data-raw '{"name":"Jeanne Dupont","email":"jeanne@example.com","roles":["admin","editor"],"active":true,"age":42}'
