// Oracle : exécute le vrai code de Bruno (bundle des sources, voir setup.sh et README.md).
const fs = require('fs');
const path = require('path');
const b = require('./bundle.js');

const [cmd, ...args] = process.argv.slice(2);
const read = (f) => fs.readFileSync(f, 'utf8');
const readJson = (f) => JSON.parse(read(f));

const stripUids = (v) => {
  if (Array.isArray(v)) return v.map(stripUids);
  if (v && typeof v === 'object') {
    const out = {};
    for (const [k, x] of Object.entries(v)) if (k !== 'uid') out[k] = stripUids(x);
    return out;
  }
  return v;
};

const convert = (specFile, group) => b.openApiToBruno(read(specFile), { groupBy: group || 'tags' });

const MAX_FILENAME_LENGTH = 255;
const safePath = (filePath) => {
  const dir = path.dirname(filePath);
  const ext = path.extname(filePath);
  let base = path.basename(filePath, ext);
  if (base.length + ext.length > MAX_FILENAME_LENGTH) {
    base = b.sanitizeName(base).slice(0, MAX_FILENAME_LENGTH - ext.length);
  }
  return path.join(dir, base + ext);
};

const importCollection = (coll, location) => {
  const format = 'yml';
  const collectionPath = path.join(location, b.sanitizeName(coll.name));
  fs.mkdirSync(collectionPath, { recursive: true });
  const filename = (item) => {
    if (item?.filename) {
      const ext = path.extname(item.filename);
      return ext === '.bru' || ext === '.yml' ? item.filename.replace(ext, `.${format}`) : item.filename;
    }
    return `${item.name}.${format}`;
  };
  const walk = (items = [], current) => {
    for (const item of items) {
      if (['http-request', 'graphql-request', 'grpc-request', 'ws-request'].includes(item.type)) {
        fs.writeFileSync(safePath(path.join(current, b.sanitizeName(filename(item)))), b.stringifyItem(item));
      }
      if (item.type === 'folder') {
        const folderPath = path.join(current, b.sanitizeName(item?.filename || item?.name));
        fs.mkdirSync(folderPath, { recursive: true });
        if (item?.root?.meta?.name) {
          item.root.meta.seq = item.seq;
          fs.writeFileSync(safePath(path.join(folderPath, `folder.${format}`)), b.stringifyFolder(item.root));
        }
        if (item.items && item.items.length) walk(item.items, folderPath);
      }
    }
  };
  const brunoConfig = coll.brunoConfig || { name: coll.name, type: 'collection', ignore: ['node_modules', '.git'] };
  brunoConfig.opencollection = '1.0.0';
  fs.writeFileSync(path.join(collectionPath, 'opencollection.yml'), b.stringifyCollection(coll.root, brunoConfig, { format }));
  walk(coll.items, collectionPath);
  const envDir = path.join(collectionPath, 'environments');
  if (!fs.existsSync(envDir)) fs.mkdirSync(envDir);
  for (const env of coll.environments || []) {
    fs.writeFileSync(safePath(path.join(envDir, b.sanitizeName(`${env.name}.${format}`))), b.stringifyEnvironment(env, { format }));
  }
  return collectionPath;
};

const out = (s) => process.stdout.write(typeof s === 'string' ? s : JSON.stringify(s, null, 2) + '\n');

switch (cmd) {
  case 'openapi':
    out(stripUids(convert(args[0], args[1])));
    break;
  case 'import':
    out(importCollection(convert(args[0], args[2]), args[1]) + '\n');
    break;
  case 'curl':
    out(b.getRequestFromCurlCommand(read(args[0]), args[1]) ?? null);
    break;
  case 'stringify-item':
    out(b.stringifyItem(readJson(args[0])));
    break;
  case 'stringify-folder':
    out(b.stringifyFolder(readJson(args[0])));
    break;
  case 'stringify-collection':
    out(b.stringifyCollection(readJson(args[0]), readJson(args[1]), { format: 'yml' }));
    break;
  case 'stringify-environment':
    out(b.stringifyEnvironment(readJson(args[0]), { format: 'yml' }));
    break;
  default:
    console.error('unknown command');
    process.exit(2);
}
