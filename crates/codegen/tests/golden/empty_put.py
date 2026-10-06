import requests

url = "http://127.0.0.1:8765/items/7"


response = requests.request("PUT", url)

print(response.text)
