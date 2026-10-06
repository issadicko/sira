import assert from 'node:assert/strict';
import { test } from 'node:test';

import { draftProblem, emptyDraft, expiryLabel, groupByDomain, keyOf, shortValue } from './cookies.ts';
import type { CookieView } from './model.ts';

const cookie = (key: string, domain: string, path = '/'): CookieView => ({
  key,
  value: 'v',
  domain,
  path,
  secure: false,
  httpOnly: false,
  hostOnly: true,
  expires: null,
});

test('ef_ux_02_les_cookies_sont_regroupes_par_domaine_puis_tries_par_chemin_et_nom', () => {
  const groups = groupByDomain([cookie('b', 'z.test'), cookie('z', 'a.test', '/x'), cookie('a', 'a.test', '/x'), cookie('m', 'a.test', '/')]);
  assert.deepEqual(groups.map((g) => g.domain), ['a.test', 'z.test']);
  assert.deepEqual(groups[0].cookies.map((c) => `${c.path}${c.key}`), ['/m', '/xa', '/xz']);
  assert.deepEqual(groupByDomain([]), []);
});

test('ef_ux_02_un_cookie_se_designe_par_son_domaine_son_chemin_et_son_nom', () => {
  assert.deepEqual(keyOf(cookie('sid', 'a.test', '/app')), { domain: 'a.test', path: '/app', key: 'sid' });
});

test('ef_ux_02_un_nouveau_cookie_part_de_la_racine_et_ne_vaut_que_pour_son_hote', () => {
  assert.deepEqual(emptyDraft('a.test'), { key: '', value: '', domain: 'a.test', path: '/', secure: false, httpOnly: false, hostOnly: true, expires: null });
});

test('ef_ux_02_un_cookie_demande_un_nom_un_domaine_et_une_expiration_lisible', () => {
  const ok = { ...emptyDraft('a.test'), key: 'sid' };
  assert.equal(draftProblem(ok), null);
  assert.match(draftProblem({ ...ok, key: '  ' }) ?? '', /nom/);
  assert.match(draftProblem({ ...ok, domain: '' }) ?? '', /domaine/);
  assert.equal(draftProblem({ ...ok, expires: '2026-12-31T23:59:59Z' }), null);
  assert.equal(draftProblem({ ...ok, expires: '2026-12-31T23:59:59.250Z' }), null);
  assert.equal(draftProblem({ ...ok, expires: '  ' }), null, 'une date blanche vaut un cookie de session');
  for (const bad of ['demain', '2026-12-31', '2026-12-31T23:59:59', '2026-13-45T00:00:00Z']) {
    assert.match(draftProblem({ ...ok, expires: bad }) ?? '', /expiration/, bad);
  }
});

test('ef_ux_02_lexpiration_se_lit_en_duree_restante', () => {
  const now = Date.parse('2026-10-06T12:00:00Z');
  const at = (ms: number) => new Date(now + ms).toISOString();
  assert.equal(expiryLabel(null, now), 'Session');
  assert.equal(expiryLabel(at(-1000), now), 'Expiré');
  assert.equal(expiryLabel(at(30_000), now), '30 s');
  assert.equal(expiryLabel(at(5 * 60_000), now), '5 min');
  assert.equal(expiryLabel(at(3 * 3_600_000), now), '3 h');
  assert.equal(expiryLabel(at(2 * 86_400_000), now), '2 j');
  assert.equal(expiryLabel(at(89 * 86_400_000), now), '89 j');
  assert.match(expiryLabel('2099-01-01T00:00:00.000Z', now), /2099/, 'au-delà de trois mois : la date');
  assert.equal(expiryLabel('pas une date', now), '—');
});

test('ef_ux_02_une_valeur_longue_est_coupee_avec_une_ellipse', () => {
  assert.equal(shortValue('abc', 5), 'abc');
  assert.equal(shortValue('abcdefghij', 5), 'abcd…');
});
