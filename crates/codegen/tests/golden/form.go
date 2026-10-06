package main

import (
	"fmt"
	"io"
	"net/http"
	"strings"
)

func main() {
	url := "http://127.0.0.1:8765/login"

	payload := strings.NewReader("grant=password&user=x%40y.test&scope=a+b")

	req, err := http.NewRequest("POST", url, payload)
	if err != nil {
		panic(err)
	}

	req.Header.Add("Content-Type", "application/x-www-form-urlencoded")

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
