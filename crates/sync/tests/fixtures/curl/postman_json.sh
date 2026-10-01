curl --location 'https://api.example.com/v1/customers' \
--header 'Content-Type: application/json' \
--header 'Authorization: Bearer {{token}}' \
--data-raw '{
    "firstName": "Ada",
    "lastName": "Lovelace",
    "tags": ["vip", "beta"]
}'
