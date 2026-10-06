#!/bin/sh
set -e
cd "$(dirname "$0")"
npm install --no-audit --no-fund
./node_modules/.bin/bru --version
