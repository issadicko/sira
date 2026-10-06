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
    let body = raw;
    try { body = JSON.parse(raw); } catch {}
    const payload = { method: req.method, path: url.pathname, query, body };
    const text = JSON.stringify(payload);
    res.writeHead(200, { 'content-type': 'application/json', 'x-echo': '1', 'content-length': Buffer.byteLength(text) });
    res.end(text);
  });
});
server.listen(port, '127.0.0.1', () => console.log(server.address().port));
