curl --location --request POST 'https://api.example.com/v1/documents' \
--header 'X-Api-Key: {{apiKey}}' \
--form 'title="Facture mars"' \
--form 'file=@"/Users/ada/Documents/facture.pdf"' \
--form 'category=billing'
