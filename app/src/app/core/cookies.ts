import type { CookieDraft, CookieKey, CookieView } from './model';

export interface DomainGroup {
  domain: string;
  cookies: CookieView[];
}

/** Les cookies regroupés par domaine, dans l'ordre alphabétique des domaines puis des chemins et des noms. */
export function groupByDomain(cookies: CookieView[]): DomainGroup[] {
  const groups = new Map<string, CookieView[]>();
  for (const cookie of cookies) groups.set(cookie.domain, [...(groups.get(cookie.domain) ?? []), cookie]);
  return [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([domain, list]) => ({
      domain,
      cookies: list.sort((a, b) => a.path.localeCompare(b.path) || a.key.localeCompare(b.key)),
    }));
}

export const keyOf = (cookie: CookieView): CookieKey => ({ domain: cookie.domain, path: cookie.path, key: cookie.key });

export const emptyDraft = (domain = ''): CookieDraft => ({
  key: '',
  value: '',
  domain,
  path: '/',
  secure: false,
  httpOnly: false,
  hostOnly: true,
  expires: null,
});

const ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$/;

/** Ce qui empêche de poser le cookie : un nom, un domaine et une date d'expiration lisible sont demandés. */
export function draftProblem(draft: CookieDraft): string | null {
  if (draft.key.trim() === '') return 'Le cookie demande un nom.';
  if (draft.domain.trim() === '') return 'Le cookie demande un domaine.';
  const expires = draft.expires?.trim();
  if (expires && (!ISO.test(expires) || Number.isNaN(Date.parse(expires)))) {
    return "La date d'expiration s'écrit 2026-12-31T23:59:59Z (UTC), ou reste vide pour un cookie de session.";
  }
  return null;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** « Session », « Expiré », « 12 min », « 3 h », « 2 j » : quand le cookie disparaît ; la date au-delà de trois mois. */
export function expiryLabel(expires: string | null, now: number): string {
  if (!expires) return 'Session';
  const at = Date.parse(expires);
  if (Number.isNaN(at)) return '—';
  const left = at - now;
  if (left <= 0) return 'Expiré';
  if (left < MINUTE) return `${Math.floor(left / 1000)} s`;
  if (left < HOUR) return `${Math.floor(left / MINUTE)} min`;
  if (left < DAY) return `${Math.floor(left / HOUR)} h`;
  if (left < 90 * DAY) return `${Math.floor(left / DAY)} j`;
  return new Date(at).toLocaleDateString('fr-FR', { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' });
}

/** Le texte d'un cookie pour une liste : un nom, et sa valeur coupée à `max` caractères. */
export function shortValue(value: string, max = 40): string {
  return value.length > max ? `${value.slice(0, max - 1)}…` : value;
}
