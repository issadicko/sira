package main

import (
	"bytes"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"net/textproto"
	"os"
)

func main() {
	url := "http://127.0.0.1:8765/upload"

	payload := &bytes.Buffer{}
	writer := multipart.NewWriter(payload)
	if err := writer.WriteField("title", "Photo; \"cover\""); err != nil {
		panic(err)
	}
	{
		h := make(textproto.MIMEHeader)
		h.Set("Content-Disposition", fmt.Sprintf("form-data; name=%q", "note"))
		h.Set("Content-Type", "text/plain")
		part, err := writer.CreatePart(h)
		if err != nil {
			panic(err)
		}
		if _, err := part.Write([]byte("typed")); err != nil {
			panic(err)
		}
	}
	{
		file, err := os.Open("files/a.png")
		if err != nil {
			panic(err)
		}
		defer file.Close()
		h := make(textproto.MIMEHeader)
		h.Set("Content-Disposition", fmt.Sprintf("form-data; name=%q; filename=%q", "avatar", "a.png"))
		h.Set("Content-Type", "image/png")
		part, err := writer.CreatePart(h)
		if err != nil {
			panic(err)
		}
		if _, err := io.Copy(part, file); err != nil {
			panic(err)
		}
	}
	{
		file, err := os.Open("files/b.txt")
		if err != nil {
			panic(err)
		}
		defer file.Close()
		h := make(textproto.MIMEHeader)
		h.Set("Content-Disposition", fmt.Sprintf("form-data; name=%q; filename=%q", "doc", "b.txt"))
		h.Set("Content-Type", "application/octet-stream")
		part, err := writer.CreatePart(h)
		if err != nil {
			panic(err)
		}
		if _, err := io.Copy(part, file); err != nil {
			panic(err)
		}
	}
	if err := writer.Close(); err != nil {
		panic(err)
	}

	req, err := http.NewRequest("POST", url, payload)
	if err != nil {
		panic(err)
	}

	req.Header.Add("X-Trace", "1")
	req.Header.Set("Content-Type", writer.FormDataContentType())

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
