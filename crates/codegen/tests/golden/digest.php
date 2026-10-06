<?php

// Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
$curl = curl_init();

curl_setopt_array($curl, [
  CURLOPT_URL => 'http://127.0.0.1:8765/secure',
  CURLOPT_RETURNTRANSFER => true,
  CURLOPT_CUSTOMREQUEST => 'GET',
  CURLOPT_HTTPAUTH => CURLAUTH_DIGEST,
  CURLOPT_USERPWD => 'ada:p@ss',
]);

$response = curl_exec($curl);
$error = curl_error($curl);

curl_close($curl);

if ($error) {
  echo 'cURL Error #:' . $error;
} else {
  echo $response;
}
