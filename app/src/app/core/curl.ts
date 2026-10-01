import type { Auth, RequestDoc } from './model';

const CURL = /^\s*curl\s/i;
const NO_AUTH = new Set<Auth['type']>(['inherit', 'none']);
const PARAMETER = /^(:|\{|\d+$)/;
const VERSION = /^v\d+$/i;

/** Même détection que Bruno : le texte collé commence par `curl` suivi d'un espace. */
export function isCurlCommand(text: string): boolean {
  return CURL.test(text);
}

/**
 * Applique à `doc` ce qu'un cURL collé décrit hors URL, comme Bruno : la méthode, puis les en-têtes, le corps
 * et l'authentification seulement s'ils sont présents. L'URL passe par `withUrl`, qui resynchronise les paramètres.
 */
export function withCurl(doc: RequestDoc, curl: RequestDoc): RequestDoc {
  return {
    ...doc,
    method: curl.method ? curl.method.toUpperCase() : doc.method,
    headers: curl.headers.length ? curl.headers : doc.headers,
    body: curl.body.type === 'none' ? doc.body : curl.body,
    auth: NO_AUTH.has(curl.auth.type) ? doc.auth : curl.auth,
  };
}

const BODY_LABELS: Record<string, string> = {
  json: 'JSON',
  text: 'texte',
  xml: 'XML',
  'form-urlencoded': 'formulaire',
  'multipart-form': 'multipart',
};

/** Ce que reprend un cURL, en une ligne : en-têtes, corps, authentification. */
export function describeCurl(curl: RequestDoc): string {
  const parts: string[] = [];
  if (curl.headers.length) parts.push(`${curl.headers.length} en-tête${curl.headers.length > 1 ? 's' : ''}`);
  if (curl.body.type !== 'none') parts.push(`corps ${BODY_LABELS[curl.body.type] ?? 'autre'}`);
  if (!NO_AUTH.has(curl.auth.type)) parts.push(`auth ${curl.auth.type}`);
  return parts.join(' · ') || 'ni en-tête, ni corps';
}

/** Nom proposé pour une requête d'après son URL : ses deux derniers segments utiles, sinon l'hôte. */
export function nameFromUrl(url: string): string {
  const [, host = '', path = ''] = /^(?:[a-z][a-z0-9+.-]*:\/\/)?(\{\{[^}]*\}\}|[^/?#]*)([^?#]*)/i.exec(url.trim()) ?? [];
  const segments = path.split('/').filter((s) => s && !PARAMETER.test(s) && !VERSION.test(s));
  const name = segments.slice(-2).join(' ').replace(/[-_.]+/g, ' ').trim();
  if (name) return name.charAt(0).toUpperCase() + name.slice(1);
  return host && !host.startsWith('{{') ? host : 'Nouvelle requête';
}
