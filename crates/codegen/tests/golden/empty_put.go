package main

import (
	"fmt"
	"io"
	"net/http"
)

func main() {
	url := "http://127.0.0.1:8765/items/7"

	req, err := http.NewRequest("PUT", url, nil)
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
