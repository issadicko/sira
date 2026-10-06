<?php

$curl = curl_init();

curl_setopt_array($curl, [
  CURLOPT_URL => 'http://127.0.0.1:8765/users',
  CURLOPT_RETURNTRANSFER => true,
  CURLOPT_CUSTOMREQUEST => 'POST',
  CURLOPT_POSTFIELDS => '{
  "name": "Aminata \\"Ami\\" Diallo",
  "note": "café $HOME ${x} 日本語\\n",
  "path": "C:\\\\temp"
}',
  CURLOPT_HTTPHEADER => [
    'Content-Type: application/json; charset=utf-8',
    'Authorization: Bearer abc.def',
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
