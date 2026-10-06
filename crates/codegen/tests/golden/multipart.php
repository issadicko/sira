<?php

$curl = curl_init();

curl_setopt_array($curl, [
  CURLOPT_URL => 'http://127.0.0.1:8765/upload',
  CURLOPT_RETURNTRANSFER => true,
  CURLOPT_CUSTOMREQUEST => 'POST',
  CURLOPT_POSTFIELDS => [
    'title' => 'Photo; "cover"',
    'note' => 'typed',
    'avatar' => new CURLFile('files/a.png', 'image/png', 'a.png'),
    'doc' => new CURLFile('files/b.txt', null, 'b.txt'),
  ],
  CURLOPT_HTTPHEADER => [
    'X-Trace: 1',
  ],
]);

$response = curl_exec($curl);
$error = curl_error($curl);

curl_close($curl);

if ($error) {
  echo 'cURL Error #:' . $error;
} else {
  echo $response;
}
