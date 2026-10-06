// Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
// Digest : passe par un HttpClientHandler avec des identifiants.
// var handler = new HttpClientHandler { Credentials = new System.Net.NetworkCredential("ada", "p@ss") };
using System;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;

using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("GET"), "http://127.0.0.1:8765/secure");

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
