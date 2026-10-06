// L'en-tête X-Odd contient un caractère hors ASCII : dart:io le refuse.
import 'package:http/http.dart' as http;

Future<void> main() async {
  final request = http.Request('GET', Uri.parse('http://127.0.0.1:8765/items?x=1&y=a%20b'));
  request.headers.addAll({
    'Accept': 'application/json',
    'X-Odd': 'valé "double" \'simple\' \$dollar \\anti `tick`',
  });

  final response = await http.Response.fromStream(await request.send());
  print(response.statusCode);
  print(response.body);
}
