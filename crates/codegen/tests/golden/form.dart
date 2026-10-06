import 'package:http/http.dart' as http;

Future<void> main() async {
  final request = http.Request('POST', Uri.parse('http://127.0.0.1:8765/login'));
  request.headers.addAll({
    'Content-Type': 'application/x-www-form-urlencoded',
  });
  request.body = 'grant=password&user=x%40y.test&scope=a+b';

  final response = await http.Response.fromStream(await request.send());
  print(response.statusCode);
  print(response.body);
}
