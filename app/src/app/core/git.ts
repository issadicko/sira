import type { GitFile, GitState } from './model';

export const STATE_NAMES: Record<GitFile['state'], string> = {
  M: 'modifié',
  A: 'ajouté',
  D: 'supprimé',
  R: 'renommé',
  U: 'en conflit',
};

/** Une ligne de la comparaison en deux colonnes ; `l` et `r` sont absents quand la ligne n'existe que d'un côté. */
export interface DiffRow {
  kind: 'ctx' | 'chg' | 'add' | 'del';
  l: { no: number; text: string } | null;
  r: { no: number; text: string } | null;
}

/** Au-delà, la plus longue sous-suite commune coûterait trop cher : on montre tout comme remplacé. */
const MAX_LINES = 3000;

const linesOf = (text: string | null): string[] => (text === null || text === '' ? [] : text.replace(/\n$/, '').split('\n'));

/** Compare deux textes ligne à ligne et aligne les différences en deux colonnes (une ligne changée des deux côtés tient sur une seule rangée). */
export function diffRows(head: string | null, work: string | null): DiffRow[] {
  const a = linesOf(head);
  const b = linesOf(work);
  const rows: DiffRow[] = [];
  let ln = 0;
  let rn = 0;
  const left = (text: string) => ({ no: ++ln, text });
  const right = (text: string) => ({ no: ++rn, text });
  const flush = (dels: string[], adds: string[]) => {
    const both = Math.min(dels.length, adds.length);
    for (let i = 0; i < both; i++) rows.push({ kind: 'chg', l: left(dels[i]!), r: right(adds[i]!) });
    for (const text of dels.slice(both)) rows.push({ kind: 'del', l: left(text), r: null });
    for (const text of adds.slice(both)) rows.push({ kind: 'add', l: null, r: right(text) });
  };
  if (a.length > MAX_LINES || b.length > MAX_LINES) {
    flush(a, b);
    return rows;
  }
  const table: number[][] = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      table[i]![j] = a[i] === b[j] ? table[i + 1]![j + 1]! + 1 : Math.max(table[i + 1]![j]!, table[i]![j + 1]!);
    }
  }
  let dels: string[] = [];
  let adds: string[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      flush(dels, adds);
      dels = [];
      adds = [];
      rows.push({ kind: 'ctx', l: left(a[i]!), r: right(b[j]!) });
      i++;
      j++;
    } else if (j < b.length && (i === a.length || table[i]![j + 1]! >= table[i + 1]![j]!)) {
      adds.push(b[j++]!);
    } else {
      dels.push(a[i++]!);
    }
  }
  flush(dels, adds);
  return rows;
}

export const touchedLines = (rows: DiffRow[]): number => rows.filter((r) => r.kind !== 'ctx').length;

/** Les lignes de contexte loin de toute différence sont repliées : on garde `around` lignes autour de chaque changement. */
export interface Shown {
  row: DiffRow | null;
  /** Nombre de lignes identiques repliées à cet endroit ; 0 pour une vraie ligne. */
  hidden: number;
}

export function collapse(rows: DiffRow[], around = 3): Shown[] {
  const keep = rows.map(() => false);
  rows.forEach((row, i) => {
    if (row.kind === 'ctx') return;
    for (let k = Math.max(0, i - around); k <= Math.min(rows.length - 1, i + around); k++) keep[k] = true;
  });
  const out: Shown[] = [];
  let hidden = 0;
  rows.forEach((row, i) => {
    if (keep[i]) {
      if (hidden) out.push({ row: null, hidden });
      hidden = 0;
      out.push({ row, hidden: 0 });
    } else hidden++;
  });
  if (hidden) out.push({ row: null, hidden });
  return out;
}

/** « main · ↑2 à pousser · ↓1 à récupérer » ou « à jour avec origin/main ». */
export function branchSummary(state: GitState): string {
  if (state.unborn) return 'aucun commit';
  if (!state.upstream) return state.branch ? 'pas de branche amont' : 'tête détachée';
  const parts = [];
  if (state.ahead) parts.push(`↑${state.ahead} à pousser`);
  if (state.behind) parts.push(`↓${state.behind} à récupérer`);
  return parts.length ? parts.join(' · ') : `à jour avec ${state.upstream}`;
}

export const conflicts = (state: GitState): number => state.files.filter((f) => f.state === 'U').length;

/** Valider est permis avec un message et au moins un fichier, et jamais pendant un conflit non résolu. */
export function commitBlocker(state: GitState, message: string): string | null {
  if (!state.files.length) return 'Aucune modification à valider.';
  if (conflicts(state)) return 'Résous les conflits avant de valider.';
  if (!message.trim()) return 'Écris un message avant de valider.';
  return null;
}

export const pullBlocker = (state: GitState): string | null => {
  if (!state.upstream) return "La branche n'a pas de branche amont : rien à récupérer.";
  return null;
};

export function fileName(path: string): { name: string; dir: string } {
  const at = path.lastIndexOf('/');
  return at < 0 ? { name: path, dir: '' } : { name: path.slice(at + 1), dir: path.slice(0, at) };
}
