import { autocompletion } from '@codemirror/autocomplete';
import type { Extension } from '@codemirror/state';
import { tooltips } from '@codemirror/view';
import type { IntrospectionQuery } from 'graphql';

import type { Formatted } from '../core/graphql';

/**
 * Coloration, autocomplétion, diagnostics et survol de GraphQL ; avec un schéma, ils suivent ses types.
 * Chargé à la première requête GraphQL ouverte : `graphql` et `cm6-graphql` ne pèsent pas sur le démarrage.
 */
export async function graphqlSupport(introspection: unknown): Promise<Extension> {
  const [{ graphql }, { buildClientSchema }] = await Promise.all([import('cm6-graphql'), import('graphql')]);
  let schema;
  try {
    schema = introspection ? buildClientSchema(introspection as IntrospectionQuery) : undefined;
  } catch {
    schema = undefined;
  }
  return [graphql(schema), autocompletion({ icons: false }), tooltips({ parent: document.body })];
}

/** La requête remise en forme par graphql-js, sauf si elle porte des commentaires `#` : l'impression les retirerait. */
export async function formatQuery(query: string): Promise<Formatted> {
  const { parse, print, TokenKind } = await import('graphql');
  try {
    const document = parse(query);
    for (let token = document.loc?.startToken ?? null; token; token = token.next) {
      if (token.kind === TokenKind.COMMENT) return { ok: false, reason: 'La requête contient des commentaires #, que le formatage retirerait.' };
    }
    return { ok: true, text: print(document) };
  } catch (e) {
    return { ok: false, reason: e instanceof Error ? e.message : String(e) };
  }
}
