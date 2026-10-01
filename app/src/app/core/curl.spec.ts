import assert from 'node:assert/strict';
import { test } from 'node:test';

import { describeCurl, isCurlCommand, nameFromUrl, withCurl } from './curl.ts';
import type { RequestDoc } from './model.ts';
import { withUrl } from './url.ts';

function doc(extra: Partial<RequestDoc> = {}): RequestDoc {
  return {
    name: 'Détail',
    requestType: 'http',
    seq: 1,
    method: 'GET',
    url: 'https://old.test/a',
    params: [],
    headers: [],
    body: { type: 'none' },
    auth: { type: 'inherit' },
    assertions: [],
    variables: [],
    scripts: [],
    ...extra,
  };
}

function paste(current: RequestDoc, curl: RequestDoc): RequestDoc {
  return withCurl(withUrl(current, curl.url), curl);
}

test('ef_imp_01_detecte_une_commande_curl_comme_bruno', () => {
  assert.ok(isCurlCommand('curl https://api.exemple.test'));
  assert.ok(isCurlCommand('  \n CURL -X POST https://api.exemple.test'));
  assert.ok(isCurlCommand('curl\thttps://api.exemple.test'));
});

test('ef_imp_01_ignore_ce_qui_nest_pas_une_commande_curl', () => {
  for (const text of ['', 'curl', 'curl.exe', 'curly https://x', 'echo curl https://x', 'https://curl.se', '{{baseUrl}}/curl ']) {
    assert.equal(isCurlCommand(text), false, text);
  }
});

test('ef_imp_01_coller_remplace_url_et_methode_et_resynchronise_les_parametres', () => {
  const current = doc({
    url: 'https://old.test/:id?page=1',
    params: [
      { name: 'page', value: '1', kind: 'query', enabled: true },
      { name: 'debug', value: 'oui', kind: 'query', enabled: false },
      { name: 'id', value: '42', kind: 'path', enabled: true },
    ],
  });
  const result = paste(current, doc({ method: 'post', url: 'https://api.test/clients/:id?statut=actif&tri=nom' }));
  assert.equal(result.method, 'POST');
  assert.equal(result.url, 'https://api.test/clients/:id?statut=actif&tri=nom');
  assert.deepEqual(
    result.params.map((p) => [p.kind, p.name, p.value, p.enabled]),
    [
      ['query', 'statut', 'actif', true],
      ['query', 'tri', 'nom', true],
      ['query', 'debug', 'oui', false],
      ['path', 'id', '42', true],
    ],
  );
});

test('ef_imp_01_coller_remplace_les_en_tetes_seulement_sil_y_en_a', () => {
  const current = doc({ headers: [{ name: 'X-Canal', value: 'USSD', enabled: true }] });
  assert.deepEqual(paste(current, doc()).headers, current.headers);
  const accept = [{ name: 'Accept', value: 'application/json', enabled: true }];
  assert.deepEqual(paste(current, doc({ headers: accept })).headers, accept);
});

test('ef_imp_01_coller_applique_le_corps_seulement_sil_est_present', () => {
  const current = doc({ body: { type: 'text', data: 'avant' } });
  assert.deepEqual(paste(current, doc()).body, current.body);
  const json = { type: 'json' as const, data: '{\n  "montant": 1500\n}' };
  assert.deepEqual(paste(current, doc({ body: json })).body, json);
  const form = { type: 'form-urlencoded' as const, fields: [{ name: 'a', value: '1', enabled: true }] };
  assert.deepEqual(paste(current, doc({ body: form })).body, form);
});

test('ef_imp_01_coller_applique_lauth_seulement_si_le_curl_en_porte_une', () => {
  const current = doc({ auth: { type: 'bearer', token: '{{token}}' } });
  assert.deepEqual(paste(current, doc({ auth: { type: 'inherit' } })).auth, current.auth);
  assert.deepEqual(paste(current, doc({ auth: { type: 'none' } })).auth, current.auth);
  const basic = { type: 'basic' as const, username: 'issa', password: 'secret' };
  assert.deepEqual(paste(current, doc({ auth: basic })).auth, basic);
});

test('ef_imp_01_coller_garde_le_nom_les_assertions_et_la_documentation', () => {
  const current = doc({ docs: 'Notes', assertions: [{ expression: 'res.status', operator: 'eq', value: '200', enabled: true }] });
  const result = paste(current, doc({ name: 'Autre', method: 'DELETE', url: 'https://x.test' }));
  assert.equal(result.name, 'Détail');
  assert.equal(result.docs, 'Notes');
  assert.deepEqual(result.assertions, current.assertions);
});

test('ef_imp_01_decrit_ce_que_reprend_le_curl', () => {
  assert.equal(describeCurl(doc()), 'ni en-tête, ni corps');
  const headers = [{ name: 'A', value: '1', enabled: true }];
  assert.equal(describeCurl(doc({ headers })), '1 en-tête');
  assert.equal(
    describeCurl(doc({ headers: [...headers, ...headers], body: { type: 'json', data: '{}' }, auth: { type: 'basic', username: '', password: '' } })),
    '2 en-têtes · corps JSON · auth basic',
  );
});

test('ef_imp_01_nom_propose_depuis_les_deux_derniers_segments_utiles', () => {
  assert.equal(nameFromUrl('https://api.exemple.test/v1/transactions/42/annuler?x=1'), 'Transactions annuler');
  assert.equal(nameFromUrl('{{baseUrl}}/transactions/:id/annuler'), 'Transactions annuler');
  assert.equal(nameFromUrl('https://httpbin.org/anything/export-csv'), 'Anything export csv');
  assert.equal(nameFromUrl('https://api.exemple.test/clients/{id}/cartes_bancaires'), 'Clients cartes bancaires');
});

test('ef_imp_01_nom_propose_retombe_sur_lhote_puis_sur_un_nom_par_defaut', () => {
  assert.equal(nameFromUrl('https://api.exemple.test/'), 'api.exemple.test');
  assert.equal(nameFromUrl('http://localhost:3000'), 'localhost:3000');
  assert.equal(nameFromUrl('https://api.exemple.test/v2'), 'api.exemple.test');
  assert.equal(nameFromUrl('{{baseUrl}}'), 'Nouvelle requête');
  assert.equal(nameFromUrl(''), 'Nouvelle requête');
});
