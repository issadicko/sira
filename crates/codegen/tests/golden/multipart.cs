using System;
using System.IO;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;

using var client = new HttpClient();
using var request = new HttpRequestMessage(new HttpMethod("POST"), "http://127.0.0.1:8765/upload");
request.Headers.TryAddWithoutValidation("X-Trace", "1");
using var form = new MultipartFormDataContent();
form.Add(new StringContent("Photo; \"cover\""), "title");
var part1 = new StringContent("typed");
part1.Headers.ContentType = MediaTypeHeaderValue.Parse("text/plain");
form.Add(part1, "note");
var part2 = new ByteArrayContent(File.ReadAllBytes("files/a.png"));
part2.Headers.ContentType = MediaTypeHeaderValue.Parse("image/png");
form.Add(part2, "avatar", "a.png");
var part3 = new ByteArrayContent(File.ReadAllBytes("files/b.txt"));
form.Add(part3, "doc", "b.txt");
request.Content = form;

using var response = await client.SendAsync(request);
Console.WriteLine((int)response.StatusCode);
Console.WriteLine(await response.Content.ReadAsStringAsync());
