import assert from 'node:assert/strict';
import { test } from 'node:test';

import { inline, parseMarkdown, safeHref } from './markdown.ts';

test('ef_doc_01_les_titres_vont_de_un_a_six_diezes', () => {
  const blocks = parseMarkdown('# Un\n\n###### Six\n\n####### Sept');
  assert.deepEqual(blocks.map((b) => b.kind === 'heading' && b.level), [1, 6, false]);
  assert.equal(blocks[2].kind, 'paragraph');
});

test('ef_doc_01_un_paragraphe_sur_plusieurs_lignes_se_recolle_avec_une_espace', () => {
  assert.deepEqual(parseMarkdown('a\nb'), [{ kind: 'paragraph', children: [{ kind: 'text', text: 'a b' }] }]);
});

test('ef_doc_01_deux_espaces_en_fin_de_ligne_font_un_retour', () => {
  assert.deepEqual(inline('a  \nb'), [{ kind: 'text', text: 'a' }, { kind: 'break' }, { kind: 'text', text: 'b' }]);
});

test('ef_doc_01_gras_italique_barre_et_code_en_ligne', () => {
  assert.deepEqual(inline('**g** *i* ~~b~~ `c`').map((n) => n.kind), ['strong', 'text', 'em', 'text', 'del', 'text', 'code']);
});

test('ef_doc_01_les_traits_bas_dans_un_mot_ne_sont_pas_de_l_italique', () => {
  assert.deepEqual(inline('snake_case_name'), [{ kind: 'text', text: 'snake_case_name' }]);
});

test('ef_doc_01_un_marqueur_sans_fermeture_reste_du_texte', () => {
  assert.deepEqual(inline('2 * 3'), [{ kind: 'text', text: '2 * 3' }]);
  assert.deepEqual(inline('**ouvert'), [{ kind: 'text', text: '**ouvert' }]);
});

test('ef_doc_01_le_code_en_ligne_garde_son_contenu_tel_quel', () => {
  assert.deepEqual(inline('`a **b** <i>`'), [{ kind: 'code', text: 'a **b** <i>' }]);
});

test('ef_doc_01_une_barre_inverse_echappe_le_marqueur', () => {
  assert.deepEqual(inline('\\*pas\\*'), [{ kind: 'text', text: '*pas*' }]);
});

test('ef_doc_01_un_bloc_de_code_clos_garde_les_lignes_et_la_langue', () => {
  const [code] = parseMarkdown('```json\n{ "a": 1 }\n\n# pas un titre\n```');
  assert.deepEqual(code, { kind: 'code', language: 'json', text: '{ "a": 1 }\n\n# pas un titre' });
});

test('ef_doc_01_un_bloc_de_code_jamais_clos_va_jusqu_a_la_fin', () => {
  const [code] = parseMarkdown('```\nligne 1\nligne 2');
  assert.deepEqual(code, { kind: 'code', language: '', text: 'ligne 1\nligne 2' });
});

test('ef_doc_01_les_listes_a_puces_et_numerotees', () => {
  const [bullets, numbers] = parseMarkdown('- a\n- b\n\n3. x\n4. y');
  assert.equal(bullets.kind === 'list' && bullets.ordered, false);
  assert.equal(bullets.kind === 'list' && bullets.items.length, 2);
  assert.equal(numbers.kind === 'list' && numbers.ordered, true);
  assert.equal(numbers.kind === 'list' && numbers.start, 3);
});

test('ef_doc_01_une_liste_imbriquee_par_indentation', () => {
  const [list] = parseMarkdown('- a\n  - a1\n  - a2\n- b');
  assert.equal(list.kind, 'list');
  if (list.kind !== 'list') return;
  assert.equal(list.items.length, 2);
  const nested = list.items[0][1];
  assert.equal(nested.kind === 'list' && nested.items.length, 2);
});

test('ef_doc_01_une_citation_contient_des_blocs', () => {
  const [quote] = parseMarkdown('> # Note\n> texte');
  assert.equal(quote.kind, 'quote');
  if (quote.kind === 'quote') assert.deepEqual(quote.children.map((b) => b.kind), ['heading', 'paragraph']);
});

test('ef_doc_01_un_tableau_avec_alignements', () => {
  const [table] = parseMarkdown('| Nom | Code |\n|:--|--:|\n| a | 1 |\n| b \\| c | 2 |');
  assert.equal(table.kind, 'table');
  if (table.kind !== 'table') return;
  assert.deepEqual(table.align, ['left', 'right']);
  assert.equal(table.rows.length, 2);
  assert.deepEqual(table.rows[1][0], [{ kind: 'text', text: 'b | c' }]);
});

test('ef_doc_01_une_ligne_avec_une_barre_sans_separateur_reste_un_paragraphe', () => {
  assert.equal(parseMarkdown('a | b')[0].kind, 'paragraph');
});

test('ef_doc_01_la_regle_horizontale', () => {
  assert.deepEqual(parseMarkdown('---\n***\n___').map((b) => b.kind), ['rule', 'rule', 'rule']);
});

test('ef_doc_01_seuls_http_https_et_mailto_sont_des_liens', () => {
  assert.equal(safeHref('https://x.test/a'), 'https://x.test/a');
  assert.equal(safeHref('mailto:a@x.test'), 'mailto:a@x.test');
  assert.equal(safeHref('javascript:alert(1)'), null);
  assert.equal(safeHref(' JaVaScRiPt:alert(1)'), null);
  assert.equal(safeHref('data:text/html;base64,AA=='), null);
  assert.equal(safeHref('../autre.md'), null);
});

test('ef_doc_01_un_lien_garde_son_texte_et_son_adresse', () => {
  assert.deepEqual(inline('[la **doc**](https://x.test)'), [
    { kind: 'link', href: 'https://x.test', children: [{ kind: 'text', text: 'la ' }, { kind: 'strong', children: [{ kind: 'text', text: 'doc' }] }] },
  ]);
});

test('ef_doc_01_le_html_brut_reste_du_texte', () => {
  const [p] = parseMarkdown('<script>alert(1)</script> <b>x</b>');
  assert.deepEqual(p, { kind: 'paragraph', children: [{ kind: 'text', text: '<script>alert(1)</script> <b>x</b>' }] });
});

test('ef_doc_01_les_fins_de_ligne_windows_sont_acceptees', () => {
  assert.deepEqual(parseMarkdown('# A\r\n\r\ntexte\r\nsuite').map((b) => b.kind), ['heading', 'paragraph']);
});

test('ef_doc_01_un_texte_vide_ne_donne_aucun_bloc', () => {
  assert.deepEqual(parseMarkdown(''), []);
  assert.deepEqual(parseMarkdown('\n  \n'), []);
});
