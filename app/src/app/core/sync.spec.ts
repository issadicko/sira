import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { Hunk, OpStatus, OpView, SyncChange, SyncDecisions, SyncOperation, SyncPlan } from './model.ts';
import {
  applyCount,
  applyLabel,
  choose,
  codeLanguage,
  conflictPaths,
  conflictRefs,
  conflictsLeft,
  editValue,
  firstSelection,
  foldLabel,
  foldRanges,
  groupOps,
  initialEditValue,
  isArbitrated,
  isUpToDate,
  isValidSelection,
  keepDecisions,
  lensesFor,
  lineCount,
  noDecisions,
  opState,
  paneMarks,
  pairingLabel,
  reportSummary,
  resultMarks,
  setRecreate,
  sourceName,
  specStatus,
  stepConflict,
  toggleSkip,
  touchedFiles,
  trimEnd,
  versionLabel,
} from './sync.ts';

const change = (id: string, kind: SyncChange['kind'], extra: Partial<SyncChange> = {}): SyncChange => ({
  id,
  field: 'body',
  label: `Champ ${id}`,
  reason: '',
  kind,
  base: 'b',
  ours: 'o',
  theirs: 't',
  result: 'o',
  choices: kind === 'conflict' ? ['team', 'spec', 'edit'] : [],
  ...extra,
});

const op = (key: string, status: OpStatus, changes: SyncChange[] = [], extra: Partial<SyncOperation> = {}): SyncOperation => ({
  key,
  name: key,
  method: 'GET',
  path: `/${key}`,
  file: `${key}.yml`,
  status,
  changes,
  ...extra,
});

const plan = (operations: SyncOperation[], extra: Partial<SyncPlan> = {}): SyncPlan => {
  const count = (status: OpStatus) => operations.filter((o) => o.status === status).length;
  return {
    id: 'p1',
    source: './spec/openapi.yml',
    groupBy: 'tags',
    hasBase: true,
    from: { title: 'API', version: '2.3.0' },
    to: { title: 'API', version: '2.4.0' },
    summary: {
      unchanged: count('unchanged'),
      updated: count('updated'),
      kept: count('kept'),
      merged: count('merged'),
      conflicts: count('conflict'),
      conflictFields: operations.flatMap((o) => o.changes).filter((c) => c.kind === 'conflict').length,
      created: count('new'),
      removed: count('removed'),
      restored: count('restored'),
      missing: count('missing'),
    },
    operations,
    suggestions: [],
    ...extra,
  };
};

const conflicts = () =>
  plan([
    op('a', 'conflict', [change('a::url', 'conflict'), change('a::method', 'applied')]),
    op('b', 'conflict', [change('b::body', 'conflict', { choices: ['team', 'spec', 'both', 'edit'] })]),
    op('c', 'updated', [change('c::header/accept', 'applied')]),
    op('d', 'kept'),
    op('e', 'new'),
    op('f', 'new'),
    op('g', 'removed'),
    op('h', 'missing'),
  ]);

const decide = (...entries: [string, SyncDecisions['choices'][string]][]): SyncDecisions => ({ ...noDecisions(), choices: Object.fromEntries(entries) });

test('ef_syn_04_conflits_restants_se_decomptent_par_champ_non_arbitre', () => {
  const p = conflicts();
  assert.deepEqual(conflictRefs(p), [
    { key: 'a', changeId: 'a::url' },
    { key: 'b', changeId: 'b::body' },
  ]);
  assert.equal(conflictsLeft(p, noDecisions()), 2);
  assert.equal(conflictsLeft(p, decide(['a::url', { choice: 'team' }])), 1);
  assert.equal(conflictsLeft(p, decide(['a::url', { choice: 'team' }], ['b::body', { choice: 'both' }])), 0);
  assert.equal(conflictsLeft(null, noDecisions()), 0);
});

test('ef_syn_04_choix_refait_sur_le_meme_bouton_annule_la_decision', () => {
  const once = choose(noDecisions(), 'a::url', 'spec');
  assert.deepEqual(once.choices, { 'a::url': { choice: 'spec' } });
  const other = choose(once, 'a::url', 'team');
  assert.deepEqual(other.choices, { 'a::url': { choice: 'team' } });
  assert.deepEqual(choose(other, 'a::url', 'team').choices, {});
  assert.deepEqual(once.choices, { 'a::url': { choice: 'spec' } });
});

test('ef_syn_04_edition_a_la_main_garde_la_valeur_saisie_meme_vide', () => {
  const edited = choose(noDecisions(), 'a::url', 'edit', '{{baseUrl}}/x');
  assert.deepEqual(edited.choices['a::url'], { choice: 'edit', value: '{{baseUrl}}/x' });
  assert.equal(isArbitrated(edited, 'a::url'), true);
  assert.deepEqual(editValue(edited, 'a::url', '').choices['a::url'], { choice: 'edit', value: '' });
  assert.equal(isArbitrated(editValue(edited, 'a::url', ''), 'a::url'), true);
  assert.equal(isArbitrated(decide(['a::url', { choice: 'edit' }]), 'a::url'), false);
  assert.equal(isArbitrated(noDecisions(), 'a::url'), false);
});

test('ef_syn_04_valeur_proposee_a_l_edition_prefere_le_resultat_puis_l_equipe', () => {
  assert.equal(initialEditValue(change('x', 'conflict', { result: 'r' })), 'r');
  assert.equal(initialEditValue(change('x', 'conflict', { result: null })), 'o');
  assert.equal(initialEditValue(change('x', 'conflict', { result: null, ours: null })), 't');
  assert.equal(initialEditValue(change('x', 'conflict', { result: null, ours: null, theirs: null })), '');
});

test('ef_syn_04_edition_d_un_corps_json_utilise_le_mode_json', () => {
  assert.equal(codeLanguage('{\n  "a": 1\n}'), 'json');
  assert.equal(codeLanguage('  [1, 2]'), 'json');
  assert.equal(codeLanguage('texte libre'), 'text');
  assert.equal(codeLanguage(''), 'text');
});

test('ef_syn_04_navigation_entre_conflits_boucle_dans_les_deux_sens', () => {
  const refs = conflictRefs(conflicts());
  assert.deepEqual(stepConflict(refs, { key: 'a', changeId: 'a::url' }, 1), refs[1]);
  assert.deepEqual(stepConflict(refs, { key: 'b', changeId: 'b::body' }, 1), refs[0]);
  assert.deepEqual(stepConflict(refs, { key: 'a', changeId: 'a::url' }, -1), refs[1]);
  assert.deepEqual(stepConflict(refs, null, 1), refs[0]);
  assert.deepEqual(stepConflict(refs, { key: 'c', changeId: null }, -1), refs[1]);
  assert.equal(stepConflict([], null, 1), null);
});

test('ef_syn_04_ouverture_selectionne_le_premier_conflit_non_arbitre', () => {
  const p = conflicts();
  assert.deepEqual(firstSelection(p, noDecisions()), { key: 'a', changeId: 'a::url' });
  assert.deepEqual(firstSelection(p, decide(['a::url', { choice: 'team' }])), { key: 'b', changeId: 'b::body' });
  assert.deepEqual(firstSelection(p, decide(['a::url', { choice: 'team' }], ['b::body', { choice: 'team' }])), { key: 'a', changeId: 'a::url' });
  assert.equal(firstSelection(plan([op('c', 'updated')]), noDecisions()), null);
});

test('ef_syn_04_selection_n_est_valide_que_si_l_operation_et_le_changement_existent', () => {
  const p = conflicts();
  assert.equal(isValidSelection(p, { key: 'a', changeId: 'a::url' }), true);
  assert.equal(isValidSelection(p, { key: 'd', changeId: null }), true);
  assert.equal(isValidSelection(p, { key: 'a', changeId: 'a::nope' }), false);
  assert.equal(isValidSelection(p, { key: 'zz', changeId: null }), false);
  assert.equal(isValidSelection(p, null), false);
});

test('ef_syn_04_relance_garde_les_decisions_dont_le_contenu_est_inchange', () => {
  const before = conflicts();
  const decisions: SyncDecisions = { choices: { 'a::url': { choice: 'spec' }, 'b::body': { choice: 'both' } }, skip: ['e', 'g'], recreate: ['h', 'e'] };
  const after = plan([
    op('a', 'conflict', [change('a::url', 'conflict')]),
    op('b', 'conflict', [change('b::body', 'conflict', { theirs: 'autre contenu', choices: ['team', 'spec', 'both', 'edit'] })]),
    op('e', 'new'),
    op('g', 'removed'),
    op('h', 'missing'),
  ]);
  assert.deepEqual(keepDecisions(before, after, decisions), { choices: { 'a::url': { choice: 'spec' } }, skip: ['e'], recreate: ['h'] });
});

test('ef_syn_04_relance_abandonne_un_choix_que_le_conflit_ne_propose_plus', () => {
  const before = plan([op('a', 'conflict', [change('a::url', 'conflict', { choices: ['team', 'spec', 'both'] })])]);
  const after = plan([op('a', 'conflict', [change('a::url', 'conflict', { choices: ['team', 'spec'] })])]);
  assert.deepEqual(keepDecisions(before, after, decide(['a::url', { choice: 'both' }])).choices, {});
  assert.deepEqual(keepDecisions(null, after, decide(['a::url', { choice: 'team' }])), noDecisions());
});

test('ef_syn_04_lentille_se_limite_aux_choix_du_changement_et_garde_l_ordre_du_contrat', () => {
  const view: OpView = {
    ours: '',
    theirs: '',
    base: null,
    result: '',
    hunks: [{ changeId: 'b::body', ours: [5, 6], theirs: [5, 7], base: null, result: [5, 6] }],
  };
  const changes = [change('b::body', 'conflict', { choices: ['edit', 'team', 'both'] }), change('b::other', 'applied')];
  const [lens, ...rest] = lensesFor(view, changes, decide(['b::body', { choice: 'both' }]));
  assert.equal(rest.length, 0);
  assert.deepEqual(
    lens.choices.map((c) => c.key),
    ['team', 'both', 'edit'],
  );
  assert.deepEqual(
    lens.choices.map((c) => c.label),
    ["Garder l'équipe", 'Combiner les deux', 'Éditer à la main'],
  );
  assert.equal(lens.line, 5);
  assert.equal(lens.label, null);
  assert.equal(lens.active, 'both');
});

test('ef_syn_04_lentille_sans_ligne_dans_le_resultat_passe_en_tete_avec_le_nom_du_champ', () => {
  const view: OpView = { ours: '', theirs: '', base: null, result: '', hunks: [{ changeId: 'h', ours: [15, 17], theirs: null, base: null, result: null }] };
  const [lens] = lensesFor(view, [change('h', 'conflict', { label: 'En-tête X-Canal', choices: ['team', 'spec'] })], noDecisions());
  assert.equal(lens.line, 1);
  assert.equal(lens.label, 'En-tête X-Canal');
  assert.equal(lens.active, null);
});

const hunk = (changeId: string, extra: Partial<Hunk> = {}): Hunk => ({ changeId, ours: [3, 4], theirs: [3, 5], base: [3, 3], result: [3, 4], ...extra });

test('ef_syn_04_volets_ne_teintent_que_ce_que_chaque_cote_a_change', () => {
  const view: OpView = {
    ours: '',
    theirs: '',
    base: '',
    result: '',
    hunks: [hunk('c', { ours: [3, 4], theirs: [3, 5] }), hunk('t', { ours: null, theirs: [8, 8] }), hunk('o', { ours: [10, 10], theirs: [10, 10] }), hunk('s', { ours: [12, 12], theirs: [12, 12] })],
  };
  const changes = [change('c', 'conflict'), change('t', 'applied'), change('o', 'kept'), change('s', 'same')];
  assert.deepEqual(paneMarks(view, changes, 'ours'), [
    { from: 3, to: 4, cls: 'h-o' },
    { from: 10, to: 10, cls: 'h-o' },
  ]);
  assert.deepEqual(paneMarks(view, changes, 'theirs'), [
    { from: 3, to: 5, cls: 'h-t' },
    { from: 8, to: 8, cls: 'h-t' },
  ]);
  assert.equal(paneMarks(view, changes, 'base').length, 4);
  assert.ok(paneMarks(view, changes, 'base').every((m) => m.cls === 'h-b'));
});

test('ef_syn_04_resultat_reste_ambre_tant_que_le_conflit_n_est_pas_arbitre', () => {
  const view: OpView = {
    ours: '',
    theirs: '',
    base: null,
    result: '',
    hunks: [hunk('c1', { result: [3, 4] }), hunk('c2', { result: [8, 9] }), hunk('c3', { result: [12, 12] }), hunk('c4', { result: [14, 14] }), hunk('a', { result: [20, 20] }), hunk('k', { result: [22, 22] }), hunk('m', { result: [24, 24] }), hunk('s', { result: [26, 26] })],
  };
  const changes = [
    change('c1', 'conflict'),
    change('c2', 'conflict'),
    change('c3', 'conflict'),
    change('c4', 'conflict'),
    change('a', 'applied'),
    change('k', 'kept'),
    change('m', 'merged'),
    change('s', 'same'),
  ];
  const decisions = decide(['c2', { choice: 'team' }], ['c3', { choice: 'spec' }], ['c4', { choice: 'edit', value: 'x' }]);
  assert.deepEqual(
    resultMarks(view, changes, decisions).map((m) => `${m.from}:${m.cls}`),
    ['3:r-u', '8:r-o', '12:r-t', '14:r-e', '20:r-t', '22:r-o', '24:r-b'],
  );
  assert.equal(resultMarks(view, changes, decide(['c1', { choice: 'both' }]))[0].cls, 'r-b');
});

test('ef_syn_04_lignes_identiques_se_replient_hors_du_contexte_des_changements', () => {
  assert.deepEqual(foldRanges(30, []), []);
  assert.deepEqual(foldRanges(30, [[10, 11]]), [
    { from: 1, to: 6, label: 'lignes 1 à 6 masquées, identiques des deux côtés' },
    { from: 15, to: 30, label: 'lignes 15 à 30 masquées, identiques des deux côtés' },
  ]);
});

test('ef_syn_04_repli_fusionne_les_plages_proches_et_garde_les_petits_ecarts', () => {
  assert.deepEqual(
    foldRanges(40, [[20, 21], [8, 8]]).map(({ from, to }) => [from, to]),
    [[1, 4], [12, 16], [25, 40]],
  );
  assert.deepEqual(
    foldRanges(40, [[10, 10], [16, 16]]).map(({ from, to }) => [from, to]),
    [[1, 6], [20, 40]],
  );
  assert.deepEqual(foldRanges(12, [[2, 10]]), []);
  assert.deepEqual(
    foldRanges(12, [[1, 1]]).map(({ from, to }) => [from, to]),
    [[5, 12]],
  );
});

test('ef_syn_04_repli_respecte_le_contexte_et_le_minimum_demandes', () => {
  assert.deepEqual(
    foldRanges(20, [[10, 10]], 1, 2).map(({ from, to }) => [from, to]),
    [[1, 8], [12, 20]],
  );
  assert.deepEqual(foldRanges(5, [[3, 3]], 0, 2).map(({ from, to }) => [from, to]), [[1, 2], [4, 5]]);
});

test('ef_syn_04_libelle_du_repli_accorde_le_nombre_de_lignes', () => {
  assert.equal(foldLabel({ from: 1, to: 4 }), 'lignes 1 à 4 masquées, identiques des deux côtés');
  assert.equal(foldLabel({ from: 7, to: 7 }), 'ligne 7 masquée, identique des deux côtés');
});

test('ef_syn_04_fichier_sans_retour_final_compte_ses_lignes_comme_codemirror', () => {
  assert.equal(lineCount('a\nb\nc\n'), 3);
  assert.equal(lineCount('a\nb\nc'), 3);
  assert.equal(lineCount(''), 1);
  assert.equal(trimEnd('a\n'), 'a');
  assert.equal(trimEnd('a\n\n'), 'a\n');
});

test('ef_syn_05_groupes_de_la_barre_laterale_suivent_le_statut_des_operations', () => {
  const g = groupOps(conflicts());
  assert.deepEqual(
    g.conflicts.map((c) => c.changeId),
    ['a::url', 'b::body'],
  );
  assert.deepEqual(
    g.auto.map((o) => o.key),
    ['c', 'd'],
  );
  assert.deepEqual(
    g.created.map((o) => o.key),
    ['e', 'f'],
  );
  assert.deepEqual(
    g.removed.map((o) => o.key),
    ['g'],
  );
  assert.deepEqual(
    g.missing.map((o) => o.key),
    ['h'],
  );
});

test('ef_syn_05_application_compte_les_lignes_qu_elle_ecrit', () => {
  const p = conflicts();
  assert.equal(applyCount(p, noDecisions()), 2 + 1 + 2 + 1);
  assert.equal(applyCount(p, toggleSkip(noDecisions(), 'e')), 2 + 1 + 1 + 1);
  assert.equal(applyCount(p, setRecreate(noDecisions(), 'h', true)), 2 + 1 + 2 + 1 + 1);
  assert.equal(applyCount(p, toggleSkip(toggleSkip(noDecisions(), 'e'), 'e')), 2 + 1 + 2 + 1);
});

test('ef_syn_05_ignorer_et_recreer_se_basculent_sans_doublon', () => {
  const skipped = toggleSkip(noDecisions(), 'e');
  assert.deepEqual(skipped.skip, ['e']);
  assert.deepEqual(toggleSkip(skipped, 'e').skip, []);
  const recreate = setRecreate(setRecreate(noDecisions(), 'h', true), 'h', true);
  assert.deepEqual(recreate.recreate, ['h']);
  assert.deepEqual(setRecreate(recreate, 'h', false).recreate, []);
});

test('ef_syn_05_bouton_d_application_accorde_le_nombre_de_changements', () => {
  assert.equal(applyLabel(conflicts(), noDecisions()), 'Appliquer 6 changements');
  assert.equal(applyLabel(plan([op('c', 'updated')]), noDecisions()), 'Appliquer 1 changement');
  assert.equal(applyLabel(plan([op('d', 'kept')]), noDecisions()), 'Mettre à jour la base');
  assert.equal(applyLabel(plan([op('d', 'kept')], { hasBase: false, from: null }), noDecisions()), 'Enregistrer la connexion');
});

test('ef_syn_05_plan_sans_ecart_est_a_jour_sauf_a_la_premiere_synchro', () => {
  assert.equal(isUpToDate(plan([op('d', 'kept'), op('u', 'unchanged')])), true);
  assert.equal(isUpToDate(plan([])), true);
  assert.equal(isUpToDate(conflicts()), false);
  assert.equal(isUpToDate(plan([op('c', 'updated')])), false);
  assert.equal(isUpToDate(plan([op('g', 'removed')])), false);
  assert.equal(isUpToDate(plan([], { hasBase: false, from: null })), false);
});

test('ef_syn_05_barre_d_etat_dit_les_conflits_puis_pret_puis_a_jour', () => {
  assert.deepEqual(specStatus(null, noDecisions(), false), { tone: '', label: 'Spec OpenAPI' });
  assert.deepEqual(specStatus(null, noDecisions(), true), { tone: 'good', label: 'Spec à jour' });
  assert.deepEqual(specStatus(conflicts(), noDecisions(), false), { tone: 'warn', label: 'Spec : 2 conflits' });
  assert.deepEqual(specStatus(conflicts(), decide(['a::url', { choice: 'team' }]), false), { tone: 'warn', label: 'Spec : 1 conflit' });
  const resolved = decide(['a::url', { choice: 'team' }], ['b::body', { choice: 'team' }]);
  assert.deepEqual(specStatus(conflicts(), resolved, false), { tone: '', label: 'Spec : prête à appliquer' });
  assert.deepEqual(specStatus(plan([op('d', 'kept')]), noDecisions(), false), { tone: 'good', label: 'Spec à jour' });
});

test('ef_syn_05_arbre_signale_les_fichiers_dont_un_conflit_attend_un_arbitrage', () => {
  const p = plan([
    op('a', 'conflict', [change('a::1', 'conflict'), change('a::2', 'conflict')], { file: 'dir/a.yml' }),
    op('b', 'conflict', [change('b::1', 'conflict')], { file: 'dir/b.yml' }),
    op('n', 'conflict', [change('n::1', 'conflict')], { file: null }),
    op('c', 'updated', [change('c::1', 'applied')], { file: 'dir/c.yml' }),
  ]);
  assert.deepEqual([...conflictPaths(p, noDecisions())].sort(), ['dir/a.yml', 'dir/b.yml']);
  assert.deepEqual([...conflictPaths(p, decide(['a::1', { choice: 'team' }], ['b::1', { choice: 'spec' }]))], ['dir/a.yml']);
  assert.deepEqual([...conflictPaths(p, decide(['a::1', { choice: 'team' }], ['a::2', { choice: 'team' }], ['b::1', { choice: 'spec' }]))], []);
  assert.equal(conflictPaths(null, noDecisions()).size, 0);
});

test('ef_syn_05_versions_de_la_source_disent_le_sens_de_la_comparaison', () => {
  assert.equal(versionLabel(plan([])), 'v2.3.0 → v2.4.0');
  assert.equal(versionLabel(plan([], { from: null })), 'v2.4.0 · sans base');
  assert.equal(versionLabel(plan([], { from: { title: 'API', version: 'v2.4.0' }, to: { title: 'API', version: '2.4.0' } })), 'v2.4.0 · même version');
  assert.equal(versionLabel(plan([], { to: { title: 'API', version: 'v3.0.0' } })), 'v2.3.0 → v3.0.0');
});

test('ef_syn_05_source_se_resume_a_son_nom_de_fichier', () => {
  assert.equal(sourceName('./spec/openapi.yml'), 'openapi.yml');
  assert.equal(sourceName('https://api.example.com/v1/openapi.json'), 'openapi.json');
  assert.equal(sourceName('C:\\specs\\openapi.yml'), 'openapi.yml');
  assert.equal(sourceName('openapi.yml'), 'openapi.yml');
});

test('ef_syn_01_rapprochement_nomme_les_operations_par_methode_et_chemin', () => {
  const p = plan([op('removedOp', 'removed', [], { method: 'GET', path: '/transactions/export' }), op('GET /exports', 'new', [], { method: 'GET', path: '/exports' })]);
  assert.equal(pairingLabel(p, 'removedOp'), 'GET /transactions/export');
  assert.equal(pairingLabel(p, 'GET /exports'), 'GET /exports');
  assert.equal(pairingLabel(p, 'inconnue'), 'inconnue');
});

test('ef_syn_03_operation_retiree_reste_dans_les_deprecies_avec_son_etat', () => {
  assert.deepEqual(opState('removed'), { text: 'conservée', tone: '' });
  assert.deepEqual(opState('updated'), { text: 'spec appliquée', tone: 'good' });
  assert.deepEqual(opState('kept'), { text: 'équipe gardée', tone: '' });
  assert.deepEqual(opState('conflict'), { text: 'à arbitrer', tone: 'warn' });
});

test('ef_syn_06_bilan_apres_application_accorde_et_omet_les_postes_vides', () => {
  assert.equal(reportSummary({ written: ['a.yml'], created: ['b.yml', 'c.yml'], removed: ['d.yml'], ignored: ['e'] }), '1 requête modifiée, 2 requêtes créées, 1 requête dépréciée');
  assert.equal(reportSummary({ written: ['a.yml', 'b.yml'], created: [], removed: [], ignored: [] }), '2 requêtes modifiées');
  assert.equal(reportSummary({ written: [], created: [], removed: [], ignored: ['x'] }), 'Aucune requête modifiée');
  assert.deepEqual(touchedFiles({ written: ['a.yml'], created: ['b.yml'], removed: ['d.yml'], ignored: ['e'] }), ['a.yml', 'b.yml', 'd.yml']);
});
