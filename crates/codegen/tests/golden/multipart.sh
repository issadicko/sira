curl --request POST \
  --url 'http://127.0.0.1:8765/upload' \
  --header 'X-Trace: 1' \
  --form-string 'title=Photo; "cover"' \
  --form 'note=typed;type=text/plain' \
  --form 'avatar=@"files/a.png";type=image/png' \
  --form 'doc=@"files/b.txt"'
