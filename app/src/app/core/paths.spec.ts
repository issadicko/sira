import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { TreeItem } from './model.ts';
import { dirname, folderChoices, isInsideCollection, joinPath, relativeToRoot } from './paths.ts';

const request = (path: string): TreeItem => ({ kind: 'request', path, name: path, method: 'GET', requestType: 'http', url: '', deprecated: false });
const folder = (path: string, name: string, children: TreeItem[] = []): TreeItem => ({ kind: 'folder', path, name, children });

test('ef_imp_01_liste_les_dossiers_de_la_collection_avec_leur_profondeur', () => {
  const items = [
    request('racine.yml'),
    folder('auth', 'Auth', [request('auth/connexion.yml')]),
    folder('transactions', 'Transactions', [folder('transactions/export', 'Export', [request('transactions/export/csv.yml')])]),
  ];
  assert.deepEqual(folderChoices(items), [
    { path: 'auth', label: 'Auth', depth: 0 },
    { path: 'transactions', label: 'Transactions', depth: 0 },
    { path: 'transactions/export', label: 'Export', depth: 1 },
  ]);
});

test('ef_imp_01_dossier_parent_dun_chemin_relatif', () => {
  assert.equal(dirname('transactions/export/csv.yml'), 'transactions/export');
  assert.equal(dirname('auth/connexion.yml'), 'auth');
  assert.equal(dirname('racine.yml'), '');
  assert.equal(dirname(''), '');
});

test('ef_imp_02_chemin_du_dossier_cree_garde_le_separateur_du_systeme', () => {
  assert.equal(joinPath('/Users/issa/projets', 'Petstore'), '/Users/issa/projets/Petstore');
  assert.equal(joinPath('/Users/issa/projets/', 'Petstore'), '/Users/issa/projets/Petstore');
  assert.equal(joinPath('C:\\Users\\issa\\projets', 'Petstore'), 'C:\\Users\\issa\\projets\\Petstore');
  assert.equal(joinPath('C:\\', 'Petstore'), 'C:\\Petstore');
  assert.equal(joinPath('/', 'Petstore'), '/Petstore');
});

test('ef_req_02_fichier_de_la_collection_devient_un_chemin_relatif_en_slash', () => {
  assert.equal(relativeToRoot('/Users/issa/api', '/Users/issa/api/pieces/recu.pdf'), 'pieces/recu.pdf');
  assert.equal(relativeToRoot('/Users/issa/api/', '/Users/issa/api/recu.pdf'), 'recu.pdf');
  assert.equal(relativeToRoot('C:\\Users\\issa\\api', 'C:\\Users\\issa\\api\\pieces\\recu.pdf'), 'pieces/recu.pdf');
  assert.equal(relativeToRoot('C:\\Users\\issa\\api', 'c:\\users\\ISSA\\api\\recu.pdf'), 'recu.pdf');
});

test('ef_req_02_fichier_hors_de_la_collection_est_refuse', () => {
  assert.equal(relativeToRoot('/Users/issa/api', '/Users/issa/autre/recu.pdf'), null);
  assert.equal(relativeToRoot('/Users/issa/api', '/Users/issa/api-bis/recu.pdf'), null);
  assert.equal(relativeToRoot('/Users/issa/api', '/Users/issa/api'), null);
  assert.equal(relativeToRoot('/Users/issa/api', '/Users/issa'), null);
  assert.equal(relativeToRoot('/Users/issa/api', '/Users/issa/api/../secret.txt'), null);
  assert.equal(relativeToRoot('/Users/issa/api', '/Users/issa/api/pieces/../../secret.txt'), null);
});

test('ef_req_02_la_casse_compte_hors_windows', () => {
  assert.equal(relativeToRoot('/Users/issa/api', '/users/issa/api/recu.pdf'), null);
});

test('ef_req_02_chemin_accepte_par_le_moteur_est_relatif_sans_remontee', () => {
  assert.ok(isInsideCollection('pieces/recu.pdf'));
  assert.ok(isInsideCollection('recu.pdf'));
  assert.ok(isInsideCollection('pieces/v1..2/recu.pdf'));
});

test('ef_req_02_chemin_refuse_par_le_moteur_est_absolu_ou_remonte', () => {
  for (const path of ['/etc/passwd', 'C:\\Users\\issa\\recu.pdf', 'c:/recu.pdf', '\\\\serveur\\partage\\recu.pdf', '../partage/recu.pdf', 'pieces/../../recu.pdf', 'pieces\\..\\recu.pdf']) {
    assert.equal(isInsideCollection(path), false, path);
  }
});

test('ef_col_01_dossiers_proposes_au_deplacement_excluent_lelement_et_ses_descendants', () => {
  const items = [
    request('racine.yml'),
    folder('auth', 'Auth', [request('auth/connexion.yml')]),
    folder('transactions', 'Transactions', [folder('transactions/export', 'Export', [folder('transactions/export/archives', 'Archives')])]),
  ];
  assert.deepEqual(folderChoices(items, 0, 'transactions'), [{ path: 'auth', label: 'Auth', depth: 0 }]);
  assert.deepEqual(
    folderChoices(items, 0, 'transactions/export').map((f) => f.path),
    ['auth', 'transactions'],
  );
  assert.deepEqual(folderChoices(items, 0, 'auth/connexion.yml'), folderChoices(items));
  assert.deepEqual(folderChoices(items, 0, 'inconnu'), folderChoices(items));
});
