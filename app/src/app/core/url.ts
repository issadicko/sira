import type { Param, RequestDoc } from './model';

export interface UrlSegment {
  text: string;
  variable?: string;
}

/** Découpe une chaîne en texte brut et en `{{variables}}` pour l'affichage. */
export function segments(input: string): UrlSegment[] {
  const out: UrlSegment[] = [];
  const re = /\{\{([^}]+)\}\}/g;
  let last = 0;
  for (const m of input.matchAll(re)) {
    if (m.index > last) out.push({ text: input.slice(last, m.index) });
    out.push({ text: m[0], variable: m[1].trim() });
    last = m.index + m[0].length;
  }
  if (last < input.length) out.push({ text: input.slice(last) });
  return out;
}

function splitUrl(url: string): [string, string | null] {
  const i = url.indexOf('?');
  return i < 0 ? [url, null] : [url.slice(0, i), url.slice(i + 1)];
}

function pathNames(base: string): string[] {
  return base
    .split('/')
    .filter((s) => s.startsWith(':') && s.length > 1)
    .map((s) => s.slice(1));
}

/** L'URL a changé : les paramètres query et path suivent, les paramètres désactivés sont conservés. */
export function paramsFromUrl(url: string, previous: Param[]): Param[] {
  const [base, query] = splitUrl(url);
  const path: Param[] = pathNames(base).map(
    (name) => previous.find((p) => p.kind === 'path' && p.name === name) ?? { name, value: '', kind: 'path', enabled: true },
  );
  const fromUrl: Param[] = (query ?? '')
    .split('&')
    .filter((pair) => pair.length > 0)
    .map((pair) => {
      const eq = pair.indexOf('=');
      const name = eq < 0 ? pair : pair.slice(0, eq);
      const value = eq < 0 ? '' : pair.slice(eq + 1);
      const known = previous.find((p) => p.kind === 'query' && p.enabled && p.name === name);
      return { name, value, kind: 'query', enabled: true, description: known?.description ?? null };
    });
  const disabled = previous.filter((p) => p.kind === 'query' && !p.enabled);
  return [...fromUrl, ...disabled, ...path];
}

/** Les paramètres ont changé : la partie query de l'URL est reconstruite. */
export function urlFromParams(url: string, params: Param[]): string {
  const [base] = splitUrl(url);
  const query = params
    .filter((p) => p.kind === 'query' && p.enabled && p.name)
    .map((p) => (p.value === '' ? p.name : `${p.name}=${p.value}`))
    .join('&');
  return query ? `${base}?${query}` : base;
}

export function withUrl(doc: RequestDoc, url: string): RequestDoc {
  return { ...doc, url, params: paramsFromUrl(url, doc.params) };
}

export function withParams(doc: RequestDoc, params: Param[]): RequestDoc {
  return { ...doc, params, url: urlFromParams(doc.url, params) };
}
