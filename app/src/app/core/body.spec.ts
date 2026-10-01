import assert from 'node:assert/strict';
import { test } from 'node:test';

import { switchBody, withFile } from './body.ts';
import type { Body, MultipartField } from './model.ts';

const multipart: Body = {
  type: 'multipart-form',
  fields: [
    { name: 'motif', kind: 'text', value: 'Reçu', enabled: true, description: 'Pourquoi' },
    { name: 'justificatif', kind: 'file', value: ['pieces/recu.pdf'], enabled: true, contentType: 'application/pdf' },
    { name: 'note', kind: 'text', value: '', enabled: false },
  ],
};

test('ef_req_02_propose_les_deux_types_de_formulaire', () => {
  assert.deepEqual(switchBody({ type: 'none' }, 'form-urlencoded'), { type: 'form-urlencoded', fields: [] });
  assert.deepEqual(switchBody({ type: 'none' }, 'multipart-form'), { type: 'multipart-form', fields: [] });
});

test('ef_req_02_formulaire_vers_multipart_conserve_les_champs_en_texte', () => {
  const form: Body = {
    type: 'form-urlencoded',
    fields: [{ name: 'grant_type', value: 'client_credentials', enabled: true, description: 'Flux OAuth' }, { name: 'scope', value: 'a b', enabled: false }],
  };
  assert.deepEqual(switchBody(form, 'multipart-form'), {
    type: 'multipart-form',
    fields: [
      { name: 'grant_type', kind: 'text', value: 'client_credentials', enabled: true, description: 'Flux OAuth' },
      { name: 'scope', kind: 'text', value: 'a b', enabled: false, description: null },
    ],
  });
});

test('ef_req_02_multipart_vers_formulaire_garde_les_textes_et_abandonne_les_fichiers', () => {
  assert.deepEqual(switchBody(multipart, 'form-urlencoded'), {
    type: 'form-urlencoded',
    fields: [
      { name: 'motif', value: 'Reçu', enabled: true, description: 'Pourquoi' },
      { name: 'note', value: '', enabled: false, description: null },
    ],
  });
});

test('ef_req_02_changer_vers_le_meme_type_ne_touche_a_rien', () => {
  assert.equal(switchBody(multipart, 'multipart-form'), multipart);
});

test('ef_req_02_le_texte_passe_dun_type_texte_a_lautre_et_sefface_pour_les_formulaires', () => {
  const json: Body = { type: 'json', data: '{"a":1}' };
  assert.deepEqual(switchBody(json, 'xml'), { type: 'xml', data: '{"a":1}' });
  assert.deepEqual(switchBody(json, 'form-urlencoded'), { type: 'form-urlencoded', fields: [] });
  assert.deepEqual(switchBody(multipart, 'json'), { type: 'json', data: '{\n  \n}' });
  assert.deepEqual(switchBody(multipart, 'text'), { type: 'text', data: '' });
  assert.deepEqual(switchBody(json, 'none'), { type: 'none' });
});

test('ef_req_02_un_corps_non_editable_nest_jamais_remplace', () => {
  const graphql: Body = { type: 'other', label: 'GraphQL' };
  for (const type of ['none', 'json', 'text', 'xml', 'form-urlencoded', 'multipart-form'] as const) {
    assert.equal(switchBody(graphql, type), graphql);
  }
});

test('ef_req_02_ajoute_un_fichier_au_champ_choisi', () => {
  const fields = multipart.type === 'multipart-form' ? multipart.fields : [];
  const result = withFile(fields, 1, 'pieces/photo.jpg');
  assert.deepEqual((result[1] as Extract<MultipartField, { kind: 'file' }>).value, ['pieces/recu.pdf', 'pieces/photo.jpg']);
  assert.equal(result[0], fields[0]);
  assert.equal(result[2], fields[2]);
});

test('ef_req_02_ne_duplique_pas_un_fichier_et_ignore_un_champ_texte', () => {
  const fields = multipart.type === 'multipart-form' ? multipart.fields : [];
  assert.deepEqual(withFile(fields, 1, 'pieces/recu.pdf'), fields);
  assert.deepEqual(withFile(fields, 0, 'pieces/photo.jpg'), fields);
  assert.deepEqual(withFile(fields, 9, 'pieces/photo.jpg'), fields);
});
