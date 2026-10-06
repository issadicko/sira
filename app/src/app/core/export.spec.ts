import assert from 'node:assert/strict';
import { test } from 'node:test';

import { exportFileName, exportSummary } from './export.ts';

test('ef_imp_03_le_fichier_propose_porte_le_nom_de_la_collection_et_le_suffixe_du_format', () => {
  assert.equal(exportFileName('Paiements', 'postman'), 'Paiements.postman_collection.json');
  assert.equal(exportFileName('Paiements', 'openapi'), 'Paiements.openapi.json');
});

test('ef_imp_03_le_nom_propose_garde_les_accents_et_remplace_le_reste_par_des_tirets', () => {
  assert.equal(exportFileName("  Boutique d'été / v2  ", 'openapi'), 'Boutique-d-été-v2.openapi.json');
  assert.equal(exportFileName('../../etc', 'postman'), 'etc.postman_collection.json');
});

test('ef_imp_03_une_collection_sans_lettre_ni_chiffre_se_nomme_collection', () => {
  assert.equal(exportFileName('', 'openapi'), 'collection.openapi.json');
  assert.equal(exportFileName('***', 'postman'), 'collection.postman_collection.json');
});

test('ef_imp_03_le_resume_compte_les_elements_non_exportes', () => {
  assert.equal(exportSummary(0), 'Collection exportée');
  assert.equal(exportSummary(1), 'Collection exportée · 1 élément non exporté');
  assert.equal(exportSummary(3), 'Collection exportée · 3 éléments non exportés');
});
