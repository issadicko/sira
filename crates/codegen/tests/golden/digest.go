// Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
// Digest : net/http ne le gère pas, ajoute un client qui le fait.
package main

import (
	"fmt"
	"io"
	"net/http"
)

func main() {
	url := "http://127.0.0.1:8765/secure"

	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		panic(err)
	}

	res, err := http.DefaultClient.Do(req)
	if err != nil {
		panic(err)
	}
	defer res.Body.Close()

	body, err := io.ReadAll(res.Body)
	if err != nil {
		panic(err)
	}

	fmt.Println(res.Status)
	fmt.Println(string(body))
}
