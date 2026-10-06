import requests

url = "http://127.0.0.1:8765/login"

payload = "grant=password&user=x%40y.test&scope=a+b"
headers = {
    "Content-Type": "application/x-www-form-urlencoded"
}

response = requests.request("POST", url, data=payload, headers=headers)

print(response.text)
