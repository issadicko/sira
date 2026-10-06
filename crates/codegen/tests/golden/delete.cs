using System;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;

using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("DELETE"), "http://127.0.0.1:8765/items/7");
request.Headers.TryAddWithoutValidation("X-Reason", "cleanup");

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
