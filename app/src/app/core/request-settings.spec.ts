import assert from 'node:assert/strict';
import { test } from 'node:test';

import { effectiveSettings, wholeNumber, withSettings } from './request-settings.ts';
import type { RequestDoc } from './model.ts';

const doc = (extra: Partial<RequestDoc> = {}): RequestDoc => ({
  name: 'R',
  requestType: 'http',
  method: 'GET',
  url: 'https://x.test',
  params: [],
  headers: [],
  body: { type: 'none' },
  auth: { type: 'inherit' },
  assertions: [],
  variables: [],
  scripts: [],
  ...extra,
});

test('ef_req_04_sans_reglage_la_requete_suit_les_defauts_de_bruno', () => {
  assert.deepEqual(effectiveSettings(doc()), { timeoutMs: 0, followRedirects: true, maxRedirects: 5, forwardAuthorizationHeader: true });
  const set = doc({ timeoutMs: 2000, followRedirects: false, maxRedirects: 0, forwardAuthorizationHeader: false });
  assert.deepEqual(effectiveSettings(set), { timeoutMs: 2000, followRedirects: false, maxRedirects: 0, forwardAuthorizationHeader: false });
});

test('ef_req_04_un_nombre_saisi_doit_etre_un_entier_positif_ou_nul', () => {
  assert.equal(wholeNumber(' 12 '), 12);
  assert.equal(wholeNumber('0'), 0);
  for (const bad of ['', '-1', '1.5', 'abc', '1e3', '1234567890']) assert.equal(wholeNumber(bad), null, bad);
});

test('ef_req_04_un_delai_de_zero_est_ecrit_comme_une_absence_de_delai', () => {
  assert.equal(withSettings(doc({ timeoutMs: 500 }), { timeoutMs: 0 }).timeoutMs, null);
  assert.equal(withSettings(doc(), { timeoutMs: 800 }).timeoutMs, 800);
  assert.equal(withSettings(doc(), { followRedirects: false }).followRedirects, false);
});
