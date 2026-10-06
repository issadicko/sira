curl --request POST \
  --url 'http://127.0.0.1:8765/users' \
  --header 'Content-Type: application/json; charset=utf-8' \
  --header 'Authorization: Bearer abc.def' \
  --data-raw '{
  "name": "Aminata \"Ami\" Diallo",
  "note": "café $HOME ${x} 日本語\n",
  "path": "C:\\temp"
}'
