import assert from 'node:assert/strict';
import { test } from 'node:test';

import { type Command, Commands, matchesShortcut, shortcutLabel } from './commands.ts';

interface Mods {
  meta?: boolean;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  altGraph?: boolean;
  repeat?: boolean;
}

function press(key: string, mods: Mods = {}) {
  return {
    key,
    metaKey: !!mods.meta,
    ctrlKey: !!mods.ctrl,
    shiftKey: !!mods.shift,
    altKey: !!mods.alt,
    repeat: !!mods.repeat,
    getModifierState: (name: string) => name === 'AltGraph' && !!mods.altGraph,
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

test('ef_ux_01_altgr_hors_macos_ne_declenche_jamais_un_raccourci_mod', () => {
  assert.ok(matchesShortcut('mod+\\', press('\\', { ctrl: true }), false));
  assert.ok(!matchesShortcut('mod+\\', press('\\', { ctrl: true, alt: true }), false));
  assert.ok(!matchesShortcut('mod+\\', press('\\', { ctrl: true, altGraph: true }), false));
  assert.ok(!matchesShortcut('mod+\\', press('\\', { altGraph: true }), false));
  assert.ok(!matchesShortcut('mod+k', press('k', { ctrl: true, alt: true }), false));
  assert.ok(!matchesShortcut('mod+enter', press('Enter', { ctrl: true, altGraph: true }), false));
  assert.ok(matchesShortcut('mod+\\', press('\\', { meta: true, alt: true, shift: true }), true));
});

test('ef_ux_01_altgr_saisi_sous_windows_laisse_passer_la_touche', () => {
  const commands = new Commands();
  const layout = command('layout', { keys: 'mod+\\' });
  commands.register(layout);
  const typed = press('\\', { ctrl: true, alt: true });
  assert.equal(commands.dispatch(typed, false), false);
  assert.equal(layout.runs, 0);
  assert.equal(typed.prevented, false);
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

test('ef_ux_01_touche_maintenue_ne_relance_pas_la_commande', () => {
  const commands = new Commands();
  const close = command('close', { keys: 'mod+w' });
  commands.register(close);

  assert.equal(commands.dispatch(press('w', { meta: true }), true), true);
  const held = press('w', { meta: true, repeat: true });
  assert.equal(commands.dispatch(held, true), true);
  assert.equal(commands.dispatch(press('w', { meta: true, repeat: true }), true), true);
  assert.equal(close.runs, 1);
  assert.equal(held.prevented, true);
});

test('ef_ux_01_echec_de_commande_est_signale_sans_planter_le_clavier', async () => {
  const errors: unknown[] = [];
  const commands = new Commands((e) => errors.push(e));
  commands.register(
    command('async', { keys: 'mod+r', run: () => Promise.reject('lecture impossible') }),
    command('sync', { keys: 'mod+t', run: () => { throw new Error('boum'); } }),
  );

  assert.equal(commands.dispatch(press('r', { meta: true }), true), true);
  assert.equal(commands.dispatch(press('t', { meta: true }), true), true);
  await new Promise((resolve) => setImmediate(resolve));

  assert.equal(errors.length, 2);
  assert.ok(errors.includes('lecture impossible'));
  assert.ok(errors.some((e) => e instanceof Error && e.message === 'boum'));
});

test('ef_ux_01_libelle_du_raccourci_vient_du_registre', () => {
  const commands = new Commands();
  commands.register(command('request.save', { keys: 'mod+s' }), command('tree.reload'));
  assert.equal(commands.label('request.save'), shortcutLabel('mod+s'));
  assert.equal(commands.label('tree.reload'), '');
  assert.equal(commands.label('inconnue'), '');
});

test('ef_syn_04_raccourci_de_navigation_entre_conflits_affiche_alt_et_fleche', () => {
  assert.equal(shortcutLabel('alt+arrowup', true), '⌥↑');
  assert.equal(shortcutLabel('alt+arrowdown', true), '⌥↓');
  assert.equal(shortcutLabel('alt+arrowup', false), 'Alt+↑');
  assert.equal(shortcutLabel('alt+arrowdown', false), 'Alt+↓');
});

test('ef_syn_04_fleche_seule_ne_declenche_pas_la_navigation_entre_conflits', () => {
  assert.ok(matchesShortcut('alt+arrowdown', press('ArrowDown', { alt: true }), true));
  assert.ok(matchesShortcut('alt+arrowup', press('ArrowUp', { alt: true }), false));
  assert.ok(!matchesShortcut('alt+arrowdown', press('ArrowDown'), true));
  assert.ok(!matchesShortcut('alt+arrowdown', press('ArrowDown', { alt: true, shift: true }), true));
  assert.ok(!matchesShortcut('alt+arrowdown', press('ArrowUp', { alt: true }), true));
  assert.ok(!matchesShortcut('alt+arrowdown', press('ArrowDown', { alt: true, meta: true }), true));
});

test('ef_col_04_raccourcis_de_larbre_affichent_f2_cmd_d_et_la_suppression', () => {
  assert.equal(shortcutLabel('f2', true), 'F2');
  assert.equal(shortcutLabel('f2', false), 'F2');
  assert.equal(shortcutLabel('mod+d', true), '⌘D');
  assert.equal(shortcutLabel('mod+d', false), 'Ctrl+D');
  assert.equal(shortcutLabel('mod+backspace', true), '⌘⌫');
  assert.equal(shortcutLabel('delete', false), 'Suppr');
});
