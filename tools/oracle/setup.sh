#!/bin/sh
set -eu
cd "$(dirname "$0")"
COMMIT=a2dbda9fc73d40aba3611257f683efd0336a6555
if [ ! -d bruno ]; then
  git clone --filter=blob:none --no-checkout https://github.com/usebruno/bruno.git bruno
  git -C bruno sparse-checkout set --no-cone \
    packages/bruno-filestore packages/bruno-converters packages/bruno-common/src packages/bruno-schema \
    packages/bruno-app/src/utils/curl packages/bruno-app/src/utils/common
fi
git -C bruno checkout -q "$COMMIT"
npm install --no-audit --no-fund
P=./bruno/packages
NODE_PATH="$PWD/node_modules" ./node_modules/.bin/esbuild entry.js --bundle --platform=node --format=cjs \
  --outfile=bundle.js --log-level=error --resolve-extensions=.ts,.js,.json --main-fields=module,main \
  --alias:utils/common/index=./common-shim.js \
  --alias:@usebruno/common/utils=$P/bruno-common/src/utils/index.ts \
  --alias:@usebruno/common=$P/bruno-common/src/index.ts \
  --alias:@usebruno/schema=$P/bruno-schema/src/index.js \
  --alias:@faker-js/faker=./faker-stub.ts
echo "oracle prêt : node tools/oracle/oracle.js"
