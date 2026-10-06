import requests

url = "http://127.0.0.1:8765/upload"

files = [
    ("title", (None, "Photo; \"cover\"")),
    ("note", (None, "typed", "text/plain")),
    ("avatar", ("a.png", open("files/a.png", "rb"), "image/png")),
    ("doc", ("b.txt", open("files/b.txt", "rb")))
]
headers = {
    "X-Trace": "1"
}

response = requests.request("POST", url, files=files, headers=headers)

print(response.text)
