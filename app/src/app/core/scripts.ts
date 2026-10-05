import type { AssertionResult, LogLine, PhaseReport, Script, ScriptsReport } from './model';

export interface ScriptKind {
  kind: string;
  label: string;
  hint: string;
}

/** Les trois scripts d'une requête, dans l'ordre où Bruno les écrit dans le fichier. */
export const SCRIPT_KINDS: ScriptKind[] = [
  { kind: 'before-request', label: 'Avant la requête', hint: 'req, bru : modifie la requête ou pose des variables avant l\'envoi' },
  { kind: 'after-response', label: 'Après la réponse', hint: 'res, bru : lis la réponse, garde un jeton dans une variable' },
  { kind: 'tests', label: 'Tests', hint: 'test(), expect() : vérifie la réponse' },
];

export const emptyReport = (): ScriptsReport => ({
  pre: { results: [], logs: [] },
  post: { results: [], logs: [] },
  tests: { results: [], logs: [] },
});

export function scriptCode(scripts: Script[], kind: string): string {
  return scripts.find((s) => s.kind === kind)?.code ?? '';
}

/** Les scripts avec le code de `kind` remplacé, l'ordre du fichier gardé ; un code vide retire le script. */
export function withScript(scripts: Script[], kind: string, code: string): Script[] {
  const others = scripts.filter((s) => s.kind !== kind);
  const next = code.trim() ? [...others, { kind, code }] : others;
  const rank = (k: string) => {
    const i = SCRIPT_KINDS.findIndex((s) => s.kind === k);
    return i < 0 ? SCRIPT_KINDS.length : i;
  };
  return next.sort((a, b) => rank(a.kind) - rank(b.kind));
}

const phases = (report: ScriptsReport): PhaseReport[] => [report.pre, report.post, report.tests];

export interface Tally {
  passed: number;
  total: number;
}

/** Tests de script et assertions déclaratives comptés ensemble. */
export function tally(report: ScriptsReport, assertions: AssertionResult[]): Tally {
  const results = phases(report).flatMap((p) => p.results);
  const passed = results.filter((r) => r.status === 'pass').length + assertions.filter((a) => a.passed).length;
  return { passed, total: results.length + assertions.length };
}

export const hasLogs = (report: ScriptsReport) => phases(report).some((p) => p.logs.length > 0);

const show = (value: unknown): string => (typeof value === 'string' ? value : (JSON.stringify(value) ?? String(value)));

/** Une ligne de `console` comme `console.log` l'imprimerait. */
export const formatLog = (line: LogLine): string => line.args.map(show).join(' ');

/** Les valeurs d'un test en échec, quand chai les a fournies. */
export function expectation(result: { actual?: unknown; expected?: unknown }): string | null {
  if (result.actual === undefined && result.expected === undefined) return null;
  return `attendu ${show(result.expected)} · reçu ${show(result.actual)}`;
}
