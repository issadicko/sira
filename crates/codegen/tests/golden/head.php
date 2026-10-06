<?php

$curl = curl_init();

curl_setopt_array($curl, [
  CURLOPT_URL => 'http://127.0.0.1:8765/items',
  CURLOPT_RETURNTRANSFER => true,
  CURLOPT_NOBODY => true,
]);

$response = curl_exec($curl);
$error = curl_error($curl);

curl_close($curl);

if ($error) {
  echo 'cURL Error #:' . $error;
} else {
  echo $response;
}
