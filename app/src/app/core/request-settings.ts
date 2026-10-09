import type { RequestDoc } from './model';

export const DEFAULT_MAX_REDIRECTS = 5;

/** Les réglages d'envoi d'une requête tels que l'exécution les applique : les défauts de Bruno remplacent ce que le fichier ne dit pas. */
export interface EffectiveSettings {
  timeoutMs: number;
  followRedirects: boolean;
  maxRedirects: number;
  forwardAuthorizationHeader: boolean;
}

export const effectiveSettings = (doc: RequestDoc): EffectiveSettings => ({
  timeoutMs: doc.timeoutMs ?? 0,
  followRedirects: doc.followRedirects ?? true,
  maxRedirects: doc.maxRedirects ?? DEFAULT_MAX_REDIRECTS,
  forwardAuthorizationHeader: doc.forwardAuthorizationHeader ?? true,
});

/** Un entier positif ou nul saisi, `null` quand le texte n'en est pas un (vide, négatif, décimal, lettres). */
export function wholeNumber(text: string): number | null {
  const trimmed = text.trim();
  return /^\d{1,9}$/.test(trimmed) ? Number(trimmed) : null;
}

export type SettingsPatch = Partial<Pick<RequestDoc, 'timeoutMs' | 'followRedirects' | 'maxRedirects' | 'forwardAuthorizationHeader' | 'keepAliveMs'>>;

/** Le document avec ces réglages ; un délai de 0 (illimité) est écrit comme une absence de délai. */
export function withSettings(doc: RequestDoc, patch: SettingsPatch): RequestDoc {
  const next = { ...doc, ...patch };
  const timeout = patch.timeoutMs === 0 ? { ...next, timeoutMs: null } : next;
  return patch.keepAliveMs === 0 ? { ...timeout, keepAliveMs: null } : timeout;
}

/** Les réglages d'une connexion WebSocket : le délai pour la joindre (0 : 30 s) et l'intervalle des pings (0 : aucun). */
export const wsSettings = (doc: RequestDoc): { timeoutMs: number; keepAliveMs: number } => ({
  timeoutMs: doc.timeoutMs ?? 0,
  keepAliveMs: doc.keepAliveMs ?? 0,
});
