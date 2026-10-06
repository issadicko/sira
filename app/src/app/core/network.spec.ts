import assert from 'node:assert/strict';
import { test } from 'node:test';

import { DEFAULT_NETWORK, fileName, portProblem, proxyProblem, sameNetwork, withProxy, withProxyMode } from './network.ts';

test('ef_req_04_un_port_est_vide_ou_va_de_1_a_65535', () => {
  for (const ok of ['', '  ', '1', '8080', '65535']) assert.equal(portProblem(ok), null, ok);
  for (const bad of ['0', '65536', '-1', 'huit', '80.5', '1e3', '123456']) assert.ok(portProblem(bad), bad);
});

test('ef_req_04_un_proxy_manuel_demande_un_hote_et_un_port_valable', () => {
  const manual = withProxyMode(DEFAULT_NETWORK, 'manual');
  assert.match(proxyProblem(manual) ?? '', /nom d’hôte/);
  assert.equal(proxyProblem(withProxy(manual, { hostname: 'proxy.test', port: '3128' })), null);
  assert.match(proxyProblem(withProxy(manual, { hostname: 'proxy.test', port: '0' })) ?? '', /port/i);
});

test('ef_req_04_un_proxy_desactive_ou_systeme_nest_pas_verifie', () => {
  const broken = withProxy(DEFAULT_NETWORK, { hostname: '', port: 'x' });
  for (const mode of ['off', 'system'] as const) assert.equal(proxyProblem(withProxyMode(broken, mode)), null, mode);
});

test('ef_req_04_changer_de_mode_garde_la_configuration_du_proxy_manuel', () => {
  const configured = withProxy(withProxyMode(DEFAULT_NETWORK, 'manual'), { hostname: 'proxy.test', username: 'alice' });
  const back = withProxyMode(withProxyMode(configured, 'off'), 'manual');
  assert.deepEqual(back.proxy.config, { ...DEFAULT_NETWORK.proxy.config, hostname: 'proxy.test', username: 'alice' });
});

test('ef_req_04_deux_reglages_identiques_sont_reconnus_comme_tels', () => {
  assert.ok(sameNetwork(DEFAULT_NETWORK, structuredClone(DEFAULT_NETWORK)));
  assert.ok(!sameNetwork(DEFAULT_NETWORK, { ...DEFAULT_NETWORK, verifyTls: false }));
  assert.ok(!sameNetwork(DEFAULT_NETWORK, withProxy(DEFAULT_NETWORK, { bypassProxy: 'localhost' })));
});

test('ef_req_04_le_nom_du_fichier_dautorites_se_lit_sans_son_dossier', () => {
  assert.equal(fileName('/Users/demo/certificats/ca.pem'), 'ca.pem');
  assert.equal(fileName('C:\\certs\\ca.pem'), 'ca.pem');
  assert.equal(fileName('ca.pem'), 'ca.pem');
});
