import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from './api';
import { Choice, OpView, SpecRef, SyncChange, SyncDecisions, SyncPlan, SyncReport, SyncStatus } from './model';
import { Workspace } from './store';
import {
  Selection,
  applyCount,
  choose,
  conflictPaths,
  conflictRefs,
  conflictsLeft,
  editValue,
  firstSelection,
  groupOps,
  initialEditValue,
  isValidSelection,
  keepDecisions,
  noDecisions,
  reportSummary,
  setRecreate,
  specStatus,
  stepConflict,
  toggleSkip,
  touchedFiles,
} from './sync';

const EDIT_DELAY_MS = 200;

export interface Synced {
  report: SyncReport;
  to: SpecRef;
  source: string;
}

/** État de la synchro OpenAPI : plan courant, décisions, sélection et résultat recomposé de l'opération ouverte. */
@Injectable({ providedIn: 'root' })
export class SyncStore {
  private readonly ws = inject(Workspace);

  readonly status = signal<SyncStatus | null>(null);
  readonly plan = signal<SyncPlan | null>(null);
  readonly decisions = signal<SyncDecisions>(noDecisions());
  readonly selection = signal<Selection | null>(null);
  readonly opView = signal<OpView | null>(null);
  readonly pairings = signal<[string, string][]>([]);
  readonly comparing = signal(false);
  readonly applying = signal(false);
  readonly connecting = signal(false);
  readonly showBase = signal(false);
  readonly error = signal<string | null>(null);
  readonly viewError = signal<string | null>(null);
  readonly synced = signal<Synced | null>(null);

  readonly root = computed(() => this.ws.collection()?.root ?? null);
  readonly connected = computed(() => !!this.status()?.connected);
  readonly showConnect = computed(() => this.connecting() || (!!this.status() && !this.connected() && !this.plan() && !this.synced()));
  readonly refs = computed(() => {
    const plan = this.plan();
    return plan ? conflictRefs(plan) : [];
  });
  readonly groups = computed(() => {
    const plan = this.plan();
    return plan ? groupOps(plan) : null;
  });
  readonly conflictsLeft = computed(() => conflictsLeft(this.plan(), this.decisions()));
  readonly conflictPaths = computed(() => conflictPaths(this.plan(), this.decisions()));
  readonly applyCount = computed(() => {
    const plan = this.plan();
    return plan ? applyCount(plan, this.decisions()) : 0;
  });
  readonly specStatus = computed(() => specStatus(this.plan(), this.decisions(), !!this.synced()));
  readonly selectedOp = computed(() => this.plan()?.operations.find((o) => o.key === this.selection()?.key) ?? null);
  readonly selectedChange = computed<SyncChange | null>(() => this.selectedOp()?.changes.find((c) => c.id === this.selection()?.changeId) ?? null);

  private requestedSource: string | null = null;
  private planSeq = 0;
  private viewSeq = 0;
  private statusSeq = 0;
  private viewTimer?: ReturnType<typeof setTimeout>;

  constructor() {
    effect(() => {
      const root = this.root();
      untracked(() => this.reset(root));
    });
  }

  private reset(root: string | null) {
    clearTimeout(this.viewTimer);
    this.planSeq++;
    this.viewSeq++;
    this.requestedSource = null;
    this.status.set(null);
    this.plan.set(null);
    this.decisions.set(noDecisions());
    this.selection.set(null);
    this.opView.set(null);
    this.pairings.set([]);
    this.comparing.set(false);
    this.applying.set(false);
    this.connecting.set(false);
    this.showBase.set(false);
    this.error.set(null);
    this.viewError.set(null);
    this.synced.set(null);
    if (root) void this.refreshStatus();
  }

  async refreshStatus() {
    const root = this.root();
    if (!root) return;
    const seq = ++this.statusSeq;
    try {
      const status = await api.syncStatus(root);
      if (seq === this.statusSeq && this.root() === root) this.status.set(status);
    } catch (e) {
      if (seq === this.statusSeq && this.root() === root) this.error.set(String(e));
    }
  }

  /** Ouvre la vue : compare avec la spec enregistrée si la collection est connectée et qu'aucun plan n'existe. */
  async enter() {
    if (!this.status()) await this.refreshStatus();
    if (this.connected() && !this.plan() && !this.synced() && !this.comparing() && !this.error() && !this.connecting()) await this.compare();
  }

  beginConnect() {
    this.connecting.set(true);
    this.error.set(null);
  }

  cancelConnect() {
    this.connecting.set(false);
    this.error.set(null);
  }

  /** Première comparaison avec une source choisie : sans base, la collection n'est pas encore connectée. */
  connect(source: string) {
    this.requestedSource = source;
    this.pairings.set([]);
    this.decisions.set(noDecisions());
    return this.compare();
  }

  relaunch() {
    this.synced.set(null);
    return this.compare();
  }

  pair(removed: string, added: string) {
    this.pairings.update((p) => [...p, [removed, added]]);
    return this.compare();
  }

  private async compare() {
    const root = this.root();
    if (!root) return;
    const seq = ++this.planSeq;
    this.comparing.set(true);
    this.error.set(null);
    try {
      const previous = this.plan();
      const plan = await api.syncPlan(root, this.requestedSource, this.pairings());
      if (seq !== this.planSeq || this.root() !== root) return;
      this.decisions.update((d) => keepDecisions(previous, plan, d));
      this.plan.set(plan);
      this.connecting.set(false);
      if (!isValidSelection(plan, this.selection())) this.selection.set(firstSelection(plan, this.decisions()));
      await this.refreshView();
    } catch (e) {
      if (seq === this.planSeq) this.error.set(String(e));
    } finally {
      if (seq === this.planSeq) this.comparing.set(false);
    }
  }

  select(selection: Selection | null) {
    const sameOp = selection?.key === this.selection()?.key;
    this.selection.set(selection);
    if (sameOp) return;
    this.opView.set(null);
    void this.refreshView();
  }

  next(direction: 1 | -1) {
    const target = stepConflict(this.refs(), this.selection(), direction);
    if (target) this.select({ key: target.key, changeId: target.changeId });
  }

  choose(change: SyncChange, key: string, choice: Choice) {
    const value = choice === 'edit' ? initialEditValue(change) : undefined;
    this.decisions.update((d) => choose(d, change.id, choice, value));
    this.selection.set({ key, changeId: change.id });
    void this.refreshView();
  }

  setEditValue(changeId: string, value: string) {
    this.decisions.update((d) => editValue(d, changeId, value));
    clearTimeout(this.viewTimer);
    this.viewTimer = setTimeout(() => void this.refreshView(), EDIT_DELAY_MS);
  }

  toggleSkip(key: string) {
    this.decisions.update((d) => toggleSkip(d, key));
  }

  setRecreate(key: string, recreate: boolean) {
    this.decisions.update((d) => setRecreate(d, key, recreate));
  }

  private async refreshView() {
    const plan = this.plan();
    const selection = this.selection();
    if (!plan || !selection) {
      this.opView.set(null);
      return;
    }
    const seq = ++this.viewSeq;
    try {
      const view = await api.syncOpView(plan.id, selection.key, this.decisions());
      if (seq !== this.viewSeq || this.plan()?.id !== plan.id) return;
      this.opView.set(view);
      this.viewError.set(null);
    } catch (e) {
      if (seq === this.viewSeq) this.viewError.set(String(e));
    }
  }

  /** Abandonne la synchro en cours, rien n'a été écrit. */
  abandon() {
    this.clear();
    this.ws.view.set('collections');
    this.ws.notify("Synchro abandonnée : rien n'a été écrit sur le disque.");
  }

  private clear() {
    this.planSeq++;
    this.viewSeq++;
    clearTimeout(this.viewTimer);
    this.plan.set(null);
    this.selection.set(null);
    this.opView.set(null);
    this.decisions.set(noDecisions());
    this.pairings.set([]);
    this.requestedSource = null;
    this.comparing.set(false);
    this.error.set(null);
  }

  async apply() {
    const plan = this.plan();
    const root = this.root();
    if (!plan || !root || this.applying() || this.conflictsLeft() > 0) return;
    this.applying.set(true);
    try {
      const report = await api.syncApply(plan.id, this.decisions());
      if (this.root() !== root) return;
      this.clear();
      this.synced.set({ report, to: plan.to, source: plan.source });
      await Promise.all([this.refreshStatus(), this.ws.reload()]);
      const stale = await this.ws.refreshTabs(touchedFiles(report));
      const demo = this.ws.demo ? ' Mode démo : rien n\'a été écrit sur ton disque.' : '';
      const note = stale ? ` ${stale > 1 ? `${stale} onglets modifiés n'ont` : "Un onglet modifié n'a"} pas été relu : enregistre-le avec précaution.` : '';
      this.ws.notify(`Spec synchronisée : ${reportSummary(report).toLowerCase()}.${demo}${note}`);
    } catch (e) {
      this.ws.notify(String(e), true);
    } finally {
      this.applying.set(false);
    }
  }
}
