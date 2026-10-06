import assert from 'node:assert/strict';
import { test } from 'node:test';

import { describeEntry, forExistingRequests, statusLabel, whenLabel } from './history.ts';
import type { HistoryEntry, TreeItem } from './model.ts';

const entry = (path: string, extra: Partial<HistoryEntry> = {}): HistoryEntry => ({
  path,
  name: path,
  method: 'GET',
  url: '{{baseUrl}}/users',
  env: 'dev',
  status: 200,
  error: null,
  durationMs: 12.4,
  size: 321,
  at: '2026-10-06T12:30:00.000Z',
  ...extra,
});

const request = (path: string): TreeItem => ({ kind: 'request', path, name: path, method: 'GET', requestType: 'http', url: '', deprecated: false });

test('ef_ux_02_l_historique_ne_garde_que_les_requetes_qui_existent_encore', () => {
  const tree: TreeItem[] = [request('a.yml'), { kind: 'folder', path: 'users', name: 'users', children: [request('users/b.yml')] }];
  const entries = [entry('a.yml'), entry('supprimee.yml'), entry('users/b.yml'), entry('users/ancien.yml')];
  assert.deepEqual(forExistingRequests(entries, tree).map((e) => e.path), ['a.yml', 'users/b.yml']);
});

test('ef_ux_02_une_entree_du_jour_montre_l_heure_une_plus_ancienne_le_jour', () => {
  const now = new Date('2026-10-06T15:00:00');
  assert.match(whenLabel(new Date('2026-10-06T09:05:00').toISOString(), now), /^09:05$/);
  assert.match(whenLabel(new Date('2026-10-01T09:05:00').toISOString(), now), /^1(er)? oct/);
  assert.equal(whenLabel('pas une date', now), '');
});

test('ef_ux_02_sans_reponse_la_ligne_affiche_err_et_l_infobulle_dit_pourquoi', () => {
  assert.equal(statusLabel(entry('a.yml')), '200');
  const failed = entry('a.yml', { status: null, error: 'connexion refusée', durationMs: 0, size: 0 });
  assert.equal(statusLabel(failed), 'ERR');
  assert.equal(describeEntry(failed), 'GET {{baseUrl}}/users\nEnvironnement : dev\nconnexion refusée');
  assert.equal(describeEntry(entry('a.yml')), 'GET {{baseUrl}}/users\nEnvironnement : dev\n12 ms · 321 o');
});
