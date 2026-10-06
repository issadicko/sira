import type { Body, RequestDoc } from './model';

export interface GraphqlParts {
  query: string;
  variables: string;
}

/** La requête et les variables d'un corps GraphQL ; vides quand la requête n'en a pas encore. */
export const graphqlParts = (body: Body): GraphqlParts =>
  body.type === 'graphql' ? { query: body.query, variables: body.variables } : { query: '', variables: '' };

export const withGraphql = (doc: RequestDoc, patch: Partial<GraphqlParts>): RequestDoc => ({
  ...doc,
  body: { type: 'graphql', ...graphqlParts(doc.body), ...patch },
});

export interface SchemaSummary {
  types: number;
  queries: number;
  mutations: number;
  subscriptions: number;
}

interface IntrospectedType {
  name?: string;
  fields?: unknown[] | null;
}

interface Introspection {
  __schema?: {
    queryType?: { name?: string } | null;
    mutationType?: { name?: string } | null;
    subscriptionType?: { name?: string } | null;
    types?: IntrospectedType[];
  };
}

/** Ce que contient un schéma : ses types (sans ceux d'introspection) et les champs de ses trois racines. */
export function summarize(introspection: unknown): SchemaSummary | null {
  const schema = (introspection as Introspection | null)?.__schema;
  if (!schema?.types) return null;
  const fieldsOf = (root?: { name?: string } | null) => schema.types?.find((t) => t.name === root?.name)?.fields?.length ?? 0;
  return {
    types: schema.types.filter((t) => !t.name?.startsWith('__')).length,
    queries: fieldsOf(schema.queryType),
    mutations: fieldsOf(schema.mutationType),
    subscriptions: fieldsOf(schema.subscriptionType),
  };
}

const plural = (n: number, one: string, many: string) => `${n} ${n > 1 ? many : one}`;

export function describeSchema(summary: SchemaSummary): string {
  const parts = [plural(summary.types, 'type', 'types'), plural(summary.queries, 'requête', 'requêtes')];
  if (summary.mutations) parts.push(plural(summary.mutations, 'mutation', 'mutations'));
  if (summary.subscriptions) parts.push(plural(summary.subscriptions, 'abonnement', 'abonnements'));
  return parts.join(' · ');
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** « à l'instant », « il y a 5 min »… pour un instant ISO 8601 ; la date quand c'est plus vieux qu'un mois. */
export function ageLabel(iso: string, now: number): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return '';
  const elapsed = Math.max(0, now - then);
  if (elapsed < MINUTE) return "à l'instant";
  if (elapsed < HOUR) return `il y a ${Math.floor(elapsed / MINUTE)} min`;
  if (elapsed < DAY) return `il y a ${Math.floor(elapsed / HOUR)} h`;
  if (elapsed < 30 * DAY) return `il y a ${Math.floor(elapsed / DAY)} j`;
  return new Date(then).toLocaleDateString('fr-FR');
}

export type Formatted = { ok: true; text: string } | { ok: false; reason: string };

/** Les variables mises en forme (2 espaces), sauf si elles ne sont pas du JSON ou portent des commentaires que la mise en forme retirerait. */
export function formatVariables(text: string): Formatted {
  const bare = stripComments(text);
  if (bare !== text) return { ok: false, reason: 'Les variables contiennent des commentaires, que le formatage retirerait.' };
  if (!bare.trim()) return { ok: true, text: '' };
  try {
    return { ok: true, text: JSON.stringify(JSON.parse(bare), null, 2) };
  } catch {
    return { ok: false, reason: 'Variables invalides : du JSON est attendu.' };
  }
}

/** Le texte sans ses commentaires de ligne et de bloc ; les chaînes sont respectées. Même règle que le moteur (`strip_comments`). */
export function stripComments(text: string): string {
  let out = '';
  let inString = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    const next = text[i + 1];
    if (inString) {
      out += c;
      if (c === '\\') out += text[++i] ?? '';
      else if (c === '"') inString = false;
    } else if (c === '"') {
      inString = true;
      out += c;
    } else if (c === '/' && next === '/') {
      while (i < text.length && text[i] !== '\n') i++;
      out += '\n';
    } else if (c === '/' && next === '*') {
      i += 2;
      while (i < text.length && !(text[i] === '*' && text[i + 1] === '/')) i++;
      i++;
    } else {
      out += c;
    }
  }
  return out;
}
