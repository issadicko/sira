curl 'https://upload.example.com/files' \
  -H 'accept: */*' \
  -H 'content-type: multipart/form-data; boundary=----WebKitFormBoundary7MA4YWxkTrZu0gW' \
  -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36' \
  --data-raw $'------WebKitFormBoundary7MA4YWxkTrZu0gW\r\nContent-Disposition: form-data; name="description"\r\n\r\nRapport\r\n------WebKitFormBoundary7MA4YWxkTrZu0gW--\r\n'
