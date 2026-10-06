import 'package:http/http.dart' as http;

Future<void> main() async {
  final request = http.Request('PATCH', Uri.parse('http://127.0.0.1:8765/items/7'));
  request.headers.addAll({
    'Content-Type': 'text/plain',
  });
  request.body = 'quoi ? ça va\r\ntrès bien';

  final response = await http.Response.fromStream(await request.send());
  print(response.statusCode);
  print(response.body);
}
