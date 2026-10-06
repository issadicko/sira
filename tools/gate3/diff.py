#!/usr/bin/env python3
"""Gate 3 (ENF-COMP-03) : `xc run` et `bru run` sur chaque collection du corpus, comparés requête par requête.

Chaque collection est copiée dans un dossier temporaire où les adresses http(s) sont réécrites vers un serveur local
(`echo-server.js`) : les deux outils parlent au même serveur déterministe. Sont comparés : le statut de chaque
requête, le code HTTP, les assertions, les tests (et ceux des scripts pré-requête et post-réponse) et l'erreur.

    tools/gate3/setup.sh                      # installe @usebruno/cli 4.2.1
    cargo build -p xc-cli                     # target/debug/xc
    python3 tools/gate3/diff.py [id ...]      # tout le corpus, ou les collections nommées

Le corpus est celui de `crates/core/tests/corpus/manifest.json`, récupéré dans `target/corpus` par le test Gate 1
(`cargo test -p xc-core --test corpus`). `SHOW=n` limite le nombre de requêtes différentes détaillées par collection.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, '..', '..'))
BRU = os.path.join(HERE, 'node_modules', '.bin', 'bru')
XC = os.path.join(ROOT, 'target', 'debug', 'xc')
PORT = 4599
BASE = f'http://127.0.0.1:{PORT}'
TIMEOUT = 150
HOSTS = re.compile(r'https?://[A-Za-z0-9._-]+(:[0-9]+)?')

with open(os.path.join(ROOT, 'crates/core/tests/corpus/manifest.json'), encoding='utf8') as f:
    MANIFEST = json.load(f)


def collection_root(entry):
    base = os.path.join(ROOT, 'target', 'corpus', entry['id'])
    return base if entry['path'] == '.' else os.path.join(base, entry['path'])


def prepare(entry, dest):
    shutil.copytree(collection_root(entry), dest, symlinks=True)
    for directory, _, files in os.walk(dest):
        for name in files:
            if not name.endswith(('.yml', '.yaml', '.js')):
                continue
            path = os.path.join(directory, name)
            with open(path, encoding='utf8', errors='surrogateescape') as f:
                text = f.read()
            rewritten = HOSTS.sub(BASE, text)
            if rewritten != text:
                with open(path, 'w', encoding='utf8', errors='surrogateescape') as f:
                    f.write(rewritten)


def pick_env(dest):
    directory = os.path.join(dest, 'environments')
    if not os.path.isdir(directory):
        return None
    names = sorted(f[:-4] for f in os.listdir(directory) if f.endswith('.yml'))
    with open(os.path.join(dest, 'opencollection.yml'), encoding='utf8') as f:
        preset = re.search(r'defaultEnvironment:\s*(.+)', f.read())
    if preset and preset.group(1).strip().strip('"\'') in names:
        return preset.group(1).strip().strip('"\'')
    return names[0] if names else None


def normalize(report):
    if isinstance(report, dict):
        report = [report]
    out = []
    for iteration in report:
        for r in iteration['results']:
            out.append({
                'path': r.get('path') or r['test']['filename'],
                'status': r['status'],
                'http': r['response'].get('status'),
                'assertions': [(a.get('lhsExpr'), a.get('rhsExpr'), a['status']) for a in r.get('assertionResults', [])],
                'tests': [(t['description'], t['status']) for t in r.get('testResults', [])],
                'pre': [(t['description'], t['status']) for t in r.get('preRequestTestResults', [])],
                'post': [(t['description'], t['status']) for t in r.get('postResponseTestResults', [])],
                'error': r.get('error'),
            })
    return out


def run(command, cwd):
    try:
        return subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=TIMEOUT)
    except subprocess.TimeoutExpired:
        class Timeout:
            returncode = -9
            stdout = ''
            stderr = 'TIMEOUT'
        return Timeout()


def compare(entry, show):
    tmp = tempfile.mkdtemp(prefix='gate3-')
    try:
        dest = os.path.join(tmp, 'c')
        prepare(entry, dest)
        env = pick_env(dest)
        env_args = ['--env', env] if env else []
        bru_json, xc_json = os.path.join(tmp, 'bru.json'), os.path.join(tmp, 'xc.json')
        bru = run([BRU, 'run', '--reporter-json', bru_json] + env_args, dest)
        xc = run([XC, 'run', dest, '--reporter-json', xc_json] + env_args, tmp)
        try:
            with open(bru_json, encoding='utf8') as f:
                expected = normalize(json.load(f))
            with open(xc_json, encoding='utf8') as f:
                actual = normalize(json.load(f))
        except Exception as e:
            print(f"{entry['id']}: rapport illisible ({e}) bru={bru.returncode} xc={xc.returncode}")
            print(bru.stderr[-300:])
            print(xc.stderr[-300:])
            return
        by_bru = {r['path']: r for r in expected}
        by_xc = {r['path']: r for r in actual}
        different = [p for p in sorted(set(by_bru) | set(by_xc)) if by_bru.get(p) != by_xc.get(p)]
        print(f"{entry['id']:32} env={env} bru={len(expected)} xc={len(actual)} diff={len(different)}")
        for path in different[:show]:
            print('  ', path)
            for key in ['status', 'http', 'assertions', 'tests', 'pre', 'post', 'error']:
                b, x = (by_bru.get(path) or {}).get(key), (by_xc.get(path) or {}).get(key)
                if b != x:
                    print('     ', key, '\n        bru:', str(b)[:300], '\n        xc :', str(x)[:300])
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main(ids):
    server = subprocess.Popen(['node', os.path.join(HERE, 'echo-server.js'), str(PORT)], stdout=subprocess.DEVNULL)
    try:
        show = int(os.environ.get('SHOW', '3'))
        for entry in MANIFEST:
            if not ids or entry['id'] in ids:
                compare(entry, show)
    finally:
        server.terminate()


if __name__ == '__main__':
    main(sys.argv[1:])
