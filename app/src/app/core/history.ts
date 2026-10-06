import type { HistoryEntry, TreeItem } from './model';

export const HISTORY_PREVIEW = 5;
export const HISTORY_LIMIT = 50;

function paths(items: TreeItem[], into = new Set<string>()): Set<string> {
  for (const item of items) {
    if (item.kind === 'folder') paths(item.children, into);
    else into.add(item.path);
  }
  return into;
}

/** Les entrées dont la requête est toujours dans la collection : une requête renommée ou supprimée n'a plus de fichier à ouvrir. */
export function forExistingRequests(entries: HistoryEntry[], items: TreeItem[]): HistoryEntry[] {
  const known = paths(items);
  return entries.filter((entry) => known.has(entry.path));
}

/** L'heure pour un envoi d'aujourd'hui, le jour et le mois pour un plus ancien. */
export function whenLabel(iso: string, now: Date): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return '';
  const sameDay = at.toDateString() === now.toDateString();
  return sameDay
    ? at.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' })
    : at.toLocaleDateString('fr-FR', { day: 'numeric', month: 'short' });
}

/** Ce que la ligne affiche à la place du code HTTP : le code, ou ERR quand aucune réponse n'est venue. */
export const statusLabel = (entry: HistoryEntry): string => (entry.status === null ? 'ERR' : String(entry.status));

/** Le détail d'une entrée pour l'infobulle : adresse, environnement, durée, taille, erreur. */
export function describeEntry(entry: HistoryEntry): string {
  const parts = [`${entry.method} ${entry.url}`];
  if (entry.env) parts.push(`Environnement : ${entry.env}`);
  if (entry.status !== null) parts.push(`${Math.round(entry.durationMs)} ms · ${entry.size} o`);
  if (entry.error) parts.push(entry.error);
  return parts.join('\n');
}
