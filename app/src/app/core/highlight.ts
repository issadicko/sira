const escapeHtml = (s: string) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

/** Colore une ligne de JSON déjà indentée. Le résultat est échappé, donc sûr pour innerHTML. */
export function highlightJsonLine(line: string): string {
  return escapeHtml(line).replace(
    /(&quot;(?:[^&]|&(?!quot;))*?&quot;)(\s*:)?|\b(true|false|null)\b|(-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b)/g,
    (_m, str: string, colon: string, kw: string, num: string) => {
      if (str) return colon ? `<span class="t-key">${str}</span>${colon}` : `<span class="t-str">${str}</span>`;
      if (kw) return `<span class="t-kw">${kw}</span>`;
      return `<span class="t-num">${num}</span>`;
    },
  );
}

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

export function formatSize(bytes: number): string {
  return bytes < 1024 ? `${bytes} o` : `${(bytes / 1024).toLocaleString('fr-FR', { maximumFractionDigits: 1 })} Ko`;
}
