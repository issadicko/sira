import type { SpecSummary } from './model';

/** Format de la spec pour l'aperçu : « OpenAPI 3.0.3 » ou « Swagger 2.0 », sans numéro si elle n'en déclare pas. */
export function specLabel({ format, formatVersion }: SpecSummary): string {
  const name = format === 'swagger' ? 'Swagger' : 'OpenAPI';
  return formatVersion ? `${name} ${formatVersion}` : name;
}

export function importedMessage(requests: number): string {
  return `Collection importée : ${requests} ${requests > 1 ? 'requêtes' : 'requête'}`;
}

/** Ce que dit un import terminé (Postman, Insomnia) : les requêtes importées, et ce qui a été écarté ou corrigé. */
export function importSummary(requests: number, issues: { severity: string }[]): string {
  const errors = issues.filter((i) => i.severity === 'error').length;
  const warnings = issues.length - errors;
  const parts = [importedMessage(requests)];
  if (errors) parts.push(`${errors} ${errors > 1 ? 'éléments ignorés' : 'élément ignoré'}`);
  if (warnings) parts.push(`${warnings} ${warnings > 1 ? 'avertissements' : 'avertissement'}`);
  return parts.join(' · ');
}
