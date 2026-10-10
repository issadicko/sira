import assert from 'node:assert/strict';
import { test } from 'node:test';

import { branchSummary, collapse, commitBlocker, conflicts, diffRows, fileName, pullBlocker, touchedLines } from './git.ts';
import type { GitFile, GitState } from './model.ts';

const file = (path: string, state: GitFile['state'] = 'M'): GitFile => ({ path, state, staged: false, untracked: false, from: null });
const state = (extra: Partial<GitState> = {}): GitState => ({ repo: true, branch: 'main', upstream: 'origin/main', ahead: 0, behind: 0, unborn: false, files: [], ...extra });

test('ef_git_01_deux_textes_identiques_ne_donnent_que_du_contexte', () => {
  const rows = diffRows('a\nb\n', 'a\nb\n');
  assert.deepEqual(rows.map((r) => r.kind), ['ctx', 'ctx']);
  assert.equal(touchedLines(rows), 0);
});

test('ef_git_01_une_ligne_changee_tient_sur_une_rangee_avec_ses_deux_numeros', () => {
  const rows = diffRows('a\nb\nc\n', 'a\nB\nc\n');
  assert.deepEqual(rows.map((r) => r.kind), ['ctx', 'chg', 'ctx']);
  assert.deepEqual([rows[1]!.l, rows[1]!.r], [{ no: 2, text: 'b' }, { no: 2, text: 'B' }]);
});

test('ef_git_01_les_lignes_ajoutees_et_supprimees_restent_alignees_avec_un_cote_vide', () => {
  const rows = diffRows('a\nb\n', 'a\nx\ny\nb\n');
  assert.deepEqual(rows.map((r) => r.kind), ['ctx', 'add', 'add', 'ctx']);
  assert.deepEqual(rows.map((r) => r.l?.no ?? null), [1, null, null, 2]);
  assert.deepEqual(rows.map((r) => r.r?.no ?? null), [1, 2, 3, 4]);
  const removed = diffRows('a\nb\nc\n', 'a\nc\n');
  assert.deepEqual(removed.map((r) => r.kind), ['ctx', 'del', 'ctx']);
  assert.equal(removed[1]!.r, null);
});

test('ef_git_01_un_fichier_nouveau_ou_supprime_est_tout_ajoute_ou_tout_supprime', () => {
  assert.deepEqual(diffRows(null, 'a\nb\n').map((r) => r.kind), ['add', 'add']);
  assert.deepEqual(diffRows('a\nb\n', null).map((r) => r.kind), ['del', 'del']);
  assert.deepEqual(diffRows(null, null), []);
  assert.deepEqual(diffRows('', 'a').map((r) => r.kind), ['add']);
});

test('ef_git_01_un_tres_gros_fichier_est_montre_comme_remplace_sans_calcul_couteux', () => {
  const many = Array.from({ length: 3500 }, (_, i) => String(i)).join('\n');
  const rows = diffRows(many, `${many}\nfin`);
  assert.equal(rows.length, 3501);
  assert.ok(rows.every((r) => r.kind !== 'ctx'));
});

test('ef_git_01_les_lignes_identiques_loin_des_changements_sont_repliees', () => {
  const head = Array.from({ length: 20 }, (_, i) => `l${i}`).join('\n');
  const work = head.replace('l10', 'L10');
  const shown = collapse(diffRows(head, work), 2);
  assert.equal(shown[0]!.hidden, 8);
  assert.equal(shown.at(-1)!.hidden, 7);
  assert.equal(shown.filter((s) => s.row).length, 5);
  assert.deepEqual(collapse(diffRows('a', 'a')).map((s) => s.hidden), [1]);
});

test('ef_git_01_le_resume_de_la_branche_dit_ce_qui_reste_a_pousser_ou_a_recuperer', () => {
  assert.equal(branchSummary(state()), 'à jour avec origin/main');
  assert.equal(branchSummary(state({ ahead: 2 })), '↑2 à pousser');
  assert.equal(branchSummary(state({ ahead: 1, behind: 3 })), '↑1 à pousser · ↓3 à récupérer');
  assert.equal(branchSummary(state({ upstream: null })), 'pas de branche amont');
  assert.equal(branchSummary(state({ upstream: null, branch: null })), 'tête détachée');
  assert.equal(branchSummary(state({ unborn: true })), 'aucun commit');
});

test('ef_git_01_valider_demande_des_modifications_un_message_et_aucun_conflit', () => {
  assert.equal(commitBlocker(state(), 'x'), 'Aucune modification à valider.');
  assert.equal(commitBlocker(state({ files: [file('a.yml')] }), '  '), 'Écris un message avant de valider.');
  assert.equal(commitBlocker(state({ files: [file('a.yml')] }), 'ok'), null);
  const conflicted = state({ files: [file('a.yml', 'U')] });
  assert.equal(conflicts(conflicted), 1);
  assert.equal(commitBlocker(conflicted, 'ok'), 'Résous les conflits avant de valider.');
});

test('ef_git_01_recuperer_demande_une_branche_amont', () => {
  assert.equal(pullBlocker(state()), null);
  assert.match(pullBlocker(state({ upstream: null }))!, /branche amont/);
});

test('ef_git_01_un_chemin_se_coupe_en_nom_et_dossier', () => {
  assert.deepEqual(fileName('a/b/c.yml'), { name: 'c.yml', dir: 'a/b' });
  assert.deepEqual(fileName('c.yml'), { name: 'c.yml', dir: '' });
});
