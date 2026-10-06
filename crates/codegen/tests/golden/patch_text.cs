using System;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;

using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("PATCH"), "http://127.0.0.1:8765/items/7");
request.Content = new StringContent("quoi ? ça va\r\ntrès bien", Encoding.UTF8);
request.Content.Headers.ContentType = MediaTypeHeaderValue.Parse("text/plain");

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
