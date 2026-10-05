import assert from 'node:assert/strict';
import { test } from 'node:test';

import { affectedTabs, describeDiskChange, mergeChanges, touchesEnvironments, verdictFor } from './disk-sync.ts';
import type { DiskChange } from './model.ts';

const change = (paths: string[], truncated = false, root = '/c'): DiskChange => ({ root, paths, truncated });

test('ef_col_03_a_tab_without_draft_adopts_the_file_that_changed_on_disk', () => {
  assert.equal(verdictFor('v1', 'v1', 'v1', 'v2'), 'adopt');
});

test('ef_col_03_a_tab_with_a_draft_is_marked_stale_and_keeps_the_draft_when_the_disk_changed', () => {
  assert.equal(verdictFor('v1', 'v1', 'brouillon', 'v2'), 'stale');
});

test('ef_col_03_our_own_save_is_not_a_change_even_when_typing_went_on_meanwhile', () => {
  assert.equal(verdictFor('v1', 'v1', 'v1', 'v1'), 'unchanged');
  assert.equal(verdictFor('v1', 'v1', 'brouillon', 'v1'), 'unchanged');
});

test('ef_col_03_the_disk_is_compared_to_what_rust_read_not_to_the_document_the_interface_built', () => {
  const rust = '{"name":"b","value":"","kind":"query","enabled":true}';
  const interfaceForm = '{"name":"b","value":"","enabled":true,"kind":"query","description":null}';
  assert.notEqual(rust, interfaceForm);
  assert.equal(verdictFor(rust, interfaceForm, interfaceForm, rust), 'unchanged');
  assert.equal(verdictFor(rust, interfaceForm, `${interfaceForm} `, rust), 'unchanged');
});

test('ef_col_03_a_colleague_making_the_same_edit_as_the_draft_is_not_a_conflict', () => {
  assert.equal(verdictFor('v1', 'v1', 'v2', 'v2'), 'adopt');
});

test('ef_col_03_only_the_open_tabs_whose_file_changed_are_reread_unless_the_batch_is_truncated', () => {
  const open = ['a.yml', 'sous/b.yml', 'c.yml'];
  assert.deepEqual(affectedTabs(change(['sous/b.yml', 'autre.yml']), open), ['sous/b.yml']);
  assert.deepEqual(affectedTabs(change([]), open), []);
  assert.deepEqual(affectedTabs(change([], true), open), open);
});

test('ef_col_03_a_folder_that_moved_reaches_the_tabs_inside_it_but_not_its_namesakes', () => {
  const open = ['A/x.yml', 'A/B/y.yml', 'AA/z.yml', 'A.yml'];
  assert.deepEqual(affectedTabs(change(['A', 'A.old']), open), ['A/x.yml', 'A/B/y.yml']);
  assert.deepEqual(affectedTabs(change(['A/B']), open), ['A/B/y.yml']);
});

test('ef_col_03_environment_files_and_the_local_env_file_trigger_a_variables_reload', () => {
  assert.equal(touchesEnvironments(change(['environments/dev.yml'])), true);
  assert.equal(touchesEnvironments(change(['.env'])), true);
  assert.equal(touchesEnvironments(change(['.env.local'])), true);
  assert.equal(touchesEnvironments(change(['sous/.env'])), true);
  assert.equal(touchesEnvironments(change([], true)), true);
  assert.equal(touchesEnvironments(change(['Auth/Connexion.yml', 'environment.yml', 'envs/dev.yml'])), false);
});

test('ef_col_03_batches_received_while_one_is_being_handled_are_merged', () => {
  assert.deepEqual(mergeChanges(null, change(['a.yml'])), change(['a.yml']));
  assert.deepEqual(mergeChanges(change(['b.yml', 'a.yml']), change(['a.yml', 'c.yml'])), change(['b.yml', 'a.yml', 'c.yml']));
  assert.equal(mergeChanges(change(['a.yml']), change([], true)).truncated, true);
  assert.equal(mergeChanges(change([], true), change(['a.yml'])).truncated, true);
  assert.deepEqual(mergeChanges(change(['a.yml'], false, '/ancienne'), change(['x.yml'])), change(['x.yml']));
});

test('ef_col_03_the_user_is_told_only_what_needs_attention', () => {
  assert.equal(describeDiskChange({ closed: [], reloaded: 0, stale: [] }), null);
  assert.equal(describeDiskChange({ closed: [], reloaded: 1, stale: [] }), 'Une requête ouverte a été relue depuis le disque.');
  assert.equal(describeDiskChange({ closed: [], reloaded: 3, stale: [] }), '3 requêtes ouvertes ont été relues depuis le disque.');
  assert.equal(describeDiskChange({ closed: ['Connexion'], reloaded: 0, stale: [] }), '« Connexion » est fermée : son fichier a été supprimé.');
  assert.equal(describeDiskChange({ closed: ['a', 'b'], reloaded: 0, stale: [] }), '2 onglets fermés : leurs fichiers ont été supprimés.');
  assert.equal(describeDiskChange({ closed: [], reloaded: 0, stale: ['Connexion'] }), '« Connexion » a changé sur le disque : ton brouillon est gardé.');
  assert.equal(describeDiskChange({ closed: [], reloaded: 0, stale: ['a', 'b'] }), '2 requêtes ont changé sur le disque : tes brouillons sont gardés.');
  const all = describeDiskChange({ closed: ['x'], reloaded: 1, stale: ['y'] });
  assert.ok(all?.includes('« x » est fermée') && all.includes('relue') && all.includes('« y » a changé'));
});
