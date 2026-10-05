import type { DiskChange } from './model';

export type TabVerdict = 'unchanged' | 'adopt' | 'stale';

/**
 * Que faire d'un onglet dont le fichier vient d'être relu (quatre sérialisations JSON). `base` est le fichier tel que Rust l'a lu la
 * dernière fois, à l'ouverture ou juste après notre écriture : on compare le disque à `base`, jamais à `saved`, car le document de
 * l'interface (valeurs `null`, ordre des clés) ne se sérialise pas comme celui que Rust relit. `saved` est la version enregistrée du
 * document de l'interface, `current` le brouillon. Un onglet sans brouillon adopte le disque ; avec un brouillon, il est périmé :
 * l'enregistrer remplacerait les changements du disque par une version qui les ignore.
 */
export function verdictFor(base: string, saved: string, current: string, disk: string): TabVerdict {
  if (disk === base) return 'unchanged';
  if (disk === current) return 'adopt';
  return current === saved ? 'adopt' : 'stale';
}

/** Les onglets, parmi `tabPaths`, dont le fichier a changé ou dont un dossier a bougé ; tous si le lot est tronqué. */
export function affectedTabs(change: DiskChange, tabPaths: string[]): string[] {
  if (change.truncated) return tabPaths;
  return tabPaths.filter((path) => change.paths.some((changed) => path === changed || path.startsWith(`${changed}/`)));
}

const baseName = (path: string) => path.slice(path.lastIndexOf('/') + 1);

/** Vrai si le lot touche les environnements ou le `.env` : leurs variables sont à relire. */
export function touchesEnvironments(change: DiskChange): boolean {
  return change.truncated || change.paths.some((path) => path.startsWith('environments/') || baseName(path) === '.env' || baseName(path).startsWith('.env.'));
}

/** Réunit deux lots reçus pendant qu'un traitement était en cours ; un lot d'une autre collection remplace l'ancien. */
export function mergeChanges(pending: DiskChange | null, next: DiskChange): DiskChange {
  if (!pending || pending.root !== next.root) return next;
  return { root: next.root, paths: [...new Set([...pending.paths, ...next.paths])], truncated: pending.truncated || next.truncated };
}

/** Résumé d'un traitement, ou `null` quand rien ne mérite d'être dit à l'utilisateur. */
export function describeDiskChange(outcome: { closed: string[]; reloaded: number; stale: string[] }): string | null {
  const parts: string[] = [];
  const { closed, reloaded, stale } = outcome;
  if (closed.length) {
    parts.push(closed.length === 1 ? `« ${closed[0]} » est fermée : son fichier a été supprimé.` : `${closed.length} onglets fermés : leurs fichiers ont été supprimés.`);
  }
  if (reloaded) parts.push(reloaded === 1 ? 'Une requête ouverte a été relue depuis le disque.' : `${reloaded} requêtes ouvertes ont été relues depuis le disque.`);
  if (stale.length) {
    parts.push(
      stale.length === 1
        ? `« ${stale[0]} » a changé sur le disque : ton brouillon est gardé.`
        : `${stale.length} requêtes ont changé sur le disque : tes brouillons sont gardés.`,
    );
  }
  return parts.length ? parts.join(' ') : null;
}
