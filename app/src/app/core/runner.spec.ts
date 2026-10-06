import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { RunEvent, RunResult, RunSkip, TreeItem } from './model.ts';
import { SKIP_LABELS, duration, filterResults, firstProblem, fraction, groupByIteration, haltText, idle, reduce, rowLabel, scopes, tallyResults } from './runner.ts';

const phase = () => ({ results: [], logs: [] });

function result(patch: Partial<RunResult> = {}): RunResult {
  return {
    iteration: 0,
    name: 'A',
    path: 'a.yml',
    method: 'GET',
    url: 'http://x/a',
    status: 'pass',
    skipped: null,
    http: { status: 200, reason: 'OK', size: 2, timeMs: 12 },
    error: null,
    assertions: [],
    scripts: { pre: phase(), post: phase(), tests: phase() },
    durationMs: 20,
    ...patch,
  };
}

const feed = (events: RunEvent[]) => events.reduce(reduce, idle());

test('ef_run_01 le suivi reprend les événements du run dans l\'ordre', () => {
  const p = feed([
    { kind: 'begin', runId: 'r', requests: 2, iterations: 1 },
    { kind: 'iteration', runId: 'r', index: 0, total: 1, row: null },
    { kind: 'started', runId: 'r', iteration: 0, path: 'a.yml', name: 'A', method: 'GET' },
  ]);
  assert.deepEqual(p.running, { path: 'a.yml', name: 'A', method: 'GET', iteration: 0 });
  assert.equal(p.requests, 2);
  const done = reduce(p, { kind: 'finished', runId: 'r', result: result() });
  assert.equal(done.running, null);
  assert.equal(done.results.length, 1);
  assert.equal(fraction(done), 0.5);
});

test('ef_run_01 un événement d\'un autre run est ignoré, un nouveau run repart de zéro', () => {
  const p = feed([{ kind: 'begin', runId: 'r', requests: 1, iterations: 1 }, { kind: 'finished', runId: 'r', result: result() }]);
  assert.equal(reduce(p, { kind: 'finished', runId: 'ancien', result: result() }), p);
  const next = reduce(p, { kind: 'begin', runId: 's', requests: 3, iterations: 2 });
  assert.deepEqual([next.runId, next.results.length, next.requests, next.iterations], ['s', 0, 3, 2]);
});

test('ef_run_01 l\'attente et les avertissements sont gardés, l\'attente cesse à la requête suivante', () => {
  const p = feed([
    { kind: 'begin', runId: 'r', requests: 2, iterations: 1 },
    { kind: 'waiting', runId: 'r', ms: 300 },
    { kind: 'warning', runId: 'r', message: 'Could not find request with name \'X\'' },
  ]);
  assert.equal(p.waitingMs, 300);
  assert.equal(p.warnings.length, 1);
  assert.equal(reduce(p, { kind: 'started', runId: 'r', iteration: 0, path: 'b.yml', name: 'B', method: 'GET' }).waitingMs, null);
});

test('ef_run_01 les compteurs séparent réussies, échecs, erreurs et ignorées, tests et assertions ensemble', () => {
  const failing = result({
    status: 'fail',
    assertions: [{ expression: 'res.status', operator: 'eq', expected: '200', actual: '500', passed: false }],
    scripts: { pre: phase(), post: phase(), tests: { results: [{ description: 'ok', status: 'pass' }, { description: 'ko', status: 'fail' }], logs: [] } },
  });
  const t = tallyResults([result(), failing, result({ status: 'error' }), result({ status: 'skipped', skipped: 'bail' })]);
  assert.deepEqual([t.passed, t.failed, t.errors, t.skipped], [1, 1, 1, 1]);
  assert.deepEqual(t.checks, { passed: 1, total: 3 });
});

test('ef_run_01 le filtre « problèmes » garde les échecs et les erreurs', () => {
  const all = [result(), result({ status: 'fail' }), result({ status: 'error' }), result({ status: 'skipped' })];
  assert.equal(filterResults(all, 'all').length, 4);
  assert.deepEqual(filterResults(all, 'problems').map((r) => r.status), ['fail', 'error']);
});

test('ef_run_02 les résultats sont rangés par itération avec leurs données', () => {
  const rows = [{ id: 1 }, { id: 'deux' }];
  const groups = groupByIteration([result({ iteration: 1 }), result({ iteration: 0 }), result({ iteration: 1, name: 'B' })], rows);
  assert.deepEqual(groups.map((g) => [g.index, g.results.length, g.row]), [[0, 1, { id: 1 }], [1, 2, { id: 'deux' }]]);
  assert.equal(rowLabel({ id: 7, name: 'ada', tags: ['a'] }), 'id=7, name=ada, tags=["a"]');
  assert.equal(rowLabel(null), '');
});

test('ef_run_01 la progression plafonne à 1 même avec des sauts', () => {
  const p = feed([{ kind: 'begin', runId: 'r', requests: 1, iterations: 1 }, { kind: 'finished', runId: 'r', result: result() }, { kind: 'finished', runId: 'r', result: result() }]);
  assert.equal(fraction(p), 1);
  assert.equal(fraction(idle()), 0);
});

const tree: TreeItem[] = [
  { kind: 'request', path: 'root.yml', name: 'Root', method: 'GET', requestType: 'http', url: '', deprecated: false },
  {
    kind: 'folder',
    path: 'users',
    name: 'users',
    children: [
      { kind: 'request', path: 'users/a.yml', name: 'A', method: 'GET', requestType: 'http', url: '', deprecated: false },
      { kind: 'folder', path: 'users/admin', name: 'admin', children: [{ kind: 'request', path: 'users/admin/b.yml', name: 'B', method: 'GET', requestType: 'graphql', url: '', deprecated: false }] },
    ],
  },
  { kind: 'folder', path: 'vide', name: 'vide', children: [] },
  { kind: 'folder', path: 'flux', name: 'flux', children: [{ kind: 'request', path: 'flux/s.yml', name: 'S', method: 'GET', requestType: 'grpc', url: '', deprecated: false }] },
];

test('ef_run_01 les portées sont la collection puis les dossiers qui ont des requêtes HTTP ou GraphQL, pas gRPC', () => {
  const list = scopes(tree, 'Shop');
  assert.deepEqual(list.map((s) => [s.path, s.depth, s.requests]), [['', 0, 3], ['users', 1, 2], ['users/admin', 2, 1]]);
  assert.equal(list[0]?.name, 'Shop');
});

test('ef_run_01 le motif d\'un arrêt tient en une phrase', () => {
  assert.equal(haltText({ kind: 'bail', request: 'B', reason: 'test failure', remaining: 2 }), 'Arrêté au premier échec : un test a échoué dans « B », 2 requêtes ignorées.');
  assert.equal(haltText({ kind: 'bail', request: 'B', reason: 'request failure', remaining: 0 }), 'Arrêté au premier échec : la requête a échoué dans « B ».');
  assert.match(haltText({ kind: 'stopExecution', request: 'C', remaining: 1 }), /un script dans « C », 1 requête ignorée/);
  assert.match(haltText({ kind: 'loop' }), /boucle sans fin/);
  assert.equal(haltText({ kind: 'cancelled' }), 'Run annulé.');
});

test('ef_run_01 les durées se lisent d\'un coup d\'œil', () => {
  assert.equal(duration(820.4), '820 ms');
  assert.equal(duration(2450), '2,5 s');
  assert.equal(duration(65_000), '1 min 05 s');
});

test('ef_run_01 la première raison d\'un échec vient de l\'erreur, puis des assertions, puis des tests', () => {
  assert.equal(firstProblem(result()), null);
  assert.equal(firstProblem(result({ status: 'error', error: { stage: 'send', message: 'connexion refusée' } })), 'connexion refusée');
  const assertion = { expression: 'res.status', operator: 'eq', expected: '200', actual: '500', passed: false };
  assert.equal(firstProblem(result({ status: 'fail', assertions: [assertion] })), 'res.status eq 200 : reçu 500');
  assert.equal(firstProblem(result({ assertions: [{ ...assertion, error: 'expected 500 to equal 200' }] })), 'expected 500 to equal 200');
  const tests = { results: [{ description: 'ok', status: 'pass' }, { description: 'total', status: 'fail', error: 'expected 1 to equal 2' }], logs: [] };
  assert.equal(firstProblem(result({ status: 'fail', scripts: { pre: phase(), post: phase(), tests } })), 'total : expected 1 to equal 2');
  assert.equal(firstProblem(result({ scripts: { pre: phase(), post: { results: [], logs: [], error: 'boom' }, tests: phase() } })), 'boom');
});

test('ef_run_01 chaque motif d\'une requête ignorée a son libellé', () => {
  const reasons = ['script', 'bail', 'stopExecution', 'prompts', 'unreadable'] satisfies RunSkip[];
  for (const why of reasons) assert.ok(SKIP_LABELS[why], why);
  assert.match(SKIP_LABELS['prompts'] ?? '', /variables à saisir/);
});
