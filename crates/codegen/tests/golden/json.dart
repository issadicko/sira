import 'package:http/http.dart' as http;

Future<void> main() async {
  final request = http.Request('POST', Uri.parse('http://127.0.0.1:8765/users'));
  request.headers.addAll({
    'Content-Type': 'application/json; charset=utf-8',
    'Authorization': 'Bearer abc.def',
  });
  request.body = '{\n  "name": "Aminata \\"Ami\\" Diallo",\n  "note": "café \$HOME \${x} 日本語\\n",\n  "path": "C:\\\\temp"\n}';

  final response = await http.Response.fromStream(await request.send());
  print(response.statusCode);
  print(response.body);
}
