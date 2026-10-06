import type { Api } from './api';
import { DemoCollection, collectionAt } from './demo-tree';
import { emptyReport } from './scripts';
import { DataInfo, ReportFormat, RunArgs, RunDone, RunEvent, RunHalt, RunResult, RunSummary, TreeItem } from './model';
import { tallyResults } from './runner';

type DemoRunner = Pick<Api, 'pickDataFile' | 'pickSavePath' | 'inspectRunData' | 'startRun' | 'cancelRun' | 'onRunEvent' | 'exportRun'>;

const DATA = { path: '~/démo/transactions.csv', info: { rows: 3, columns: ['txId', 'canal'] } satisfies DataInfo };
const ROWS: Record<string, unknown>[] = [
  { txId: 'TX-0043', canal: 'ussd' },
  { txId: 'TX-0044', canal: 'web' },
  { txId: 'TX-0045', canal: 'ussd' },
];

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Les requêtes HTTP sous `targets` (toute la collection si vide), dans l'ordre de l'arbre. */
function requestsUnder(items: TreeItem[], targets: string[]): TreeItem[] {
  const out: TreeItem[] = [];
  const walk = (list: TreeItem[], inside: boolean) => {
    for (const item of list) {
      if (item.kind === 'folder') walk(item.children, inside || targets.includes(item.path));
      else if (item.requestType === 'http' && (inside || targets.includes(item.path))) out.push(item);
    }
  };
  walk(items, !targets.length);
  return out;
}

const sum = (results: RunResult[]): RunSummary => {
  const t = tallyResults(results);
  const failedAssertions = results.reduce((n, r) => n + r.assertions.filter((a) => !a.passed).length, 0);
  const totalAssertions = results.reduce((n, r) => n + r.assertions.length, 0);
  return {
    totalRequests: results.length,
    passedRequests: t.passed,
    failedRequests: t.failed,
    errorRequests: t.errors,
    skippedRequests: t.skipped,
    skippedByBail: results.filter((r) => r.skipped === 'bail').length,
    totalAssertions,
    passedAssertions: totalAssertions - failedAssertions,
    failedAssertions,
    totalTests: 0,
    passedTests: 0,
    failedTests: 0,
    totalPreRequestTests: 0,
    passedPreRequestTests: 0,
    failedPreRequestTests: 0,
    totalPostResponseTests: 0,
    passedPostResponseTests: 0,
    failedPostResponseTests: 0,
  };
};

export function createDemoRunner(collections: Map<string, DemoCollection>): DemoRunner {
  const handlers = new Set<(event: RunEvent) => void>();
  const cancelled = new Set<string>();
  const emit = (event: RunEvent) => handlers.forEach((h) => h(event));

  return {
    pickDataFile: async () => DATA.path,
    pickSavePath: async (defaultName: string, _format: ReportFormat) => `~/démo/${defaultName}`,
    inspectRunData: async () => structuredClone(DATA.info),
    cancelRun: async (runId) => {
      cancelled.add(runId);
      return true;
    },
    onRunEvent: async (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    exportRun: async () => undefined,
    startRun: async (args: RunArgs): Promise<RunDone> => {
      const c = collectionAt(collections, args.root);
      const items = requestsUnder(c.info.items, args.targets);
      const rows = args.data ? ROWS : [null];
      const { runId } = args;
      const results: RunResult[] = [];
      const begun = performance.now();
      let halt: RunHalt | null = null;
      emit({ kind: 'begin', runId, requests: items.length, iterations: rows.length });

      run: for (const [iteration, row] of rows.entries()) {
        emit({ kind: 'iteration', runId, index: iteration, total: rows.length, row });
        for (const [position, item] of items.entries()) {
          if (item.kind !== 'request') continue;
          if (cancelled.delete(runId)) {
            halt = { kind: 'cancelled' };
            break run;
          }
          emit({ kind: 'started', runId, iteration, path: item.path, name: item.name, method: item.method });
          await wait(260 + Math.round(Math.random() * 240));
          const doc = c.files[item.path];
          const forbidden = item.method === 'DELETE';
          const assertions = (doc?.assertions ?? [])
            .filter((a) => a.enabled)
            .map((a) => ({
              expression: a.expression,
              operator: a.operator,
              expected: a.value,
              actual: forbidden && a.expression === 'res.status' ? '403' : '200',
              passed: !forbidden && (a.expression === 'res.status' || a.value === 'XOF'),
            }));
          const timeMs = 80 + Math.round(Math.random() * 90);
          const failed = assertions.some((a) => !a.passed);
          const result: RunResult = {
            iteration,
            name: item.name,
            path: item.path,
            method: item.method,
            url: item.url.replace(/\{\{([^}]+)\}\}/g, (all, n: string) => (n === 'baseUrl' ? 'https://api.paiements.example' : (row?.[n] as string | undefined) ?? all)),
            status: failed ? 'fail' : 'pass',
            skipped: null,
            http: { status: forbidden ? 403 : 200, reason: forbidden ? 'Forbidden' : 'OK', size: 420, timeMs },
            error: null,
            assertions,
            scripts: emptyReport(),
            durationMs: timeMs + 12,
          };
          results.push(result);
          emit({ kind: 'finished', runId, result });
          if (failed && args.bail) {
            const rest = items.slice(position + 1);
            for (const skipped of rest) {
              if (skipped.kind !== 'request') continue;
              const placeholder: RunResult = { ...result, name: skipped.name, path: skipped.path, method: skipped.method, url: skipped.url, status: 'skipped', skipped: 'bail', http: null, assertions: [], durationMs: 0 };
              results.push(placeholder);
              emit({ kind: 'finished', runId, result: placeholder });
            }
            halt = { kind: 'bail', request: item.name, reason: 'assertion failure', remaining: rest.length };
            break run;
          }
          if (args.delayMs && position < items.length - 1) {
            emit({ kind: 'waiting', runId, ms: args.delayMs });
            await wait(args.delayMs);
          }
        }
      }
      const summary = sum(results);
      return { runId, summary, halt, failed: summary.failedRequests + summary.errorRequests > 0, elapsedMs: performance.now() - begun };
    },
  };
}
