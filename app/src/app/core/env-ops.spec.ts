import assert from 'node:assert/strict';
import { test } from 'node:test';

import { reconciled, sameVars, varsKey } from './disk-sync.ts';
import { blankVar, varProblems } from './env-ops.ts';
import type { EnvVar } from './model.ts';
import { cloneName, envFileName, envNameProblem, validateName } from './tree-ops.ts';

const v = (name: string, value: string | null = '', extra: Partial<EnvVar> = {}): EnvVar => ({
  name,
  value,
  secret: false,
  enabled: true,
  description: null,
  dataType: null,
  ...extra,
});

test('ef_var_01_une_valeur_absente_et_une_valeur_vide_sont_la_meme_variable', () => {
  assert.ok(sameVars([v('a', null)], [v('a', '')]));
  assert.ok(sameVars([v('a', 'x', { description: '  ' })], [v('a', 'x', { description: null })]));
  assert.ok(sameVars([{ name: 'a', secret: false, enabled: true }], [v('a', '')]));
  assert.ok(!sameVars([v('a', 'x')], [v('a', 'y')]));
  assert.ok(!sameVars([v('a')], [v('a', '', { enabled: false })]));
  assert.ok(!sameVars([v('a', '8080')], [v('a', '8080', { dataType: 'number' })]));
});

test('enf_sec_01_la_valeur_d_un_secret_ne_compte_jamais', () => {
  assert.ok(sameVars([v('t', 'x', { secret: true })], [v('t', null, { secret: true })]));
  assert.equal(varsKey([v('t', 'x', { secret: true })]), varsKey([v('t', null, { secret: true })]));
});

test('ef_var_01_une_variable_neuve_est_vide_activee_et_sans_type', () => {
  assert.deepEqual(blankVar('baseUrl'), v('baseUrl'));
  assert.equal(blankVar().name, '');
});

test('ef_var_01_chaque_ligne_dit_ce_qui_empeche_d_enregistrer', () => {
  const rows = [v('baseUrl'), v(''), v('baseUrl'), v('a b'), v('ok_1.2-x')];
  const problems = varProblems(rows, []);
  assert.equal(problems[0], null);
  assert.equal(problems[1], 'Donne un nom à la variable.');
  assert.equal(problems[2], 'Cette variable est déjà définie plus haut.');
  assert.equal(problems[3], 'Lettres, chiffres, « _ », « - » et « . » seulement.');
  assert.equal(problems[4], null);
});

test('ef_var_01_un_nom_ecrit_a_la_main_dans_le_fichier_n_empeche_pas_d_enregistrer_une_autre_variable', () => {
  assert.deepEqual(varProblems([v('nom avec espace'), v('autre')], ['nom avec espace']), [null, null]);
  assert.deepEqual(varProblems([v('nom avec espace'), v('x y')], ['nom avec espace']), [null, 'Lettres, chiffres, « _ », « - » et « . » seulement.']);
});

test('ef_var_01_un_doublon_que_le_fichier_contient_deja_n_empeche_pas_d_enregistrer_un_nouveau_doublon_si', () => {
  const rows = [v('host'), v('host', 'x', { enabled: false }), v('port')];
  assert.deepEqual(varProblems(rows, ['host', 'host', 'port']), [null, null, null]);
  assert.deepEqual(varProblems([...rows, v('host')], ['host', 'host', 'port']), [null, null, null, 'Cette variable est déjà définie plus haut.']);
  assert.deepEqual(varProblems([v('port'), v('port')], ['port']), [null, 'Cette variable est déjà définie plus haut.']);
});

test('ef_col_04_l_apercu_du_nom_de_fichier_suit_le_moteur', () => {
  assert.equal(envFileName('a.yml', []), 'a', 'le moteur retire .yml, qui serait sinon doublé');
  assert.equal(envFileName('dev.yml', ['dev']), 'dev 1');
  assert.ok(envNameProblem('.dev'), 'un point en tête cacherait le fichier');
  assert.equal(envNameProblem('dev'), null);
  assert.equal(envNameProblem('Prod/EU'), null);
});

test('ef_col_04_le_nom_de_fichier_d_un_environnement_est_assaini_et_ne_remplace_jamais_un_autre', () => {
  assert.equal(envFileName('Prod/EU', []), 'Prod-EU');
  assert.equal(envFileName('dev', ['dev']), 'dev 1');
  assert.equal(envFileName('DEV', ['dev', 'dev 1']), 'DEV 2', 'la casse ne distingue pas deux noms');
  assert.equal(envFileName('dev', ['dev', 'prod'], 'dev'), 'dev', 'renommer en son propre nom ne change rien');
  assert.equal(envFileName('Dev', ['dev'], 'dev'), 'Dev', 'un changement de casse seul est permis');
});

test('ef_col_04_les_noms_d_environnement_refuses_le_sont_comme_les_noms_de_requete', () => {
  assert.equal(validateName('', 'request'), 'Donne un nom.');
  assert.ok(validateName('con', 'request'));
  assert.ok(validateName('-dev', 'request'));
  assert.equal(validateName('Recette 2', 'request'), null);
  assert.equal(cloneName('dev'), 'dev copie');
});

test('ef_col_03_un_brouillon_intact_adopte_le_fichier_qui_a_change', () => {
  const state = { base: [v('a', '1')], draft: [v('a', '1')], stale: false };
  const next = reconciled(state, [v('a', '2')]);
  assert.deepEqual(next.draft, [v('a', '2')]);
  assert.deepEqual(next.base, [v('a', '2')]);
  assert.equal(next.stale, false);
});

test('ef_col_03_un_brouillon_modifie_devient_perime_et_garde_ses_modifications', () => {
  const state = { base: [v('a', '1')], draft: [v('a', 'brouillon')], stale: false };
  const next = reconciled(state, [v('a', '2')]);
  assert.deepEqual(next.draft, [v('a', 'brouillon')]);
  assert.deepEqual(next.base, [v('a', '1')]);
  assert.equal(next.stale, true);
});

test('ef_col_03_notre_propre_enregistrement_n_est_pas_un_changement_exterieur', () => {
  const written = [v('a', 'brouillon')];
  const next = reconciled({ base: [v('a', '1')], draft: [v('a', 'brouillon')], stale: false }, written);
  assert.equal(next.stale, false);
  assert.deepEqual(next.base, written);
  const after = reconciled({ base: written, draft: written, stale: false }, written);
  assert.equal(after.stale, false, 'la relecture qui suit notre enregistrement ne change rien');
  const same = reconciled({ base: [v('a', '1')], draft: [v('a', 'brouillon')], stale: false }, [v('a', '1')]);
  assert.equal(same.stale, false, 'le disque n\'a pas changé : rien à signaler');
  assert.deepEqual(same.draft, [v('a', 'brouillon')]);
});

test('ef_col_03_le_fichier_qui_revient_a_la_version_ouverte_retire_l_etat_perime', () => {
  const state = { base: [v('a', '1')], draft: [v('a', 'brouillon')], stale: true };
  assert.equal(reconciled(state, [v('a', '1')]).stale, false);
});

test('ef_col_03_un_collegue_qui_fait_la_meme_modification_n_est_pas_un_conflit', () => {
  const next = reconciled({ base: [v('a', '1')], draft: [v('a', '2')], stale: false }, [v('a', '2', { description: null })]);
  assert.equal(next.stale, false);
  assert.deepEqual(next.base, [v('a', '2')]);
});

test('ef_col_03_la_comparaison_du_disque_ignore_la_forme_que_rust_donne_aux_valeurs_vides', () => {
  const rust = [{ name: 'a', value: null, secret: false, enabled: true, description: null, dataType: null }];
  const edited = [v('a', '')];
  const next = reconciled({ base: rust, draft: edited, stale: false }, rust);
  assert.equal(next.stale, false);
  assert.deepEqual(next.draft, edited);
});
