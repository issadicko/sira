using System;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;

using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("GET"), "http://127.0.0.1:8765/items?x=1&y=a%20b");
request.Headers.TryAddWithoutValidation("Accept", "application/json");
request.Headers.TryAddWithoutValidation("X-Odd", "valé \"double\" 'simple' $dollar \\anti `tick`");

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
