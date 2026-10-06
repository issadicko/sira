import requests

url = "http://127.0.0.1:8765/users"

payload = "{\n  \"name\": \"Aminata \\\"Ami\\\" Diallo\",\n  \"note\": \"café $HOME ${x} 日本語\\n\",\n  \"path\": \"C:\\\\temp\"\n}".encode("utf-8")
headers = {
    "Content-Type": "application/json; charset=utf-8",
    "Authorization": "Bearer abc.def"
}

response = requests.request("POST", url, data=payload, headers=headers)

print(response.text)
