package main

import (
	"fmt"
	"io"
	"net/http"
	"strings"
)

func main() {
	url := "http://127.0.0.1:8765/items/7"

	payload := strings.NewReader("quoi ? ça va\r\ntrès bien")

	req, err := http.NewRequest("PATCH", url, payload)
	if err != nil {
		panic(err)
	}

	req.Header.Add("Content-Type", "text/plain")

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
