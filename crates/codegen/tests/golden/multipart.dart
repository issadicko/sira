import 'package:http/http.dart' as http;
import 'package:http_parser/http_parser.dart';

Future<void> main() async {
  final request = http.MultipartRequest('POST', Uri.parse('http://127.0.0.1:8765/upload'));
  request.headers.addAll({
    'X-Trace': '1',
  });
  request.fields['title'] = 'Photo; "cover"';
  request.files.add(http.MultipartFile.fromString('note', 'typed', contentType: MediaType.parse('text/plain')));
  request.files.add(await http.MultipartFile.fromPath('avatar', 'files/a.png', filename: 'a.png', contentType: MediaType.parse('image/png')));
  request.files.add(await http.MultipartFile.fromPath('doc', 'files/b.txt', filename: 'b.txt'));

  final response = await http.Response.fromStream(await request.send());
  print(response.statusCode);
  print(response.body);
}
