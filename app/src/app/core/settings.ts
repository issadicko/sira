export type FontGroup = 'ui' | 'request' | 'response';
export type FamilyChoice = 'default' | 'system' | 'custom';

export interface FontSetting {
  family: FamilyChoice;
  /** Nom d'une police installée, utilisé quand `family` vaut `custom`. */
  custom: string;
  /** Taille en px. */
  size: number;
}

export type Settings = Record<FontGroup, FontSetting>;

export interface FontGroupSpec {
  id: FontGroup;
  label: string;
  hint: string;
  presets: Record<'default' | 'system', { label: string; stack: string }>;
  size: { min: number; max: number; default: number };
  variables: { family: string; size: string };
}

export const SETTINGS_KEY = 'xc-settings';
export const SETTINGS_VERSION = 1;
export const FONT_NAME_MAX = 64;
export const FONT_NAME_RULE = `Lettres, chiffres, espaces, point, tiret et plus, ${FONT_NAME_MAX} caractères au plus.`;

const SANS = '"Inter", -apple-system, BlinkMacSystemFont, "Segoe UI Variable", "Segoe UI", system-ui, sans-serif';
const SYSTEM_SANS = 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI Variable", "Segoe UI", sans-serif';
const MONO = '"JetBrains Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace';
const SYSTEM_MONO = 'ui-monospace, "SF Mono", Menlo, Consolas, monospace';

export const FONT_GROUPS: readonly FontGroupSpec[] = [
  {
    id: 'ui',
    label: 'Interface',
    hint: 'Menus, listes, onglets, boutons et textes.',
    presets: { default: { label: 'Inter (par défaut)', stack: SANS }, system: { label: 'Police du système', stack: SYSTEM_SANS } },
    size: { min: 11, max: 18, default: 13 },
    variables: { family: '--font-ui', size: '--size-ui' },
  },
  {
    id: 'request',
    label: 'Éditeur de requête',
    hint: "Barre d'URL, corps, valeurs des paramètres et des en-têtes, scripts.",
    presets: { default: { label: 'JetBrains Mono (par défaut)', stack: MONO }, system: { label: 'Monospace du système', stack: SYSTEM_MONO } },
    size: { min: 10, max: 24, default: 12.5 },
    variables: { family: '--font-req', size: '--size-req' },
  },
  {
    id: 'response',
    label: 'Résultat',
    hint: 'Corps, en-têtes et chronologie de la réponse.',
    presets: { default: { label: 'JetBrains Mono (par défaut)', stack: MONO }, system: { label: 'Monospace du système', stack: SYSTEM_MONO } },
    size: { min: 10, max: 24, default: 12.5 },
    variables: { family: '--font-res', size: '--size-res' },
  },
];

const FONT_NAME = /^[\p{L}\p{N}][\p{L}\p{N} ._+-]*$/u;

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);

export const groupSpec = (id: FontGroup): FontGroupSpec => FONT_GROUPS.find((g) => g.id === id)!;

/** Vrai pour un nom de police sûr à glisser entre guillemets dans une déclaration CSS. */
export function validFontName(name: string): boolean {
  const trimmed = name.trim();
  return trimmed.length <= FONT_NAME_MAX && FONT_NAME.test(trimmed);
}

/** Taille bornée au pas de 0,5 px ; une valeur qui n'est pas un nombre donne la taille par défaut. */
export function clampSize(group: FontGroup, value: unknown): number {
  const { min, max, default: fallback } = groupSpec(group).size;
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value * 2) / 2));
}

export function defaultFont(group: FontGroup): FontSetting {
  return { family: 'default', custom: '', size: groupSpec(group).size.default };
}

export function defaultSettings(): Settings {
  return { ui: defaultFont('ui'), request: defaultFont('request'), response: defaultFont('response') };
}

export function isDefaultFont(group: FontGroup, font: FontSetting): boolean {
  return font.family === 'default' && font.size === groupSpec(group).size.default;
}

/** Pile de polices d'un groupe ; une police personnalisée est toujours suivie de la pile par défaut, qui finit par la famille générique. */
export function fontStack(group: FontGroup, font: FontSetting): string {
  const { presets } = groupSpec(group);
  if (font.family === 'custom' && validFontName(font.custom)) return `"${font.custom.trim()}", ${presets.default.stack}`;
  return presets[font.family === 'system' ? 'system' : 'default'].stack;
}

/** Variables CSS à poser sur `:root` pour ces réglages. */
export function cssVariables(settings: Settings): Record<string, string> {
  const variables: Record<string, string> = {};
  for (const { id, variables: names } of FONT_GROUPS) {
    variables[names.family] = fontStack(id, settings[id]);
    variables[names.size] = `${settings[id].size}px`;
  }
  return variables;
}

/** Applique un changement à un groupe : la taille est bornée, un nom de police invalide est ignoré (un nom vide l'efface). */
export function withFont(settings: Settings, group: FontGroup, change: Partial<FontSetting>): Settings {
  const current = settings[group];
  const name = change.custom?.trim();
  const custom = name === undefined || (name !== '' && !validFontName(name)) ? current.custom : name;
  const size = change.size === undefined ? current.size : clampSize(group, change.size);
  return { ...settings, [group]: { family: change.family ?? current.family, custom, size } };
}

export function resetFont(settings: Settings, group: FontGroup): Settings {
  return { ...settings, [group]: defaultFont(group) };
}

function readFont(group: FontGroup, raw: unknown): FontSetting {
  const data = isRecord(raw) ? raw : {};
  const custom = typeof data['custom'] === 'string' && validFontName(data['custom']) ? data['custom'].trim() : '';
  const family: FamilyChoice = data['family'] === 'system' ? 'system' : data['family'] === 'custom' && custom ? 'custom' : 'default';
  return { family, custom, size: clampSize(group, data['size']) };
}

/** Lit les réglages enregistrés : une version inconnue ou un JSON illisible donne les défauts, une valeur invalide revient à son défaut. */
export function parseSettings(raw: string | null | undefined): Settings {
  let data: unknown = null;
  try {
    data = raw ? JSON.parse(raw) : null;
  } catch {
    data = null;
  }
  if (!isRecord(data) || data['v'] !== SETTINGS_VERSION) return defaultSettings();
  return { ui: readFont('ui', data['ui']), request: readFont('request', data['request']), response: readFont('response', data['response']) };
}

/**
 * JSON enregistré dans `localStorage`. Il porte aussi les variables CSS calculées (`css`) pour que `index.html` les pose
 * avant le premier rendu sans dupliquer les règles de validation.
 */
export function serializeSettings(settings: Settings): string {
  return JSON.stringify({ v: SETTINGS_VERSION, ...settings, css: cssVariables(settings) });
}
