using System;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;

using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("POST"), "http://127.0.0.1:8765/login");
request.Content = new StringContent("grant=password&user=x%40y.test&scope=a+b", Encoding.UTF8);
request.Content.Headers.ContentType = MediaTypeHeaderValue.Parse("application/x-www-form-urlencoded");

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
