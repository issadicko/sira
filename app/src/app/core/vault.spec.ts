import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { VaultPrefs } from './model.ts';
import { sameVault, ttlLabel, vaultProblem, vaultVariable } from './vault.ts';

const prefs = (extra: Partial<VaultPrefs> = {}): VaultPrefs => ({ addr: '', namespace: '', auth: 'token', roleId: '', approleMount: '', ...extra });

test('ef_var_03_une_adresse_vide_ou_en_http_ou_https_convient', () => {
  assert.equal(vaultProblem(prefs()), null);
  assert.equal(vaultProblem(prefs({ addr: 'https://vault.test:8200' })), null);
  assert.equal(vaultProblem(prefs({ addr: ' http://127.0.0.1:8200 ' })), null);
});

test('ef_var_03_une_adresse_sans_schema_est_refusee_en_le_disant', () => {
  assert.equal(vaultProblem(prefs({ addr: 'vault.test:8200' })), "L'adresse de Vault commence par http:// ou https://.");
  assert.notEqual(vaultProblem(prefs({ addr: 'ftp://x' })), null);
});

test('ef_var_03_la_variable_d_un_champ_se_compose_avec_montage_chemin_et_champ', () => {
  assert.equal(vaultVariable('secret/app/db', 'password'), '{{$vault.secret/app/db#password}}');
  assert.equal(vaultVariable('/secret/app/', ' cle '), '{{$vault.secret/app#cle}}');
});

test('ef_var_03_la_duree_de_vie_d_un_jeton_se_lit_en_secondes_minutes_heures_ou_jours', () => {
  assert.equal(ttlLabel(null), 'sans expiration');
  assert.equal(ttlLabel(45), '45 s');
  assert.equal(ttlLabel(3600), '60 min');
  assert.equal(ttlLabel(7200), '2 h');
  assert.equal(ttlLabel(3 * 86400), '3 j');
});

test('ef_var_03_deux_reglages_identiques_sont_reconnus', () => {
  assert.equal(sameVault(prefs({ addr: 'https://a.test' }), prefs({ addr: 'https://a.test' })), true);
  assert.equal(sameVault(prefs(), prefs({ auth: 'approle' })), false);
});
