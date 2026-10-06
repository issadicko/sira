package main

import (
	"fmt"
	"io"
	"net/http"
)

func main() {
	url := "http://127.0.0.1:8765/items?x=1&y=a%20b"

	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		panic(err)
	}

	req.Header.Add("Accept", "application/json")
	req.Header.Add("X-Odd", "valé \"double\" 'simple' $dollar \\anti `tick`")

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
