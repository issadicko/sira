import type { Body, MultipartField } from './model';

/**
 * Passe à un autre type de corps : le texte est conservé entre json, texte et xml, les champs texte entre formulaires.
 * Un corps `other` (GraphQL, SPARQL, fichier…) n'est pas éditable ici : il reste tel quel.
 */
export function switchBody(body: Body, type: Body['type']): Body {
  if (type === body.type || body.type === 'other') return body;
  if (type === 'json' || type === 'text' || type === 'xml') {
    return { type, data: 'data' in body ? body.data : type === 'json' ? '{\n  \n}' : '' };
  }
  if (type === 'form-urlencoded') {
    const fields = body.type === 'multipart-form' ? body.fields : [];
    return {
      type,
      fields: fields.flatMap((f) =>
        f.kind === 'text' ? [{ name: f.name, value: f.value, enabled: f.enabled, description: f.description ?? null }] : [],
      ),
    };
  }
  if (type === 'multipart-form') {
    const fields = body.type === 'form-urlencoded' ? body.fields : [];
    return {
      type,
      fields: fields.map((f) => ({ name: f.name, kind: 'text', value: f.value, enabled: f.enabled, description: f.description ?? null })),
    };
  }
  return { type: 'none' };
}

/** Ajoute `path` aux fichiers du champ `index`, sans doublon ; sans effet si ce champ est un texte. */
export function withFile(fields: MultipartField[], index: number, path: string): MultipartField[] {
  return fields.map((f, i) =>
    i === index && f.kind === 'file' && !f.value.includes(path) ? { ...f, value: [...f.value, path] } : f,
  );
}
