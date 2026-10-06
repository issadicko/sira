// Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.
// Digest : package:http ne le gère pas, utilise un client qui le fait (http_auth).
import 'package:http/http.dart' as http;

Future<void> main() async {
  final request = http.Request('GET', Uri.parse('http://127.0.0.1:8765/secure'));

  final response = await http.Response.fromStream(await request.send());
  print(response.statusCode);
  print(response.body);
}
