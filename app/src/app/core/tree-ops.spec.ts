import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { TreeItem } from './model.ts';
import {
  canDrop,
  cloneName,
  closeUnder,
  contentsOf,
  describeContents,
  dropPosition,
  dropPositionAt,
  endSeq,
  findItem,
  insideRect,
  isNoopMove,
  isUnder,
  missingPaths,
  navigate,
  neighbourOf,
  normalizeQuery,
  remapPath,
  remapPaths,
  remapSet,
  reorderMove,
  reorderNotice,
  sanitizeName,
  tabbablePath,
  treeKeyAction,
  unsavedMessage,
  validateName,
  visibleRows,
} from './tree-ops.ts';

const request = (path: string): TreeItem => ({ kind: 'request', path, name: path, method: 'GET', requestType: 'http', url: '', deprecated: false });
const folder = (path: string, children: TreeItem[] = []): TreeItem => ({ kind: 'folder', path, name: path, children });

const tree: TreeItem[] = [
  folder('auth', [request('auth/connexion.yml'), request('auth/jeton.yml')]),
  folder('transactions', [request('transactions/liste.yml'), folder('transactions/export', [request('transactions/export/csv.yml')]), request('transactions/detail.yml')]),
  request('sante.yml'),
];

const key = (name: string, mods: Partial<Record<'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey', boolean>> = {}) => ({
  key: name,
  metaKey: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  ...mods,
});

test('ef_col_01_position_de_depot_dune_requete_est_avant_ou_apres_selon_la_moitie_survolee', () => {
  assert.equal(dropPosition(0, 'request'), 'before');
  assert.equal(dropPosition(0.49, 'request'), 'before');
  assert.equal(dropPosition(0.5, 'request'), 'after');
  assert.equal(dropPosition(1, 'request'), 'after');
});

test('ef_col_01_position_de_depot_dun_dossier_replie_decoupe_la_ligne_en_trois', () => {
  assert.equal(dropPosition(0.1, 'folder'), 'before');
  assert.equal(dropPosition(0.33, 'folder'), 'before');
  assert.equal(dropPosition(0.34, 'folder'), 'inside');
  assert.equal(dropPosition(0.5, 'folder'), 'inside');
  assert.equal(dropPosition(0.66, 'folder'), 'inside');
  assert.equal(dropPosition(0.67, 'folder'), 'after');
  assert.equal(dropPosition(0.95, 'folder'), 'after');
});

test('ef_col_01_position_de_depot_dun_dossier_ouvert_ne_propose_pas_apres', () => {
  assert.equal(dropPosition(0.1, 'folder', true), 'before');
  assert.equal(dropPosition(0.5, 'folder', true), 'inside');
  assert.equal(dropPosition(0.95, 'folder', true), 'inside');
});

test('ef_col_01_depot_dun_dossier_dans_lui_meme_ou_un_descendant_est_refuse', () => {
  assert.equal(canDrop('transactions', 'transactions'), false);
  assert.equal(canDrop('transactions', 'transactions/export'), false);
  assert.equal(canDrop('transactions', 'transactions/export/csv.yml'), false);
  assert.equal(canDrop('transactions/export', 'transactions/liste.yml'), true);
  assert.equal(canDrop('transactions/export', 'transactions'), true);
});

test('ef_col_01_depot_vers_la_racine_ou_un_voisin_de_meme_prefixe_est_permis', () => {
  assert.equal(canDrop('transactions', ''), true);
  assert.equal(canDrop('auth', 'auth-bis'), true);
  assert.equal(canDrop('auth', 'auth-bis/liste.yml'), true);
  assert.equal(canDrop('auth/jeton.yml', 'auth/connexion.yml'), true);
  assert.equal(canDrop('auth/jeton.yml', 'auth/jeton.yml'), false);
});

test('ef_col_01_chemin_dun_fichier_deplace_ou_renomme_est_remplace_tel_quel', () => {
  assert.equal(remapPath('auth/jeton.yml', 'auth/jeton.yml', 'transactions/jeton.yml'), 'transactions/jeton.yml');
  assert.equal(remapPath('auth/connexion.yml', 'auth/jeton.yml', 'transactions/jeton.yml'), 'auth/connexion.yml');
  assert.equal(remapPath('auth/jeton.yml.bak', 'auth/jeton.yml', 'x.yml'), 'auth/jeton.yml.bak');
});

test('ef_col_01_chemins_sous_un_dossier_renomme_ou_deplace_changent_de_prefixe', () => {
  assert.equal(remapPath('transactions', 'transactions', 'paiements'), 'paiements');
  assert.equal(remapPath('transactions/liste.yml', 'transactions', 'paiements'), 'paiements/liste.yml');
  assert.equal(remapPath('transactions/export/csv.yml', 'transactions', 'auth/transactions'), 'auth/transactions/export/csv.yml');
  assert.equal(remapPath('transactions-bis/liste.yml', 'transactions', 'paiements'), 'transactions-bis/liste.yml');
  assert.equal(remapPath('auth/connexion.yml', 'transactions', 'paiements'), 'auth/connexion.yml');
});

test('ef_col_01_onglets_suivent_le_nouveau_chemin_et_gardent_leur_brouillon', () => {
  const tabs = [
    { path: 'transactions/liste.yml', doc: { name: 'Liste', url: 'brouillon' }, saved: 'a' },
    { path: 'transactions/export/csv.yml', doc: { name: 'CSV', url: '' }, saved: 'b' },
    { path: 'auth/jeton.yml', doc: { name: 'Jeton', url: '' }, saved: 'c' },
  ];
  const moved = remapPaths(tabs, 'transactions', 'paiements');
  assert.deepEqual(
    moved.map((t) => t.path),
    ['paiements/liste.yml', 'paiements/export/csv.yml', 'auth/jeton.yml'],
  );
  assert.equal(moved[0].doc, tabs[0].doc);
  assert.equal(moved[0].saved, 'a');
  assert.equal(moved[2], tabs[2]);
});

test('ef_col_01_onglet_dun_fichier_renomme_suit_son_nouveau_chemin', () => {
  const tabs = [{ path: 'auth/jeton.yml' }, { path: 'auth/connexion.yml' }];
  assert.deepEqual(remapPaths(tabs, 'auth/jeton.yml', 'auth/jeton oauth.yml'), [{ path: 'auth/jeton oauth.yml' }, { path: 'auth/connexion.yml' }]);
});

test('ef_col_01_dossiers_ouverts_suivent_un_dossier_deplace', () => {
  const open = new Set(['auth', 'transactions', 'transactions/export']);
  assert.deepEqual([...remapSet(open, 'transactions', 'auth/transactions')], ['auth', 'auth/transactions', 'auth/transactions/export']);
});

test('ef_col_01_clavier_option_fleche_deplace_dun_cran_parmi_les_freres', () => {
  assert.deepEqual(reorderMove(tree, 'transactions/export', -1), { target: 'transactions/liste.yml', position: 'before' });
  assert.deepEqual(reorderMove(tree, 'transactions/export', 1), { target: 'transactions/detail.yml', position: 'after' });
  assert.deepEqual(reorderMove(tree, 'auth', 1), { target: 'transactions', position: 'after' });
  assert.deepEqual(reorderMove(tree, 'sante.yml', -1), { target: 'transactions', position: 'before' });
});

test('ef_col_01_clavier_option_fleche_ne_sort_pas_de_la_liste', () => {
  assert.equal(reorderMove(tree, 'auth', -1), null);
  assert.equal(reorderMove(tree, 'sante.yml', 1), null);
  assert.equal(reorderMove(tree, 'auth/connexion.yml', -1), null);
  assert.equal(reorderMove(tree, 'transactions/detail.yml', 1), null);
  assert.equal(reorderMove(tree, 'inconnu.yml', 1), null);
});

test('ef_col_01_nom_propose_a_la_duplication_ajoute_copie', () => {
  assert.equal(cloneName('Connexion'), 'Connexion copie');
  assert.equal(cloneName('Connexion copie'), 'Connexion copie copie');
});

test('ef_col_01_clavier_option_fleche_est_reconnu_sans_confondre_les_autres_touches', () => {
  assert.equal(treeKeyAction(key('ArrowUp', { altKey: true }), true), 'up');
  assert.equal(treeKeyAction(key('ArrowDown', { altKey: true }), false), 'down');
  assert.equal(treeKeyAction(key('ArrowDown', { altKey: true, metaKey: true }), true), null);
  assert.equal(treeKeyAction(key('ArrowDown', { altKey: true, shiftKey: true }), true), null);
});

test('ef_col_04_nom_vide_ou_blanc_est_refuse', () => {
  assert.equal(validateName('', 'request'), 'Donne un nom.');
  assert.equal(validateName('   ', 'folder'), 'Donne un nom.');
  assert.equal(validateName('', 'collection'), 'Donne un nom.');
  assert.equal(validateName('  Connexion  ', 'request'), null);
});

test('ef_col_04_requete_nommee_collection_ou_folder_est_refusee_sans_tenir_compte_de_la_casse', () => {
  for (const name of ['collection', 'folder', 'Folder', 'COLLECTION', ' folder ']) {
    assert.match(validateName(name, 'request') ?? '', /réservé/, name);
  }
  assert.equal(validateName('folder 2', 'request'), null);
  assert.equal(validateName('Ma collection', 'request'), null);
  assert.equal(validateName('folder', 'collection'), null);
});

test('ef_col_04_nom_qui_commence_par_un_tiret_ou_nomme_un_peripherique_windows_est_refuse', () => {
  assert.match(validateName('-brouillon', 'request') ?? '', /tiret/);
  assert.match(validateName('-x', 'collection') ?? '', /tiret/);
  for (const name of ['CON', 'nul', 'Com1', 'LPT9', 'aux']) assert.match(validateName(name, 'request') ?? '', /réservé par Windows/, name);
  assert.equal(validateName('CONNEXION', 'request'), null);
  assert.equal(validateName('console', 'request'), null);
});

test('ef_col_04_nom_trop_long_ou_avec_caractere_de_controle_est_refuse', () => {
  assert.equal(validateName('a'.repeat(255), 'request'), null);
  assert.match(validateName('a'.repeat(256), 'request') ?? '', /trop long/);
  assert.match(validateName('Liste\u0000', 'request') ?? '', /invisible/);
  assert.match(validateName('Liste\u001f', 'folder') ?? '', /invisible/);
  assert.match(validateName('...', 'request') ?? '', /points/);
});

test('ef_col_04_les_caracteres_que_le_fichier_assainit_restent_acceptes_dans_le_nom_affiche', () => {
  assert.equal(validateName('GET /transactions/:id', 'request'), null);
  assert.equal(validateName('Annuler : erreur de saisie ?', 'request'), null);
  assert.equal(validateName('Paiements 2026', 'folder'), null);
});

test('ef_col_04_dossier_cache_par_larbre_est_refuse', () => {
  for (const name of ['.oc-sync', '.git', '.cache', 'node_modules', 'Node_Modules', 'folder.yml', 'opencollection.yml']) {
    assert.match(validateName(name, 'folder', 'auth') ?? '', /réservé/, name);
  }
  assert.equal(validateName('git', 'folder', 'auth'), null);
});

test('ef_col_04_dossiers_environments_et_mocks_ne_sont_reserves_qua_la_racine', () => {
  assert.match(validateName('environments', 'folder', '') ?? '', /réservé/);
  assert.match(validateName('Mocks', 'folder') ?? '', /réservé/);
  assert.equal(validateName('environments', 'folder', 'auth'), null);
  assert.equal(validateName('mocks', 'folder', 'transactions/export'), null);
});

test('ef_col_04_onglets_dun_dossier_supprime_se_ferment_et_laissent_longlet_voisin_actif', () => {
  const tabs = [{ path: 'auth/jeton.yml' }, { path: 'transactions/liste.yml' }, { path: 'transactions/detail.yml' }, { path: 'sante.yml' }];
  const closed = closeUnder(tabs, 'transactions/liste.yml', 'transactions');
  assert.deepEqual(closed.tabs, [{ path: 'auth/jeton.yml' }, { path: 'sante.yml' }]);
  assert.equal(closed.active, 'sante.yml');
});

test('ef_col_04_suppression_garde_longlet_actif_quand_il_nest_pas_concerne', () => {
  const tabs = [{ path: 'auth/jeton.yml' }, { path: 'transactions/liste.yml' }];
  assert.deepEqual(closeUnder(tabs, 'auth/jeton.yml', 'transactions'), { tabs: [{ path: 'auth/jeton.yml' }], active: 'auth/jeton.yml' });
});

test('ef_col_04_suppression_du_dernier_onglet_actif_selectionne_le_precedent_ou_aucun', () => {
  const tabs = [{ path: 'auth/jeton.yml' }, { path: 'sante.yml' }];
  assert.equal(closeUnder(tabs, 'sante.yml', 'sante.yml').active, 'auth/jeton.yml');
  assert.deepEqual(closeUnder([{ path: 'sante.yml' }], 'sante.yml', 'sante.yml'), { tabs: [], active: null });
  assert.equal(closeUnder(tabs, 'sante.yml', 'sante').active, 'sante.yml');
});

test('ef_col_04_suppression_selectionne_le_frere_suivant_puis_precedent_puis_le_dossier', () => {
  assert.equal(neighbourOf(tree, 'auth/connexion.yml'), 'auth/jeton.yml');
  assert.equal(neighbourOf(tree, 'auth/jeton.yml'), 'auth/connexion.yml');
  assert.equal(neighbourOf(tree, 'transactions/export/csv.yml'), 'transactions/export');
  assert.equal(neighbourOf(tree, 'sante.yml'), 'transactions');
  assert.equal(neighbourOf([request('seul.yml')], 'seul.yml'), null);
});

test('ef_col_04_confirmation_de_suppression_compte_les_requetes_et_sous_dossiers_contenus', () => {
  const transactions = findItem(tree, 'transactions');
  assert.ok(transactions);
  assert.deepEqual(contentsOf(transactions), { requests: 3, folders: 1 });
  assert.deepEqual(contentsOf(findItem(tree, 'auth')!), { requests: 2, folders: 0 });
  assert.deepEqual(contentsOf(findItem(tree, 'sante.yml')!), { requests: 0, folders: 0 });
  assert.deepEqual(contentsOf(folder('vide')), { requests: 0, folders: 0 });
});

test('ef_col_04_chemin_sous_un_dossier_est_reconnu_sans_confondre_les_prefixes', () => {
  assert.ok(isUnder('transactions/liste.yml', 'transactions'));
  assert.ok(isUnder('transactions', 'transactions'));
  assert.equal(isUnder('transactions-bis/liste.yml', 'transactions'), false);
  assert.equal(isUnder('auth', 'auth/jeton.yml'), false);
});

test('ef_col_04_raccourcis_renommer_dupliquer_supprimer_selon_la_plateforme', () => {
  assert.equal(treeKeyAction(key('F2'), true), 'rename');
  assert.equal(treeKeyAction(key('F2'), false), 'rename');
  assert.equal(treeKeyAction(key('d', { metaKey: true }), true), 'clone');
  assert.equal(treeKeyAction(key('D', { ctrlKey: true }), false), 'clone');
  assert.equal(treeKeyAction(key('d', { ctrlKey: true }), true), null);
  assert.equal(treeKeyAction(key('d'), true), null);
  assert.equal(treeKeyAction(key('Backspace', { metaKey: true }), true), 'delete');
  assert.equal(treeKeyAction(key('Delete'), false), 'delete');
  assert.equal(treeKeyAction(key('Delete'), true), 'delete');
  assert.equal(treeKeyAction(key('Backspace'), true), null);
  assert.equal(treeKeyAction(key('Backspace'), false), null);
});

test('ef_col_04_menu_contextuel_au_clavier_touche_menu_ou_maj_f10', () => {
  assert.equal(treeKeyAction(key('ContextMenu'), false), 'menu');
  assert.equal(treeKeyAction(key('F10', { shiftKey: true }), false), 'menu');
  assert.equal(treeKeyAction(key('F10'), false), null);
  assert.equal(treeKeyAction(key('F10', { shiftKey: true, ctrlKey: true }), false), null);
});

test('ef_col_04_contenu_dun_dossier_est_decrit_au_singulier_ou_au_pluriel', () => {
  assert.equal(describeContents({ requests: 6, folders: 1 }), '6 requêtes et 1 sous-dossier');
  assert.equal(describeContents({ requests: 1, folders: 0 }), '1 requête');
  assert.equal(describeContents({ requests: 0, folders: 2 }), '2 sous-dossiers');
  assert.equal(describeContents({ requests: 0, folders: 0 }), '');
});

test('ef_col_04_suppression_avec_onglets_modifies_previent_que_les_modifications_seront_perdues', () => {
  assert.equal(unsavedMessage(['Connexion']), "L'onglet « Connexion » contient des modifications non enregistrées : elles seront perdues.");
  assert.match(unsavedMessage(['Connexion', 'Jeton']), /^2 onglets contiennent .*\(Connexion, Jeton\)/);
});

const rowsOf = (open: string[] = [], query = '') => visibleRows(tree, new Set(open), query);
const paths = (open: string[] = [], query = '') => rowsOf(open, query).map((r) => r.path);

test('ef_ux_01_lignes_affichees_suivent_les_dossiers_ouverts', () => {
  assert.deepEqual(paths(), ['auth', 'transactions', 'sante.yml']);
  assert.deepEqual(paths(['transactions']), ['auth', 'transactions', 'transactions/liste.yml', 'transactions/export', 'transactions/detail.yml', 'sante.yml']);
  assert.deepEqual(rowsOf(['transactions', 'transactions/export']).find((r) => r.path === 'transactions/export/csv.yml'), {
    path: 'transactions/export/csv.yml',
    kind: 'request',
    depth: 2,
    parent: 'transactions/export',
    open: false,
  });
  assert.deepEqual(rowsOf(['transactions']).find((r) => r.path === 'transactions'), { path: 'transactions', kind: 'folder', depth: 0, parent: '', open: true });
  assert.equal(rowsOf().find((r) => r.path === 'auth')?.open, false);
});

test('ef_ux_01_filtre_ouvre_les_dossiers_utiles_et_masque_le_reste_sans_tenir_compte_des_accents', () => {
  assert.deepEqual(paths([], 'csv'), ['transactions', 'transactions/export', 'transactions/export/csv.yml']);
  assert.ok(rowsOf([], 'csv').filter((r) => r.kind === 'folder').every((r) => r.open));
  assert.equal(normalizeQuery('  Étagé  '), 'etage');
  assert.deepEqual(visibleRows([request('élan.yml'), request('autre.yml')], new Set(), normalizeQuery('ELAN')).map((r) => r.path), ['élan.yml']);
  assert.deepEqual(paths([], 'absent'), []);
});

test('ef_ux_01_une_seule_ligne_est_atteignable_par_tab', () => {
  const rows = rowsOf(['auth']);
  assert.equal(tabbablePath(rows, 'auth/jeton.yml'), 'auth/jeton.yml');
  assert.equal(tabbablePath(rows, 'transactions/liste.yml'), 'auth');
  assert.equal(tabbablePath(rows, null), 'auth');
  assert.equal(tabbablePath([], 'auth'), null);
});

test('ef_ux_01_fleches_haut_bas_debut_et_fin_parcourent_les_lignes_affichees', () => {
  const rows = rowsOf(['transactions']);
  assert.deepEqual(navigate(rows, 'auth', 'next'), { focus: 'transactions' });
  assert.deepEqual(navigate(rows, 'transactions', 'next'), { focus: 'transactions/liste.yml' });
  assert.deepEqual(navigate(rows, 'transactions/detail.yml', 'next'), { focus: 'sante.yml' });
  assert.deepEqual(navigate(rows, 'sante.yml', 'prev'), { focus: 'transactions/detail.yml' });
  assert.equal(navigate(rows, 'auth', 'prev'), null);
  assert.equal(navigate(rows, 'sante.yml', 'next'), null);
  assert.deepEqual(navigate(rows, 'transactions/liste.yml', 'first'), { focus: 'auth' });
  assert.deepEqual(navigate(rows, 'auth', 'last'), { focus: 'sante.yml' });
  assert.equal(navigate(rows, 'inconnu.yml', 'next'), null);
});

test('ef_ux_01_fleche_droite_ouvre_un_dossier_ferme_puis_va_au_premier_enfant', () => {
  assert.deepEqual(navigate(rowsOf(), 'auth', 'expand'), { toggle: 'auth' });
  assert.deepEqual(navigate(rowsOf(['auth']), 'auth', 'expand'), { focus: 'auth/connexion.yml' });
  assert.equal(navigate(rowsOf(['auth']), 'auth/connexion.yml', 'expand'), null);
  const empty = visibleRows([folder('vide'), request('x.yml')], new Set(['vide']), '');
  assert.equal(navigate(empty, 'vide', 'expand'), null);
});

test('ef_ux_01_fleche_gauche_ferme_un_dossier_ouvert_puis_va_au_parent', () => {
  const rows = rowsOf(['transactions']);
  assert.deepEqual(navigate(rows, 'transactions', 'collapse'), { toggle: 'transactions' });
  assert.deepEqual(navigate(rows, 'transactions/liste.yml', 'collapse'), { focus: 'transactions' });
  assert.deepEqual(navigate(rows, 'transactions/export', 'collapse'), { focus: 'transactions' });
  assert.equal(navigate(rows, 'auth', 'collapse'), null);
  assert.equal(navigate(rows, 'sante.yml', 'collapse'), null);
});

test('ef_ux_01_fleche_gauche_sous_un_filtre_va_au_parent_car_les_dossiers_restent_ouverts', () => {
  const rows = rowsOf([], 'csv');
  assert.deepEqual(navigate(rows, 'transactions/export', 'collapse', true), { focus: 'transactions' });
  assert.deepEqual(navigate(rows, 'transactions/export', 'collapse', false), { toggle: 'transactions/export' });
});

test('ef_ux_01_entree_et_espace_activent_la_ligne', () => {
  assert.equal(treeKeyAction(key('Enter'), true), 'activate');
  assert.equal(treeKeyAction(key(' '), false), 'activate');
  assert.deepEqual(navigate(rowsOf(), 'sante.yml', 'activate'), { activate: 'sante.yml' });
});

test('ef_ux_01_touches_de_navigation_sont_reconnues_sans_modificateur', () => {
  const expected = { ArrowDown: 'next', ArrowUp: 'prev', Home: 'first', End: 'last', ArrowRight: 'expand', ArrowLeft: 'collapse' };
  for (const [name, action] of Object.entries(expected)) {
    assert.equal(treeKeyAction(key(name), true), action, name);
    assert.equal(treeKeyAction(key(name, { metaKey: true }), true), null, `${name} avec ⌘`);
    assert.equal(treeKeyAction(key(name, { ctrlKey: true }), false), null, `${name} avec Ctrl`);
    assert.equal(treeKeyAction(key(name, { shiftKey: true }), false), null, `${name} avec Maj`);
  }
  assert.equal(treeKeyAction(key('ArrowUp', { altKey: true }), true), 'up');
  assert.equal(treeKeyAction(key('toString'), true), null);
});

test('ef_col_01_option_fleche_en_bout_de_liste_annonce_la_position', () => {
  assert.equal(reorderNotice(-1), 'Déjà en première position');
  assert.equal(reorderNotice(1), 'Déjà en dernière position');
  assert.equal(reorderMove(tree, 'auth', -1), null);
  assert.equal(reorderMove(tree, 'sante.yml', 1), null);
});

test('ef_col_01_depot_qui_ne_change_pas_la_position_est_sans_effet', () => {
  assert.equal(isNoopMove(tree, 'auth', 'transactions', 'before'), true);
  assert.equal(isNoopMove(tree, 'transactions', 'auth', 'after'), true);
  assert.equal(isNoopMove(tree, 'auth/jeton.yml', 'auth/jeton.yml', 'after'), true);
  assert.equal(isNoopMove(tree, 'sante.yml', '', 'inside'), true);
  assert.equal(isNoopMove(tree, 'auth/jeton.yml', 'auth', 'inside'), true);
});

test('ef_col_01_depot_qui_change_la_position_ou_le_dossier_est_applique', () => {
  assert.equal(isNoopMove(tree, 'auth', 'transactions', 'after'), false);
  assert.equal(isNoopMove(tree, 'transactions', 'auth', 'before'), false);
  assert.equal(isNoopMove(tree, 'auth', 'sante.yml', 'before'), false);
  assert.equal(isNoopMove(tree, 'auth/connexion.yml', 'auth', 'inside'), false);
  assert.equal(isNoopMove(tree, 'auth', '', 'inside'), false);
  assert.equal(isNoopMove(tree, 'auth/jeton.yml', 'transactions/liste.yml', 'before'), false);
  assert.equal(isNoopMove(tree, 'inconnu.yml', '', 'inside'), false);
});

test('ef_col_01_position_de_depot_est_calculee_depuis_les_coordonnees_de_levenement', () => {
  const box = { top: 100, height: 26 };
  assert.equal(dropPositionAt(101, box, 'request'), 'before');
  assert.equal(dropPositionAt(120, box, 'request'), 'after');
  assert.equal(dropPositionAt(105, box, 'folder'), 'before');
  assert.equal(dropPositionAt(113, box, 'folder'), 'inside');
  assert.equal(dropPositionAt(124, box, 'folder'), 'after');
  assert.equal(dropPositionAt(124, box, 'folder', true), 'inside');
  assert.equal(dropPositionAt(500, box, 'request'), 'after');
  assert.equal(dropPositionAt(-20, box, 'request'), 'before');
  assert.equal(dropPositionAt(100, { top: 100, height: 0 }, 'request'), 'after');
});

test('ef_col_01_sous_un_filtre_seul_le_depot_dans_un_dossier_reste_possible', () => {
  const box = { top: 0, height: 26 };
  for (const y of [1, 13, 25]) {
    assert.equal(dropPositionAt(y, box, 'folder', false, false), 'inside');
    assert.equal(dropPositionAt(y, box, 'request', false, false), null);
  }
  assert.equal(dropPosition(0.1, 'folder', false, false), 'inside');
});

test('ef_col_01_glisser_a_quitte_la_zone_quand_le_pointeur_sort_du_rectangle', () => {
  const rect = { left: 44, top: 80, right: 314, bottom: 600 };
  assert.equal(insideRect(100, 200, rect), true);
  assert.equal(insideRect(44, 80, rect), true);
  assert.equal(insideRect(314, 200, rect), false);
  assert.equal(insideRect(100, 600, rect), false);
  assert.equal(insideRect(0, 0, rect), false);
});

test('ef_col_04_nom_de_fichier_suit_sanitize_name_du_moteur', () => {
  assert.equal(sanitizeName('-brouillon'), 'brouillon');
  assert.equal(sanitizeName('  - -x'), 'x');
  assert.equal(sanitizeName('GET /transactions/:id'), 'GET -transactions--id');
  assert.equal(sanitizeName('/liste'), 'liste');
  assert.equal(sanitizeName('Liste. '), 'Liste');
  assert.equal(sanitizeName('.env'), '.env');
  assert.equal(sanitizeName('a-b'), 'a-b');
});

test('ef_col_04_seq_de_fin_de_liste_depasse_le_plus_grand_seq_et_le_nombre_de_freres', () => {
  assert.equal(endSeq([]), 1);
  assert.equal(endSeq([1, 2, 3]), 4);
  assert.equal(endSeq([1, 7]), 8);
  assert.equal(endSeq([0, 0, 0]), 4);
  assert.equal(endSeq([2, 5, 1, 1, 1, 1, 1]), 8);
});

test('ef_col_04_chemins_disparus_de_larbre_sont_reperes_apres_un_echec', () => {
  assert.deepEqual(missingPaths(tree, ['auth/jeton.yml', 'auth/ancien.yml', 'sante.yml', 'transactions/export']), ['auth/ancien.yml']);
  assert.deepEqual(missingPaths(tree, []), []);
});
