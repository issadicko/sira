import type { SpecSummary } from './model';

/** Format de la spec pour l'aperçu : « OpenAPI 3.0.3 » ou « Swagger 2.0 », sans numéro si elle n'en déclare pas. */
export function specLabel({ format, formatVersion }: SpecSummary): string {
  const name = format === 'swagger' ? 'Swagger' : 'OpenAPI';
  return formatVersion ? `${name} ${formatVersion}` : name;
}

export function importedMessage(requests: number): string {
  return `Collection importée : ${requests} ${requests > 1 ? 'requêtes' : 'requête'}`;
}
