curl 'https://api.example.com/v1/notes' \
  -H 'content-type: application/json' \
  --data-raw $'{"title":"L\'été","body":"ligne 1\nligne 2\ttab","emoji":"\u00e9"}'
