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
  findItem,
  isUnder,
  neighbourOf,
  remapPath,
  remapPaths,
  remapSet,
  reorderMove,
  treeKeyAction,
  unsavedMessage,
  validateName,
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
  assert.equal(treeKeyAction(key('ArrowDown'), true), null);
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
