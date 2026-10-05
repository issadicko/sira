import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { ScriptsReport } from './model.ts';
import { emptyReport, expectation, formatLog, hasLogs, scriptCode, tally, withScript } from './scripts.ts';

test('ef_scr_01 un script ajouté prend sa place dans l\'ordre du fichier', () => {
  const tests = withScript([], 'tests', 'test("a", () => {});');
  const both = withScript(tests, 'before-request', 'bru.setVar("a", 1);');
  assert.deepEqual(both.map((s) => s.kind), ['before-request', 'tests']);
  assert.equal(scriptCode(both, 'tests'), 'test("a", () => {});');
  assert.equal(scriptCode(both, 'after-response'), '');
});

test('ef_scr_01 modifier un script garde les autres et un code vide le retire', () => {
  const scripts = [
    { kind: 'before-request', code: 'a' },
    { kind: 'tests', code: 't' },
  ];
  assert.deepEqual(withScript(scripts, 'before-request', 'b'), [
    { kind: 'before-request', code: 'b' },
    { kind: 'tests', code: 't' },
  ]);
  assert.deepEqual(withScript(scripts, 'tests', '  \n'), [{ kind: 'before-request', code: 'a' }]);
});

test('ef_tst_01 les tests de script et les assertions se comptent ensemble', () => {
  const report: ScriptsReport = {
    ...emptyReport(),
    tests: { results: [{ description: 'a', status: 'pass' }, { description: 'b', status: 'fail', error: 'x' }], logs: [] },
  };
  const assertions = [{ expression: 'res.status', operator: 'eq', actual: '200', passed: true }];
  assert.deepEqual(tally(report, assertions), { passed: 2, total: 3 });
  assert.deepEqual(tally(emptyReport(), []), { passed: 0, total: 0 });
});

test('ef_scr_02 la console et les attentes se lisent comme à l\'écran de Bruno', () => {
  assert.equal(formatLog({ level: 'log', args: ['jeton', 7, { a: [1] }, null] }), 'jeton 7 {"a":[1]} null');
  assert.equal(hasLogs({ ...emptyReport(), post: { results: [], logs: [{ level: 'log', args: [] }] } }), true);
  assert.equal(hasLogs(emptyReport()), false);
  assert.equal(expectation({ actual: 200, expected: 404 }), 'attendu 404 · reçu 200');
  assert.equal(expectation({}), null);
});
