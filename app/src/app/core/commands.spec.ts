import assert from 'node:assert/strict';
import { test } from 'node:test';

import { type Command, Commands, matchesShortcut, shortcutLabel } from './commands.ts';

function press(key: string, mods: { meta?: boolean; ctrl?: boolean; shift?: boolean; alt?: boolean } = {}) {
  return {
    key,
    metaKey: !!mods.meta,
    ctrlKey: !!mods.ctrl,
    shiftKey: !!mods.shift,
    altKey: !!mods.alt,
    prevented: false,
    preventDefault() {
      this.prevented = true;
    },
  };
}

function command(id: string, extra: Partial<Command> = {}): Command & { runs: number } {
  const c = { id, title: id, group: 'Test', icon: 'x', runs: 0, run: () => c.runs++, ...extra };
  return c;
}

test('ef_ux_01_raccourci_affiche_cmd_sur_macos_et_ctrl_ailleurs', () => {
  assert.equal(shortcutLabel('mod+k', true), '⌘K');
  assert.equal(shortcutLabel('mod+k', false), 'Ctrl+K');
  assert.equal(shortcutLabel('mod+shift+p', true), '⌘⇧P');
  assert.equal(shortcutLabel('mod+shift+p', false), 'Ctrl+Maj+P');
  assert.equal(shortcutLabel('mod+enter', false), 'Ctrl+↵');
  assert.equal(shortcutLabel('mod+\\', true), '⌘\\');
  assert.equal(shortcutLabel('esc', true), 'Échap');
});

test('ef_ux_01_raccourci_correspond_a_la_touche_et_aux_modificateurs', () => {
  assert.ok(matchesShortcut('mod+k', press('k', { meta: true }), true));
  assert.ok(matchesShortcut('mod+k', press('k', { ctrl: true }), false));
  assert.ok(!matchesShortcut('mod+k', press('k', { ctrl: true }), true));
  assert.ok(!matchesShortcut('mod+k', press('k'), true));
  assert.ok(!matchesShortcut('mod+k', press('K', { meta: true, shift: true }), true));
  assert.ok(matchesShortcut('mod+shift+p', press('P', { meta: true, shift: true }), true));
  assert.ok(matchesShortcut('mod+enter', press('Enter', { ctrl: true }), false));
  assert.ok(matchesShortcut('esc', press('Escape'), true));
  assert.ok(matchesShortcut('mod+\\', press('\\', { meta: true, alt: true, shift: true }), true));
});

test('ef_ux_01_registre_execute_la_commande_active_liee_a_la_touche', () => {
  const commands = new Commands();
  let ready = false;
  const save = command('save', { keys: 'mod+s', when: () => ready });
  commands.register(save);

  const inactive = press('s', { meta: true });
  assert.equal(commands.dispatch(inactive, true), false);
  assert.equal(save.runs, 0);
  assert.equal(inactive.prevented, false);

  ready = true;
  const active = press('s', { meta: true });
  assert.equal(commands.dispatch(active, true), true);
  assert.equal(save.runs, 1);
  assert.equal(active.prevented, true);

  assert.equal(commands.dispatch(press('x', { meta: true }), true), false);
});

test('ef_ux_01_registre_ajoute_en_une_ligne_remplace_par_id_et_retire', () => {
  const commands = new Commands();
  commands.register(command('a'), command('b'));
  const remove = commands.register(command('openapi.import', { title: 'Importer une spec OpenAPI…' }));
  commands.register(command('a', { title: 'A2' }));
  assert.deepEqual(
    commands.all().map((c) => c.title),
    ['b', 'Importer une spec OpenAPI…', 'A2'],
  );
  remove();
  assert.deepEqual(
    commands.all().map((c) => c.id),
    ['b', 'a'],
  );
  assert.equal(commands.enabled(command('c', { when: () => false })), false);
  assert.equal(commands.enabled(command('d')), true);
});
