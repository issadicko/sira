export function prettyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return null;
  }
}

/** Filtre `$.a.b[0]` sur un JSON. Renvoie `undefined` si le chemin n'existe pas. */
export function jsonPath(value: unknown, path: string): { ok: true; value: unknown } | { ok: false; error: string } {
  const p = path.trim();
  if (!p || p === '$') return { ok: true, value };
  if (!/^\$(\.[\w$À-ÿ-]+|\[\d+\])+$/.test(p)) return { ok: false, error: 'Expression invalide' };
  let cur: unknown = value;
  for (const [, key, idx] of p.slice(1).matchAll(/\.([\w$À-ÿ-]+)|\[(\d+)\]/g)) {
    if (cur == null || typeof cur !== 'object') return { ok: false, error: `Rien à ${p}` };
    cur = idx !== undefined ? (cur as unknown[])[Number(idx)] : (cur as Record<string, unknown>)[key];
    if (cur === undefined) return { ok: false, error: `Rien à ${p}` };
  }
  return { ok: true, value: cur };
}

/** Vrai si une ligne dépasse `limit` caractères : le retour à la ligne y figerait l'éditeur. */
export function hasLongLine(text: string, limit = 200_000): boolean {
  let start = 0;
  for (let end = text.indexOf('\n'); end !== -1; end = text.indexOf('\n', start)) {
    if (end - start > limit) return true;
    start = end + 1;
  }
  return text.length - start > limit;
}

export function formatSize(bytes: number): string {
  return bytes < 1024 ? `${bytes} o` : `${(bytes / 1024).toLocaleString('fr-FR', { maximumFractionDigits: 1 })} Ko`;
}
