import requests

url = "http://127.0.0.1:8765/items"


response = requests.request("HEAD", url)

print(response.text)
