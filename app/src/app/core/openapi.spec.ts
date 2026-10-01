import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { SpecSummary } from './model.ts';
import { importedMessage, specLabel } from './openapi.ts';

const summary = (extra: Partial<SpecSummary> = {}): SpecSummary => ({
  title: 'Petstore',
  version: '1.0.0',
  format: 'openapi',
  formatVersion: '3.0.3',
  operationCount: 19,
  tags: [],
  servers: [],
  ...extra,
});

test('ef_imp_02_apercu_nomme_le_format_et_sa_version', () => {
  assert.equal(specLabel(summary()), 'OpenAPI 3.0.3');
  assert.equal(specLabel(summary({ format: 'openapi', formatVersion: '3.1.0' })), 'OpenAPI 3.1.0');
  assert.equal(specLabel(summary({ format: 'swagger', formatVersion: '2.0' })), 'Swagger 2.0');
});

test('ef_imp_02_apercu_sans_numero_de_version_de_format', () => {
  assert.equal(specLabel(summary({ formatVersion: null })), 'OpenAPI');
  assert.equal(specLabel(summary({ format: 'swagger', formatVersion: undefined })), 'Swagger');
});

test('ef_imp_02_message_apres_import_accorde_le_nombre_de_requetes', () => {
  assert.equal(importedMessage(19), 'Collection importée : 19 requêtes');
  assert.equal(importedMessage(1), 'Collection importée : 1 requête');
  assert.equal(importedMessage(0), 'Collection importée : 0 requête');
});
