const http = require('http');
const port = Number(process.argv[2] || 0);
const server = http.createServer((req, res) => {
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    const raw = Buffer.concat(chunks).toString('utf8');
    const url = new URL(req.url, 'http://x');
    const query = {};
    for (const [k, v] of url.searchParams) query[k] = v;
    const size = Buffer.concat(chunks).length;
    let body = raw;
    if (size > 65536) body = `<${size} octets>`;
    else try { body = JSON.parse(raw); } catch {}
    const payload = { method: req.method, path: url.pathname, query, body, access_token: 'echo-token', token_type: 'Bearer', expires_in: 3600 };
    const text = JSON.stringify(payload);
    res.writeHead(200, { 'content-type': 'application/json', 'x-echo': '1', 'content-length': Buffer.byteLength(text) });
    res.end(text);
  });
});
server.listen(port, '127.0.0.1', () => console.log(server.address().port));
