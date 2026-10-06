import requests

url = "http://127.0.0.1:8765/items/7"

headers = {
    "X-Reason": "cleanup"
}

response = requests.request("DELETE", url, headers=headers)

print(response.text)
