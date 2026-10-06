import type { ExportFormat } from './model';

export const EXPORT_FORMATS: { id: ExportFormat; label: string; version: string; suffix: string }[] = [
  { id: 'postman', label: 'Postman', version: 'v2.1', suffix: '.postman_collection.json' },
  { id: 'openapi', label: 'OpenAPI', version: '3.0.3', suffix: '.openapi.json' },
];

/** Le nom proposé pour le fichier d'export : le nom de la collection, réduit à ses lettres et chiffres, puis le suffixe du format. */
export function exportFileName(collection: string, format: ExportFormat): string {
  const slug = collection.replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '');
  const suffix = EXPORT_FORMATS.find((f) => f.id === format)?.suffix ?? '.json';
  return `${slug || 'collection'}${suffix}`;
}

/** Ce que dit un export terminé : le nombre d'éléments que le format n'a pas pu porter. */
export function exportSummary(issues: number): string {
  if (!issues) return 'Collection exportée';
  return `Collection exportée · ${issues} ${issues > 1 ? 'éléments non exportés' : 'élément non exporté'}`;
}
