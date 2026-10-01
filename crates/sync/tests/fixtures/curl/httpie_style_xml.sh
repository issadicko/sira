curl -X POST https://soap.example.com/ws \
  -H "Content-Type: text/xml; charset=utf-8" \
  -H "SOAPAction: urn:GetPrice" \
  -d '<?xml version="1.0"?><soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope"><soap:Body><GetPrice><Item>Apple</Item></GetPrice></soap:Body></soap:Envelope>'
