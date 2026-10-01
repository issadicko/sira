curl -X POST https://api.example.com/v1/orders \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer {{accessToken}}' \
  -d '{"customerId": {{customerId}}, "note": "{{note}}", "items": [{"sku": "{{sku}}", "qty": 2}]}'
