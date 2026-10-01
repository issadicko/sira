curl --request POST \
  --url https://api.example.com/v1/tickets \
  --header 'Authorization: Basic YWRhOnB3ZA==' \
  --header 'Content-Type: application/json' \
  --data '{"subject":"Panne","priority":"high"}'
