import { readFile } from 'node:fs/promises';

import { build } from 'esbuild';

// moment charge ses langues par `require('./locale/' + nom)` dans un try/catch : seule l'anglaise est embarquée.
const withoutLocales = {
  name: 'without-locales',
  setup: (b) =>
    b.onLoad({ filter: /moment\.min\.js$/ }, async (args) => ({
      contents: (await readFile(args.path, 'utf8')).replace('require("./locale/"+t)', 'null'),
    })),
};

await build({
  entryPoints: ['chai.js', 'ajv.js', 'crypto-js.js', 'moment.js', 'tv4.js', 'uuid.js', 'nanoid.js', 'buffer.js'],
  bundle: true,
  format: 'iife',
  minify: true,
  target: 'es2020',
  external: ['crypto'],
  plugins: [withoutLocales],
  outdir: '../../crates/script/js/libs',
  logLevel: 'warning',
});
