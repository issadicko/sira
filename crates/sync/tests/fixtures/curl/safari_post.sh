curl 'https://api.example.com/v2/items' \
-X 'POST' \
-H 'Content-Type: application/json' \
-H 'Accept: application/json' \
-H 'Origin: https://www.example.com' \
-H 'User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15' \
--data-binary '{"sku":"A-42","quantity":3}'
