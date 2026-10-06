using System;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;

using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("POST"), "http://127.0.0.1:8765/users");
request.Headers.TryAddWithoutValidation("Authorization", "Bearer abc.def");
request.Content = new StringContent("{\n  \"name\": \"Aminata \\\"Ami\\\" Diallo\",\n  \"note\": \"café $HOME ${x} 日本語\\n\",\n  \"path\": \"C:\\\\temp\"\n}", Encoding.UTF8);
request.Content.Headers.ContentType = MediaTypeHeaderValue.Parse("application/json; charset=utf-8");

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
