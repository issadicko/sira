import assert from 'node:assert/strict';
import { test } from 'node:test';

import { ageLabel, describeSchema, formatVariables, graphqlParts, stripComments, summarize, withGraphql } from './graphql.ts';
import { switchBody } from './body.ts';
import type { RequestDoc } from './model.ts';
import { isRunnable } from './runner.ts';

const doc = (extra: Partial<RequestDoc> = {}): RequestDoc => ({
  name: 'Produit',
  requestType: 'graphql',
  seq: 1,
  method: 'POST',
  url: '{{baseUrl}}/graphql',
  params: [],
  headers: [],
  body: { type: 'none' },
  auth: { type: 'inherit' },
  assertions: [],
  variables: [],
  scripts: [],
  ...extra,
});

const INTROSPECTION = {
  __schema: {
    queryType: { name: 'Query' },
    mutationType: { name: 'Mutation' },
    subscriptionType: null,
    types: [
      { name: 'Query', fields: [{ name: 'product' }, { name: 'products' }] },
      { name: 'Mutation', fields: [{ name: 'addCartItem' }] },
      { name: 'Product', fields: [{ name: 'name' }] },
      { name: 'String', fields: null },
      { name: '__Schema', fields: [{ name: 'types' }] },
    ],
  },
};

test('ef_gql_01_une_requete_sans_corps_a_une_requete_et_des_variables_vides', () => {
  assert.deepEqual(graphqlParts({ type: 'none' }), { query: '', variables: '' });
  assert.deepEqual(graphqlParts({ type: 'graphql', query: '{ me }', variables: '{}' }), { query: '{ me }', variables: '{}' });
});

test('ef_gql_01_modifier_la_requete_garde_les_variables', () => {
  const start = doc({ body: { type: 'graphql', query: '{ a }', variables: '{"x":1}' } });
  assert.deepEqual(withGraphql(start, { query: '{ b }' }).body, { type: 'graphql', query: '{ b }', variables: '{"x":1}' });
  assert.deepEqual(withGraphql(doc(), { variables: '{"x":1}' }).body, { type: 'graphql', query: '', variables: '{"x":1}' });
});

test('ef_gql_01_le_corps_graphql_ne_change_pas_de_type_depuis_le_selecteur', () => {
  const body = { type: 'graphql', query: '{ a }', variables: '' } as const;
  assert.equal(switchBody(body, 'json'), body);
});

test('ef_gql_01_les_requetes_graphql_se_lancent_pas_les_autres_protocoles', () => {
  assert.ok(isRunnable('http') && isRunnable('graphql'));
  assert.ok(!isRunnable('grpc') && !isRunnable('websocket'));
});

test('ef_gql_01_resume_du_schema_sans_les_types_d_introspection', () => {
  const summary = summarize(INTROSPECTION);
  assert.deepEqual(summary, { types: 4, queries: 2, mutations: 1, subscriptions: 0 });
  assert.equal(describeSchema(summary!), '4 types · 2 requêtes · 1 mutation');
  assert.equal(summarize({}), null);
  assert.equal(summarize(null), null);
});

test('ef_gql_01_ages_du_schema_en_francais', () => {
  const now = Date.parse('2026-10-06T12:00:00Z');
  assert.equal(ageLabel('2026-10-06T11:59:40Z', now), "à l'instant");
  assert.equal(ageLabel('2026-10-06T11:55:00Z', now), 'il y a 5 min');
  assert.equal(ageLabel('2026-10-06T09:00:00Z', now), 'il y a 3 h');
  assert.equal(ageLabel('2026-10-04T12:00:00Z', now), 'il y a 2 j');
  assert.equal(ageLabel('pas une date', now), '');
});

test('ef_gql_01_les_variables_acceptent_les_commentaires_comme_le_moteur', () => {
  assert.equal(stripComments('{ // note\n "u": "http://x.test", /* b */ "n": 1 }'), '{ \n "u": "http://x.test",  "n": 1 }');
  assert.deepEqual(formatVariables('{"a":1,"b":[1,2]}'), { ok: true, text: '{\n  "a": 1,\n  "b": [\n    1,\n    2\n  ]\n}' });
  assert.deepEqual(formatVariables('  '), { ok: true, text: '' });
  assert.equal(formatVariables('{ pas du json }').ok, false);
});

test('ef_gql_01_le_formatage_des_variables_ne_retire_jamais_un_commentaire', () => {
  const result = formatVariables('{"a":1, // note\n"b":2}');
  assert.equal(result.ok, false);
  assert.match(!result.ok ? result.reason : '', /commentaires/);
});
