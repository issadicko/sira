curl 'https://shop.example.com/api/cart' \
  -H 'accept: */*' \
  -b 'session_id=8f14e45fceea167a5a36dedd4bea2543; _ga=GA1.1.1234567890.1700000000; consent=%7B%22ads%22%3Afalse%7D' \
  -H 'referer: https://shop.example.com/cart' \
  -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36'
