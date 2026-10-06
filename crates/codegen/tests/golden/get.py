import requests

url = "http://127.0.0.1:8765/items?x=1&y=a%20b"

headers = {
    "Accept": "application/json",
    "X-Odd": "valé \"double\" 'simple' $dollar \\anti `tick`"
}

response = requests.request("GET", url, headers=headers)

print(response.text)
