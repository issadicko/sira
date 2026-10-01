import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';

import {
  FONT_GROUPS,
  FONT_NAME_MAX,
  type Settings,
  clampSize,
  cssVariables,
  defaultSettings,
  fontStack,
  groupSpec,
  isDefaultFont,
  parseSettings,
  resetFont,
  serializeSettings,
  validFontName,
  withFont,
} from './settings.ts';

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

const SYSTEM_MONO = 'ui-monospace, "SF Mono", Menlo, Consolas, monospace';

function custom(overrides: Partial<Settings> = {}): Settings {
  return { ...defaultSettings(), ...overrides };
}

test('ef_ux_03_defauts_reprennent_les_valeurs_actuelles_du_code', () => {
  const defaults = defaultSettings();
  assert.deepEqual(defaults.ui, { family: 'default', custom: '', size: 13 });
  assert.deepEqual(defaults.request, { family: 'default', custom: '', size: 12.5 });
  assert.deepEqual(defaults.response, { family: 'default', custom: '', size: 12.5 });
});

test('ef_ux_03_defauts_et_piles_par_defaut_sont_ceux_de_styles_css', () => {
  const css = source('../../styles.css');
  const declared = (name: string) => css.match(new RegExp(`^\\s*${name}:\\s*(.+);$`, 'm'))?.[1];
  assert.equal(declared('--font-ui'), groupSpec('ui').presets.default.stack);
  assert.equal(declared('--font-mono'), groupSpec('request').presets.default.stack);
  assert.equal(declared('--font-mono'), groupSpec('response').presets.default.stack);
  assert.equal(declared('--size-ui'), '13px');
  assert.equal(declared('--size-req'), '12.5px');
  assert.equal(declared('--size-res'), '12.5px');
});

test('ef_ux_03_preregles_hors_ligne_de_chaque_groupe', () => {
  const labels = FONT_GROUPS.map((g) => [g.label, g.presets.default.label, g.presets.system.label]);
  assert.deepEqual(labels, [
    ['Interface', 'Inter (par défaut)', 'Police du système'],
    ['Éditeur de requête', 'JetBrains Mono (par défaut)', 'Monospace du système'],
    ['Résultat', 'JetBrains Mono (par défaut)', 'Monospace du système'],
  ]);
  assert.equal(fontStack('request', { family: 'system', custom: '', size: 12.5 }), SYSTEM_MONO);
  assert.equal(fontStack('response', { family: 'system', custom: '', size: 12.5 }), SYSTEM_MONO);
  assert.match(fontStack('ui', { family: 'system', custom: '', size: 13 }), /^system-ui, .*sans-serif$/);
  assert.match(fontStack('ui', { family: 'default', custom: '', size: 13 }), /^"Inter", .*sans-serif$/);
  assert.match(fontStack('request', { family: 'default', custom: '', size: 12.5 }), /^"JetBrains Mono", .*monospace$/);
});

test('ef_ux_03_police_personnalisee_est_citee_puis_suivie_d_un_repli_generique', () => {
  const mono = fontStack('request', { family: 'custom', custom: ' Fira Code ', size: 12.5 });
  assert.ok(mono.startsWith('"Fira Code", '));
  assert.ok(mono.endsWith(', monospace'));
  const sans = fontStack('ui', { family: 'custom', custom: 'Fira Sans', size: 13 });
  assert.ok(sans.startsWith('"Fira Sans", '));
  assert.ok(sans.endsWith(', sans-serif'));
});

test('ef_ux_03_nom_de_police_accepte', () => {
  for (const name of ['Fira Code', 'JetBrains Mono NL', 'Source Code Pro', 'M PLUS 1 Code', 'Noto Sans CJK JP', 'ヒラギノ角ゴ', ' Menlo ', 'IBM Plex Mono', 'Courier New', 'Inter-Variable', 'Iosevka_Term', 'x'.repeat(FONT_NAME_MAX)]) {
    assert.ok(validFontName(name), name);
  }
});

test('ef_ux_03_nom_de_police_refuse_les_caracteres_qui_sortent_de_la_declaration', () => {
  const refused = [
    '',
    '   ',
    'a;b',
    'a{b',
    'a}b',
    '<script>',
    'a<b',
    '"Fira Code"',
    "a'b",
    'a\\b',
    'a,b',
    'url(x)',
    'a/b',
    '*',
    '-Fira',
    'Fira\nCode',
    'x'.repeat(FONT_NAME_MAX + 1),
    'Fira Code; background: red',
  ];
  for (const name of refused) assert.equal(validFontName(name), false, JSON.stringify(name));
});

test('ef_ux_03_famille_personnalisee_invalide_retombe_sur_la_pile_par_defaut', () => {
  const fallback = groupSpec('request').presets.default.stack;
  for (const name of ['', 'a;b', '<b>', 'x'.repeat(200)]) {
    assert.equal(fontStack('request', { family: 'custom', custom: name, size: 12.5 }), fallback, JSON.stringify(name));
  }
  assert.ok(!cssVariables(custom({ request: { family: 'custom', custom: 'a;b{c}', size: 12.5 } }))['--font-req']!.includes(';'));
});

test('ef_ux_03_taille_bornee_par_groupe_et_arrondie_au_demi_pixel', () => {
  assert.equal(clampSize('ui', 5), 11);
  assert.equal(clampSize('ui', 40), 18);
  assert.equal(clampSize('ui', 11), 11);
  assert.equal(clampSize('ui', 18), 18);
  assert.equal(clampSize('request', 9), 10);
  assert.equal(clampSize('request', 24.4), 24);
  assert.equal(clampSize('response', 100), 24);
  assert.equal(clampSize('response', 13.26), 13.5);
  assert.equal(clampSize('response', 13.2), 13);
  assert.equal(clampSize('request', -3), 10);
});

test('ef_ux_03_taille_qui_n_est_pas_un_nombre_donne_la_valeur_par_defaut', () => {
  for (const value of ['14', 'abc', null, undefined, NaN, Infinity, -Infinity, {}, [], true]) {
    assert.equal(clampSize('ui', value), 13, String(value));
    assert.equal(clampSize('request', value), 12.5, String(value));
  }
});

test('ef_ux_03_variables_css_des_trois_groupes', () => {
  const variables = cssVariables(
    custom({
      ui: { family: 'system', custom: '', size: 15 },
      request: { family: 'custom', custom: 'Fira Code', size: 14 },
      response: { family: 'system', custom: '', size: 11.5 },
    }),
  );
  assert.deepEqual(Object.keys(variables), ['--font-ui', '--size-ui', '--font-req', '--size-req', '--font-res', '--size-res']);
  assert.equal(variables['--size-ui'], '15px');
  assert.equal(variables['--size-req'], '14px');
  assert.equal(variables['--size-res'], '11.5px');
  assert.match(variables['--font-ui']!, /^system-ui, /);
  assert.match(variables['--font-req']!, /^"Fira Code", "JetBrains Mono", .*monospace$/);
  assert.equal(variables['--font-res'], SYSTEM_MONO);
});

test('ef_ux_03_variables_css_par_defaut', () => {
  const variables = cssVariables(defaultSettings());
  assert.equal(variables['--size-ui'], '13px');
  assert.equal(variables['--size-req'], '12.5px');
  assert.equal(variables['--size-res'], '12.5px');
  assert.equal(variables['--font-ui'], groupSpec('ui').presets.default.stack);
  assert.equal(variables['--font-req'], groupSpec('request').presets.default.stack);
});

test('ef_ux_03_lecture_d_un_stockage_absent_ou_illisible_donne_les_defauts', () => {
  for (const raw of [null, undefined, '', '{', 'null', '[]', '42', '"texte"', 'true']) {
    assert.deepEqual(parseSettings(raw), defaultSettings(), String(raw));
  }
});

test('ef_ux_03_version_inconnue_ou_absente_donne_les_defauts', () => {
  const saved = { ui: { family: 'system', custom: '', size: 16 } };
  assert.deepEqual(parseSettings(JSON.stringify({ ...saved })), defaultSettings());
  assert.deepEqual(parseSettings(JSON.stringify({ v: 0, ...saved })), defaultSettings());
  assert.deepEqual(parseSettings(JSON.stringify({ v: 2, ...saved })), defaultSettings());
  assert.deepEqual(parseSettings(JSON.stringify({ v: '1', ...saved })), defaultSettings());
  assert.equal(parseSettings(JSON.stringify({ v: 1, ...saved })).ui.size, 16);
});

test('ef_ux_03_valeur_invalide_revient_a_son_defaut_sans_toucher_au_reste', () => {
  const settings = parseSettings(
    JSON.stringify({
      v: 1,
      ui: { family: 'comic-sans', custom: 42, size: 'grand' },
      request: { family: 'custom', custom: 'a;b', size: 14 },
      response: { family: 'system', size: 99 },
    }),
  );
  assert.deepEqual(settings.ui, { family: 'default', custom: '', size: 13 });
  assert.deepEqual(settings.request, { family: 'default', custom: '', size: 14 });
  assert.deepEqual(settings.response, { family: 'system', custom: '', size: 24 });
});

test('ef_ux_03_groupe_absent_ou_mal_forme_donne_ses_defauts', () => {
  const settings = parseSettings(JSON.stringify({ v: 1, ui: { family: 'custom', custom: 'Fira Sans', size: 15 }, request: 'x', response: null }));
  assert.deepEqual(settings.ui, { family: 'custom', custom: 'Fira Sans', size: 15 });
  assert.deepEqual(settings.request, defaultSettings().request);
  assert.deepEqual(settings.response, defaultSettings().response);
});

test('ef_ux_03_famille_personnalisee_sans_nom_valide_revient_a_la_famille_par_defaut', () => {
  const settings = parseSettings(JSON.stringify({ v: 1, ui: { family: 'custom', custom: '', size: 13 }, request: { family: 'custom', custom: 'a{b', size: 13 } }));
  assert.equal(settings.ui.family, 'default');
  assert.equal(settings.request.family, 'default');
  assert.equal(settings.request.custom, '');
});

test('ef_ux_03_aller_retour_des_reglages_enregistres', () => {
  const settings = custom({
    ui: { family: 'system', custom: 'Fira Sans', size: 14 },
    request: { family: 'custom', custom: 'Fira Code', size: 16.5 },
    response: { family: 'default', custom: '', size: 10 },
  });
  assert.deepEqual(parseSettings(serializeSettings(settings)), settings);
  assert.deepEqual(parseSettings(serializeSettings(defaultSettings())), defaultSettings());
});

test('ef_ux_03_le_json_enregistre_est_versionne_et_porte_les_variables_css', () => {
  const settings = custom({ request: { family: 'custom', custom: 'Fira Code', size: 14 } });
  const saved = JSON.parse(serializeSettings(settings));
  assert.equal(saved.v, 1);
  assert.deepEqual(saved.css, cssVariables(settings));
  const tampered = JSON.stringify({ ...saved, css: { '--font-req': 'x; background: red' } });
  assert.deepEqual(parseSettings(tampered), settings);
});

test('ef_ux_03_retablir_ne_depend_que_de_la_famille_et_de_la_taille', () => {
  assert.ok(isDefaultFont('ui', { family: 'default', custom: 'Fira Sans', size: 13 }));
  assert.ok(isDefaultFont('request', { family: 'default', custom: '', size: 12.5 }));
  assert.ok(!isDefaultFont('request', { family: 'default', custom: '', size: 13 }));
  assert.ok(!isDefaultFont('ui', { family: 'system', custom: '', size: 13 }));
  assert.ok(!isDefaultFont('response', { family: 'custom', custom: 'Fira Code', size: 12.5 }));
});

test('ef_ux_03_changement_de_taille_borne_et_limite_au_groupe_modifie', () => {
  const before = defaultSettings();
  const after = withFont(before, 'ui', { size: 40 });
  assert.equal(after.ui.size, 18);
  assert.deepEqual(after.request, before.request);
  assert.deepEqual(after.response, before.response);
  assert.equal(withFont(after, 'ui', { size: 12 }).ui.size, 12);
  assert.equal(withFont(before, 'request', { size: Number.NaN }).request.size, 12.5);
  assert.equal(before.ui.size, 13);
});

test('ef_ux_03_changement_de_famille_garde_le_nom_personnalise_pour_y_revenir', () => {
  let settings = withFont(defaultSettings(), 'request', { family: 'custom', custom: ' Fira Code ' });
  assert.deepEqual(settings.request, { family: 'custom', custom: 'Fira Code', size: 12.5 });
  settings = withFont(settings, 'request', { family: 'system' });
  assert.deepEqual(settings.request, { family: 'system', custom: 'Fira Code', size: 12.5 });
  settings = withFont(settings, 'request', { family: 'custom' });
  assert.match(fontStack('request', settings.request), /^"Fira Code", /);
});

test('ef_ux_03_nom_invalide_est_ignore_et_nom_vide_efface_la_police_personnalisee', () => {
  const settings = withFont(defaultSettings(), 'response', { family: 'custom', custom: 'Fira Code' });
  for (const name of ['a;b', 'a{b', '<b>', 'x'.repeat(100)]) {
    assert.equal(withFont(settings, 'response', { custom: name }).response.custom, 'Fira Code', name);
  }
  const cleared = withFont(settings, 'response', { custom: '  ' });
  assert.equal(cleared.response.custom, '');
  assert.equal(fontStack('response', cleared.response), groupSpec('response').presets.default.stack);
});

test('ef_ux_03_retablir_un_groupe_ne_touche_pas_aux_autres', () => {
  let settings = withFont(defaultSettings(), 'ui', { family: 'system', size: 16 });
  settings = withFont(settings, 'request', { family: 'custom', custom: 'Fira Code', size: 14 });
  const reset = resetFont(settings, 'request');
  assert.deepEqual(reset.request, defaultSettings().request);
  assert.deepEqual(reset.ui, settings.ui);
  assert.ok(isDefaultFont('request', reset.request));
});

function runInlineScript(storage: Record<string, string>) {
  const html = source('../../index.html');
  const code = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1]).join('\n');
  const applied: Record<string, string> = {};
  const root = { dataset: {} as Record<string, string>, style: { setProperty: (name: string, value: string) => (applied[name] = value) } };
  runInNewContext(code, { localStorage: { getItem: (key: string) => storage[key] ?? null }, document: { documentElement: root } });
  return { applied, theme: root.dataset['theme'] };
}

test('ef_ux_03_index_html_pose_les_variables_enregistrees_avant_le_premier_rendu', () => {
  const settings = custom({ ui: { family: 'system', custom: '', size: 16 }, response: { family: 'custom', custom: 'Fira Code', size: 11 } });
  const { applied, theme } = runInlineScript({ 'xc-settings': serializeSettings(settings), 'xc-theme': 'light' });
  assert.deepEqual(applied, cssVariables(settings));
  assert.equal(theme, 'light');
});

test('ef_ux_03_index_html_ignore_un_stockage_absent_ou_illisible_et_garde_le_theme', () => {
  assert.deepEqual(runInlineScript({}).applied, {});
  assert.deepEqual(runInlineScript({ 'xc-settings': '{' }).applied, {});
  assert.deepEqual(runInlineScript({ 'xc-settings': '{"v":1}' }).applied, {});
  assert.equal(runInlineScript({ 'xc-settings': '{', 'xc-theme': 'dark' }).theme, 'dark');
});

test('ef_ux_03_index_html_ne_pose_que_des_variables_de_police_et_de_taille', () => {
  const css = { '--font-ui': 'serif', '--size-ui': '15px', background: 'red', '--accent': 'red', '--evil': 'x' };
  const { applied } = runInlineScript({ 'xc-settings': JSON.stringify({ v: 1, css }) });
  assert.deepEqual(applied, { '--font-ui': 'serif', '--size-ui': '15px' });
});
