import 'package:http/http.dart' as http;

Future<void> main() async {
  final request = http.Request('DELETE', Uri.parse('http://127.0.0.1:8765/items/7'));
  request.headers.addAll({
    'X-Reason': 'cleanup',
  });

  final response = await http.Response.fromStream(await request.send());
  print(response.statusCode);
  print(response.body);
}
