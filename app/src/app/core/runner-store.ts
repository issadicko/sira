import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from './api';
import { DataInfo, ReportFormat, RunDone, RunResult } from './model';
import { ResultFilter, RunProgress, filterResults, groupByIteration, idle, newRunId, reduce, scopeTargets, scopes, tallyResults } from './runner';
import { Workspace } from './store';

const EXTENSIONS: Record<ReportFormat, string> = { html: 'html', junit: 'xml', json: 'json' };

const slug = (name: string) => name.normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/[^a-zA-Z0-9]+/g, '-').replace(/^-|-$/g, '').toLowerCase() || 'collection';

/** Le runner : ce qu'on exécute (portée, délai, arrêt au premier échec, données), le run en cours ou le dernier, son export. */
@Injectable({ providedIn: 'root' })
export class RunnerStore {
  private readonly ws = inject(Workspace);

  readonly scope = signal('');
  readonly delay = signal(0);
  readonly bail = signal(false);
  readonly data = signal<{ path: string; info: DataInfo } | null>(null);
  readonly progress = signal<RunProgress>(idle());
  readonly running = signal(false);
  readonly cancelling = signal(false);
  readonly done = signal<RunDone | null>(null);
  readonly error = signal<string | null>(null);
  readonly filter = signal<ResultFilter>('all');
  readonly expanded = signal<ReadonlySet<RunResult>>(new Set());
  readonly exporting = signal<ReportFormat | null>(null);
  readonly skipHeaders = signal(false);
  readonly skipBodies = signal(false);

  readonly scopeList = computed(() => scopes(this.ws.collection()?.items ?? [], this.ws.collection()?.name ?? ''));
  readonly selected = computed(() => this.scopeList().find((s) => s.path === this.scope()) ?? this.scopeList()[0]);
  readonly tally = computed(() => tallyResults(this.progress().results));
  readonly shown = computed(() => filterResults(this.progress().results, this.filter()));
  readonly groups = computed(() => groupByIteration(this.shown(), this.progress().rows));
  readonly started = computed(() => this.progress().runId !== null);
  readonly problems = computed(() => this.tally().failed + this.tally().errors);

  private root: string | null = null;
  private listening: Promise<unknown> | null = null;

  constructor() {
    effect(() => {
      const root = this.ws.collection()?.root ?? null;
      untracked(() => this.follow(root));
    });
  }

  /** Ouvre le runner sur `path` (dossier ; vide pour toute la collection). */
  openFor(path = '') {
    if (!this.running()) this.scope.set(path);
    this.ws.view.set('runner');
    this.ws.sidebar.set(true);
  }

  async run() {
    const c = this.ws.collection();
    const scope = this.selected();
    if (!c || !scope || this.running()) return;
    const runId = newRunId();
    this.error.set(null);
    this.done.set(null);
    this.cancelling.set(false);
    this.expanded.set(new Set());
    this.running.set(true);
    this.progress.set({ ...idle(), runId });
    try {
      await this.listen();
      const done = await api.startRun({
        runId,
        root: c.root,
        targets: scopeTargets(scope.path),
        env: this.ws.env(),
        bail: this.bail(),
        delayMs: Math.max(0, Math.round(this.delay()) || 0),
        data: this.data()?.path ?? null,
      });
      this.done.set(done);
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.running.set(false);
      this.cancelling.set(false);
    }
  }

  async cancel() {
    const id = this.progress().runId;
    if (!id || !this.running() || this.cancelling()) return;
    this.cancelling.set(true);
    try {
      await api.cancelRun(id);
    } catch (e) {
      this.cancelling.set(false);
      this.ws.notify(String(e), true);
    }
  }

  async pickData() {
    try {
      const path = await api.pickDataFile();
      if (path) this.data.set({ path, info: await api.inspectRunData(path) });
    } catch (e) {
      this.ws.notify(String(e), true);
    }
  }

  clearData() {
    this.data.set(null);
  }

  toggle(result: RunResult) {
    this.expanded.update((set) => {
      const next = new Set(set);
      if (!next.delete(result)) next.add(result);
      return next;
    });
  }

  async export(format: ReportFormat) {
    const c = this.ws.collection();
    const done = this.done();
    if (!c || !done || this.exporting()) return;
    try {
      const path = await api.pickSavePath(`rapport-${slug(c.name)}.${EXTENSIONS[format]}`, format);
      if (!path) return;
      this.exporting.set(format);
      await api.exportRun({ runId: done.runId, root: c.root, format, path, skipHeaders: this.skipHeaders(), skipBodies: this.skipBodies() });
      this.ws.notify(`Rapport enregistré : ${path}`);
    } catch (e) {
      this.ws.notify(String(e), true);
    } finally {
      this.exporting.set(null);
    }
  }

  /** S'abonne une fois : un événement qui arrive après la réponse de `startRun` n'est pas perdu. */
  private listen() {
    this.listening ??= api.onRunEvent((event) => this.progress.update((p) => reduce(p, event)));
    return this.listening;
  }

  private follow(root: string | null) {
    if (root === this.root) return;
    this.root = root;
    if (this.running()) void this.cancel();
    this.progress.set(idle());
    this.done.set(null);
    this.error.set(null);
    this.expanded.set(new Set());
    this.scope.set('');
    this.data.set(null);
  }
}
