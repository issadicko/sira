# Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
import requests
from requests.auth import HTTPDigestAuth

url = "http://127.0.0.1:8765/secure"


response = requests.request("GET", url, auth=HTTPDigestAuth("ada", "p@ss"))

print(response.text)
