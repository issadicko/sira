package main

import (
	"fmt"
	"io"
	"net/http"
	"strings"
)

func main() {
	url := "http://127.0.0.1:8765/users"

	payload := strings.NewReader(`{
  "name": "Aminata \"Ami\" Diallo",
  "note": "café $HOME ${x} 日本語\n",
  "path": "C:\\temp"
}`)

	req, err := http.NewRequest("POST", url, payload)
	if err != nil {
		panic(err)
	}

	req.Header.Add("Content-Type", "application/json; charset=utf-8")
	req.Header.Add("Authorization", "Bearer abc.def")

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
