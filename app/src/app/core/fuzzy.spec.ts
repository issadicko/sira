import assert from 'node:assert/strict';
import { test } from 'node:test';

import { fuzzy, rank, segments } from './fuzzy.ts';

const order = (items: string[], query: string) => rank(items, query, (t) => [t]).map((r) => r.item);

test('ef_ux_01_recherche_floue_trouve_une_sous_sequence_sans_casse_ni_accents', () => {
  assert.deepEqual(fuzzy("Détail d'une transaction", 'DETAIL')?.marks, [0, 1, 2, 3, 4, 5]);
  assert.deepEqual(fuzzy('Liste des transactions', 'lst')?.marks, [0, 2, 3]);
  assert.deepEqual(fuzzy('Liste des transactions', 'liste trans')?.marks, [0, 1, 2, 3, 4, 10, 11, 12, 13, 14]);
  assert.equal(fuzzy('Connexion', 'xz'), null);
  assert.equal(fuzzy('abc', 'abcd'), null);
});

test('ef_ux_01_recherche_floue_requete_vide_garde_tout_sans_surlignage', () => {
  assert.deepEqual(fuzzy('Envoyer la requête', '  '), { score: 0, marks: [] });
  assert.deepEqual(order(['b', 'a', 'c'], ''), ['b', 'a', 'c']);
});

test('ef_ux_01_recherche_floue_prefere_le_prefixe', () => {
  assert.deepEqual(order(['Gérer les environnements', 'Envoyer la requête'], 'env'), [
    'Envoyer la requête',
    'Gérer les environnements',
  ]);
});

test('ef_ux_01_recherche_floue_prefere_les_debuts_de_mot', () => {
  assert.deepEqual(fuzzy('Liste des transactions', 'ldt')?.marks, [0, 6, 10]);
  assert.deepEqual(fuzzy('getUserById', 'gubi')?.marks, [0, 3, 7, 9]);
  assert.deepEqual(order(['Info', "Fermer l'onglet"], 'fo'), ["Fermer l'onglet", 'Info']);
});

test('ef_ux_01_recherche_floue_prefere_la_contiguite', () => {
  assert.deepEqual(fuzzy('transactions/annuler.yml', 'ann')?.marks, [13, 14, 15]);
  assert.deepEqual(order(['a-n-n-u-l-e-r', 'annuler'], 'annul'), ['annuler', 'a-n-n-u-l-e-r']);
});

test('ef_ux_01_classement_retient_le_meilleur_champ_et_favorise_le_premier', () => {
  const items = [
    { name: 'Connexion', url: '/auth/connexion' },
    { name: 'Liste des transactions', url: '/transactions?page=1' },
  ];
  const ranked = rank(items, 'auth', (r) => [r.name, r.url]);
  assert.equal(ranked.length, 1);
  assert.equal(ranked[0].field, 1);
  assert.deepEqual(ranked[0].marks, [1, 2, 3, 4]);
  assert.equal(rank(items, 'connexion', (r) => [r.name, r.url])[0].field, 0);
});

test('ef_ux_01_segments_regroupent_les_caracteres_surlignes', () => {
  assert.deepEqual(segments('Envoyer', [0, 1, 4]), [
    { text: 'En', hit: true },
    { text: 'vo', hit: false },
    { text: 'y', hit: true },
    { text: 'er', hit: false },
  ]);
  assert.deepEqual(segments('abc'), [{ text: 'abc', hit: false }]);
  assert.deepEqual(segments(''), []);
});
