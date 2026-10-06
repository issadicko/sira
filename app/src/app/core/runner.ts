import type { RunDone, RunEvent, RunHalt, RunResult, RunStatus, TreeItem } from './model';

/** Où en est le run : ce qui est arrivé, ce qui tourne, ce qui attend. */
export interface RunProgress {
  runId: string | null;
  /** Requêtes sélectionnées, par itération. */
  requests: number;
  iterations: number;
  results: RunResult[];
  /** La requête en cours d'envoi ; `null` entre deux requêtes. */
  running: { path: string; name: string; method: string; iteration: number } | null;
  /** Les données de chaque itération vue jusqu'ici (`null` sans fichier de données). */
  rows: (Record<string, unknown> | null)[];
  waitingMs: number | null;
  warnings: string[];
}

export const idle = (): RunProgress => ({ runId: null, requests: 0, iterations: 1, results: [], running: null, rows: [], waitingMs: null, warnings: [] });

/** Ajoute un événement au suivi ; ceux d'un autre run sont ignorés. */
export function reduce(progress: RunProgress, event: RunEvent): RunProgress {
  if (event.kind === 'begin') return { ...idle(), runId: event.runId, requests: event.requests, iterations: event.iterations };
  if (event.runId !== progress.runId) return progress;
  switch (event.kind) {
    case 'iteration': {
      const rows = progress.rows.slice();
      rows[event.index] = event.row;
      return { ...progress, rows };
    }
    case 'started':
      return { ...progress, running: { path: event.path, name: event.name, method: event.method, iteration: event.iteration }, waitingMs: null };
    case 'finished':
      return { ...progress, results: [...progress.results, event.result], running: null };
    case 'waiting':
      return { ...progress, waitingMs: event.ms };
    case 'warning':
      return { ...progress, warnings: [...progress.warnings, event.message] };
  }
}

export interface RunTally {
  passed: number;
  failed: number;
  errors: number;
  skipped: number;
  /** Tests de script et assertions, comptés ensemble. */
  checks: { passed: number; total: number };
}

export function tallyResults(results: RunResult[]): RunTally {
  const out: RunTally = { passed: 0, failed: 0, errors: 0, skipped: 0, checks: { passed: 0, total: 0 } };
  for (const r of results) {
    if (r.status === 'pass') out.passed++;
    else if (r.status === 'fail') out.failed++;
    else if (r.status === 'error') out.errors++;
    else out.skipped++;
    const phases = [r.scripts.pre, r.scripts.post, r.scripts.tests].flatMap((p) => p.results);
    out.checks.total += phases.length + r.assertions.length;
    out.checks.passed += phases.filter((t) => t.status === 'pass').length + r.assertions.filter((a) => a.passed).length;
  }
  return out;
}

export type ResultFilter = 'all' | 'problems';

export const isProblem = (status: RunStatus) => status === 'fail' || status === 'error';

/** La première raison d'un échec, à montrer sans déplier la ligne ; `null` si rien n'a échoué. */
export function firstProblem(r: RunResult): string | null {
  if (r.error) return r.error.message;
  const assertion = r.assertions.find((a) => !a.passed);
  if (assertion) return assertion.error ?? `${assertion.expression} ${assertion.operator} ${assertion.expected ?? ''}`.trim() + ` : reçu ${assertion.actual}`;
  for (const phase of [r.scripts.pre, r.scripts.post, r.scripts.tests]) {
    const failed = phase.results.find((t) => t.status !== 'pass');
    if (failed) return failed.error ? `${failed.description} : ${failed.error}` : failed.description;
    if (phase.error) return phase.error;
  }
  return null;
}

export const filterResults = (results: RunResult[], filter: ResultFilter) => (filter === 'all' ? results : results.filter((r) => isProblem(r.status)));

export interface IterationGroup {
  index: number;
  row: Record<string, unknown> | null;
  results: RunResult[];
}

/** Les résultats rangés par itération, dans l'ordre ; une itération sans résultat encore n'apparaît pas. */
export function groupByIteration(results: RunResult[], rows: (Record<string, unknown> | null)[]): IterationGroup[] {
  const groups = new Map<number, IterationGroup>();
  for (const r of results) {
    const group = groups.get(r.iteration) ?? { index: r.iteration, row: rows[r.iteration] ?? null, results: [] };
    group.results.push(r);
    groups.set(r.iteration, group);
  }
  return [...groups.values()].sort((a, b) => a.index - b.index);
}

/** Une ligne de données lisible : `id=7, name=ada`. */
export function rowLabel(row: Record<string, unknown> | null): string {
  return row ? Object.entries(row).map(([k, v]) => `${k}=${typeof v === 'string' ? v : JSON.stringify(v)}`).join(', ') : '';
}

/** La part du run déjà faite, entre 0 et 1 ; les sauts de `setNextRequest` peuvent la dépasser, elle plafonne à 1. */
export function fraction(progress: RunProgress): number {
  const planned = progress.requests * progress.iterations;
  return planned ? Math.min(1, progress.results.length / planned) : 0;
}

export interface Scope {
  /** Chemin du dossier ; vide pour toute la collection. */
  path: string;
  name: string;
  depth: number;
  requests: number;
}

/** Types de requête que l'application sait envoyer ; les autres (gRPC, WebSocket) s'affichent mais ne s'ouvrent pas. */
const RUNNABLE_TYPES: readonly string[] = ['http', 'graphql'];
export const isRunnable = (requestType: string): boolean => RUNNABLE_TYPES.includes(requestType);

const countRunnable = (items: TreeItem[]): number => items.reduce((n, i) => n + (i.kind === 'folder' ? countRunnable(i.children) : isRunnable(i.requestType) ? 1 : 0), 0);

/** La collection entière puis chaque dossier qui contient au moins une requête qui se lance, dans l'ordre de l'arbre. */
export function scopes(items: TreeItem[], collectionName: string): Scope[] {
  const out: Scope[] = [{ path: '', name: collectionName, depth: 0, requests: countRunnable(items) }];
  const walk = (list: TreeItem[], depth: number) => {
    for (const item of list) {
      if (item.kind !== 'folder') continue;
      const requests = countRunnable(item.children);
      if (requests) out.push({ path: item.path, name: item.name, depth, requests });
      walk(item.children, depth + 1);
    }
  };
  walk(items, 1);
  return out;
}

export const scopeTargets = (path: string): string[] => (path ? [path] : []);

/** Le motif d'un arrêt, en une phrase. */
export function haltText(halt: RunHalt): string {
  switch (halt.kind) {
    case 'bail':
      return `Arrêté au premier échec : ${BAIL_REASONS[halt.reason] ?? halt.reason} dans « ${halt.request} »${rest(halt.remaining)}.`;
    case 'stopExecution':
      return `Arrêté par un script dans « ${halt.request} »${rest(halt.remaining)}.`;
    case 'loop':
      return 'Arrêté : trop de sauts de setNextRequest, probablement une boucle sans fin.';
    case 'cancelled':
      return 'Run annulé.';
  }
}

const rest = (n: number) => (n ? `, ${n} ${n > 1 ? 'requêtes ignorées' : 'requête ignorée'}` : '');

const BAIL_REASONS: Record<string, string> = {
  'request failure': 'la requête a échoué',
  'assertion failure': 'une assertion a échoué',
  'pre-request test failure': 'un test de pré-requête a échoué',
  'post-response test failure': 'un test de post-réponse a échoué',
  'test failure': 'un test a échoué',
};

export const SKIP_LABELS: Record<string, string> = {
  script: 'ignorée par un script',
  bail: 'ignorée après un échec',
  stopExecution: 'ignorée : un script a arrêté le run',
  unreadable: 'fichier illisible',
};

/** Durée courte : `820 ms`, `2,4 s`, `1 min 05 s`. */
export function duration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1).replace('.', ',')} s`;
  const seconds = Math.round(ms / 1000);
  return `${Math.floor(seconds / 60)} min ${String(seconds % 60).padStart(2, '0')} s`;
}

/** Le verdict d'un run terminé, d'après le résumé de Rust. */
export const verdict = (done: RunDone): 'pass' | 'fail' => (done.failed ? 'fail' : 'pass');

let counter = 0;
export const newRunId = () => `run-${Date.now().toString(36)}-${++counter}`;
