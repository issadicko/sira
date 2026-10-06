import requests

url = "http://127.0.0.1:8765/items/7"

payload = "quoi ? ça va\r\ntrès bien".encode("utf-8")
headers = {
    "Content-Type": "text/plain"
}

response = requests.request("PATCH", url, data=payload, headers=headers)

print(response.text)
