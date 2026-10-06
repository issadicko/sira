<?php

$curl = curl_init();

curl_setopt_array($curl, [
  CURLOPT_URL => 'http://127.0.0.1:8765/login',
  CURLOPT_RETURNTRANSFER => true,
  CURLOPT_CUSTOMREQUEST => 'POST',
  CURLOPT_POSTFIELDS => 'grant=password&user=x%40y.test&scope=a+b',
  CURLOPT_HTTPHEADER => [
    'Content-Type: application/x-www-form-urlencoded',
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
