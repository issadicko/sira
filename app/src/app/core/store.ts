import { Injectable, computed, signal } from '@angular/core';

import { api } from './api';
import { CollectionInfo, EnvVar, RequestDoc, SendResult, TreeItem, VariableInfo } from './model';
import { withParams, withUrl } from './url';

export interface Tab {
  path: string;
  doc: RequestDoc;
  saved: string;
  preview: boolean;
  result?: SendResult;
  error?: string;
  sendingId?: string;
  sentAt?: number;
}

export interface HistoryEntry {
  path: string;
  method: string;
  url: string;
  status?: number;
  at: string;
}

export type View = 'collections' | 'env';

const RECENT_KEY = 'xc-recent';

function storage<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function persist(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* stockage indisponible */
  }
}

@Injectable({ providedIn: 'root' })
export class Workspace {
  readonly demo = api.demo;
  readonly collection = signal<CollectionInfo | null>(null);
  readonly loading = signal(false);
  readonly error = signal<string | null>(null);
  readonly tabs = signal<Tab[]>([]);
  readonly activePath = signal<string | null>(null);
  readonly env = signal<string | null>(null);
  readonly envVars = signal<EnvVar[]>([]);
  readonly vars = signal<VariableInfo[]>([]);
  readonly history = signal<HistoryEntry[]>([]);
  readonly openFolders = signal<Set<string>>(new Set());
  readonly view = signal<View>('collections');
  readonly sidebar = signal(true);
  readonly stacked = signal(false);
  readonly filter = signal('');
  readonly recent = signal<string[]>(storage(RECENT_KEY, []));
  readonly theme = signal<'dark' | 'light'>(document.documentElement.dataset['theme'] === 'light' ? 'light' : 'dark');
  readonly toast = signal<string | null>(null);
  readonly hover = signal<{ name: string; rect: DOMRect } | null>(null);

  readonly active = computed(() => this.tabs().find((t) => t.path === this.activePath()) ?? null);
  readonly varMap = computed(() => new Map(this.vars().map((v) => [v.name, v])));

  private varsTimer?: ReturnType<typeof setTimeout>;
  private toastTimer?: ReturnType<typeof setTimeout>;

  isDirty(tab: Tab): boolean {
    return JSON.stringify(tab.doc) !== tab.saved;
  }

  notify(message: string) {
    this.toast.set(message);
    clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => this.toast.set(null), 2600);
  }

  toggleTheme() {
    const next = this.theme() === 'dark' ? 'light' : 'dark';
    this.theme.set(next);
    document.documentElement.dataset['theme'] = next;
    try {
      localStorage.setItem('xc-theme', next);
    } catch {
      /* stockage indisponible */
    }
  }

  async pickAndOpen() {
    const root = await api.pickFolder();
    if (root) await this.open(root);
  }

  async open(root: string) {
    this.loading.set(true);
    this.error.set(null);
    try {
      const c = await api.openCollection(root);
      this.collection.set(c);
      this.tabs.set([]);
      this.activePath.set(null);
      this.openFolders.set(new Set(c.items.filter((i) => i.kind === 'folder').map((i) => i.path)));
      this.env.set(c.defaultEnvironment && c.environments.includes(c.defaultEnvironment) ? c.defaultEnvironment : c.environments[0] ?? null);
      const recent = [root, ...this.recent().filter((r) => r !== root)].slice(0, 6);
      this.recent.set(recent);
      persist(RECENT_KEY, recent);
      await this.loadEnv();
      const first = firstRequest(c.items);
      if (first) await this.openRequest(first, true);
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.loading.set(false);
    }
  }

  async reload() {
    const c = this.collection();
    if (!c) return;
    this.collection.set(await api.openCollection(c.root));
  }

  async loadEnv() {
    const c = this.collection();
    const env = this.env();
    this.envVars.set(c && env ? await api.readEnvironment(c.root, env).catch(() => []) : []);
    this.refreshVars();
  }

  async setEnv(env: string | null) {
    this.env.set(env);
    await this.loadEnv();
  }

  toggleFolder(path: string) {
    const next = new Set(this.openFolders());
    if (next.has(path)) next.delete(path);
    else next.add(path);
    this.openFolders.set(next);
  }

  async openRequest(path: string, pin = false) {
    const c = this.collection();
    if (!c) return;
    this.view.set('collections');
    if (!this.tabs().some((t) => t.path === path)) {
      try {
        const doc = await api.readRequest(c.root, path);
        const tab: Tab = { path, doc, saved: JSON.stringify(doc), preview: !pin };
        this.tabs.update((tabs) => {
          const preview = tabs.findIndex((t) => t.preview && !this.isDirty(t));
          if (!pin && preview >= 0) return tabs.map((t, i) => (i === preview ? tab : t));
          return [...tabs, tab];
        });
      } catch (e) {
        this.notify(String(e));
        return;
      }
    } else if (pin) {
      this.patchTab(path, { preview: false });
    }
    this.activePath.set(path);
    this.refreshVars();
  }

  closeTab(path: string) {
    const tabs = this.tabs();
    const i = tabs.findIndex((t) => t.path === path);
    const tab = tabs[i];
    if (tab?.sendingId) void api.cancel(tab.sendingId);
    const rest = tabs.filter((t) => t.path !== path);
    this.tabs.set(rest);
    if (this.activePath() === path) this.activePath.set(rest[Math.min(i, rest.length - 1)]?.path ?? null);
    this.refreshVars();
  }

  activate(path: string) {
    this.activePath.set(path);
    this.refreshVars();
  }

  pin(path: string) {
    this.patchTab(path, { preview: false });
  }

  private patchTab(path: string, patch: Partial<Tab>) {
    this.tabs.update((tabs) => tabs.map((t) => (t.path === path ? { ...t, ...patch } : t)));
  }

  edit(change: (doc: RequestDoc) => RequestDoc) {
    const tab = this.active();
    if (!tab) return;
    this.patchTab(tab.path, { doc: change(tab.doc), preview: false });
    this.refreshVars();
  }

  setUrl(url: string) {
    this.edit((d) => withUrl(d, url));
  }

  setParams(params: RequestDoc['params']) {
    this.edit((d) => withParams(d, params));
  }

  async save() {
    const c = this.collection();
    const tab = this.active();
    if (!c || !tab) return;
    if (!this.isDirty(tab)) {
      this.notify(`Rien à enregistrer dans ${tab.path}`);
      return;
    }
    try {
      const nameChanged = JSON.parse(tab.saved).name !== tab.doc.name;
      await api.saveRequest(c.root, tab.path, tab.doc);
      this.patchTab(tab.path, { saved: JSON.stringify(tab.doc) });
      this.notify(`Enregistré dans ${tab.path}`);
      if (nameChanged) await this.reload();
    } catch (e) {
      this.notify(`Échec de l'enregistrement : ${e}`);
    }
  }

  async send() {
    const c = this.collection();
    const tab = this.active();
    if (!c || !tab) return;
    if (tab.sendingId) return this.cancel();
    const id = crypto.randomUUID();
    const started = performance.now();
    this.patchTab(tab.path, { sendingId: id, error: undefined });
    try {
      const result = await api.send(id, c.root, tab.path, tab.doc, this.env());
      this.patchTab(tab.path, { result, sendingId: undefined, sentAt: Date.now() });
      this.history.update((h) => [
        { path: tab.path, method: result.method, url: result.url, status: result.response.status, at: time() },
        ...h,
      ].slice(0, 30));
    } catch (e) {
      const elapsed = Math.round(performance.now() - started);
      const message = String(e);
      this.patchTab(tab.path, {
        sendingId: undefined,
        result: undefined,
        error: message === 'Requête annulée' ? `Requête annulée après ${elapsed} ms. Rien n'a été enregistré.` : message,
      });
    }
  }

  async cancel() {
    const id = this.active()?.sendingId;
    if (id) await api.cancel(id);
  }

  refreshVars() {
    clearTimeout(this.varsTimer);
    this.varsTimer = setTimeout(async () => {
      const c = this.collection();
      const tab = this.active();
      if (!c || !tab) return;
      this.vars.set(await api.variables(c.root, tab.path, tab.doc, this.env()).catch(() => []));
    }, 120);
  }
}

function firstRequest(items: TreeItem[]): string | null {
  for (const item of items) {
    if (item.kind === 'request' && item.requestType === 'http') return item.path;
    if (item.kind === 'folder') {
      const found = firstRequest(item.children);
      if (found) return found;
    }
  }
  return null;
}

function time(): string {
  return new Date().toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
}
