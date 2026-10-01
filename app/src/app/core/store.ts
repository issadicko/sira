import { Injectable, computed, signal } from '@angular/core';

import { api } from './api';
import { withCurl } from './curl';
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

export interface Discard {
  message: string;
  action: string;
  resolve: (accepted: boolean) => void;
}

export type View = 'collections' | 'env';
export type DialogKind = 'curl' | 'openapi';

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
  readonly toastError = signal(false);
  readonly dialog = signal<DialogKind | null>(null);
  /** Perte de modifications à confirmer ; `null` quand aucune confirmation n'est en attente. */
  readonly discard = signal<Discard | null>(null);
  readonly hover = signal<{ name: string; rect: DOMRect } | null>(null);
  /** Saisie de la palette de commandes ; `null` quand elle est fermée. */
  readonly palette = signal<string | null>(null);

  readonly active = computed(() => this.tabs().find((t) => t.path === this.activePath()) ?? null);
  readonly modal = computed(() => !!this.dialog() || !!this.discard());
  readonly varMap = computed(() => new Map(this.vars().map((v) => [v.name, v])));

  private varsTimer?: ReturnType<typeof setTimeout>;
  private toastTimer?: ReturnType<typeof setTimeout>;
  private envSeq = 0;
  private varsSeq = 0;
  private requested = '';

  isDirty(tab: Tab): boolean {
    return JSON.stringify(tab.doc) !== tab.saved;
  }

  notify(message: string, error = false) {
    this.toast.set(message);
    this.toastError.set(error);
    clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => this.toast.set(null), error ? 5000 : 2600);
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

  /** Demande de confirmer la perte des modifications non enregistrées ; `answerDiscard` répond. */
  private confirmDiscard(message: string, action: string): Promise<boolean> {
    this.discard()?.resolve(false);
    return new Promise((resolve) => this.discard.set({ message, action, resolve }));
  }

  answerDiscard(accepted: boolean) {
    this.discard()?.resolve(accepted);
    this.discard.set(null);
  }

  /** Vrai s'il n'y a aucun onglet modifié, ou si l'utilisateur accepte de les perdre en remplaçant la collection. */
  async confirmReplace(): Promise<boolean> {
    const count = this.tabs().filter((t) => this.isDirty(t)).length;
    if (!count) return true;
    const subject = count > 1 ? `${count} onglets contiennent` : 'Un onglet contient';
    return this.confirmDiscard(
      `${subject} des modifications non enregistrées. Ouvrir une autre collection ferme tous les onglets : elles seront perdues.`,
      'Ouvrir sans enregistrer',
    );
  }

  async pickAndOpen() {
    if (!(await this.confirmReplace())) return;
    const root = await api.pickFolder();
    if (root) await this.open(root, true);
  }

  /** Ouvre une collection ; `confirmed` indique que la perte des onglets modifiés a déjà été acceptée. */
  async open(root: string, confirmed = false): Promise<boolean> {
    if (!confirmed && !(await this.confirmReplace())) return false;
    this.loading.set(true);
    this.error.set(null);
    try {
      const c = await api.openCollection(root);
      for (const tab of this.tabs()) if (tab.sendingId) void api.cancel(tab.sendingId);
      this.requested = '';
      this.collection.set(c);
      this.tabs.set([]);
      this.activePath.set(null);
      this.history.set([]);
      this.filter.set('');
      this.vars.set([]);
      this.openFolders.set(new Set(c.items.filter((i) => i.kind === 'folder').map((i) => i.path)));
      this.env.set(c.defaultEnvironment && c.environments.includes(c.defaultEnvironment) ? c.defaultEnvironment : c.environments[0] ?? null);
      const recent = [root, ...this.recent().filter((r) => r !== root)].slice(0, 6);
      this.recent.set(recent);
      persist(RECENT_KEY, recent);
      await this.loadEnv();
      const first = firstRequest(c.items);
      if (first) await this.openRequest(first, true);
      return true;
    } catch (e) {
      this.error.set(String(e));
      return false;
    } finally {
      this.loading.set(false);
    }
  }

  async reload() {
    const c = this.collection();
    if (!c) return;
    try {
      const fresh = await api.openCollection(c.root);
      if (this.collection()?.root === c.root) this.collection.set(fresh);
    } catch (e) {
      this.notify(`Impossible de relire la collection : ${e}`, true);
    }
  }

  async loadEnv() {
    const seq = ++this.envSeq;
    const c = this.collection();
    const env = this.env();
    const vars = c && env ? await api.readEnvironment(c.root, env).catch(() => []) : [];
    if (seq !== this.envSeq) return;
    this.envVars.set(vars);
    this.refreshVars();
  }

  async setEnv(env: string | null) {
    this.env.set(env);
    await this.loadEnv();
  }

  reveal(path: string) {
    const folders = path.split('/').slice(0, -1);
    this.openFolders.update((open) => new Set([...open, ...folders.map((_, i) => folders.slice(0, i + 1).join('/'))]));
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
    this.requested = path;
    if (!this.tabs().some((t) => t.path === path)) {
      try {
        const doc = await api.readRequest(c.root, path);
        if (this.requested !== path) return;
        const tab: Tab = { path, doc, saved: JSON.stringify(doc), preview: !pin };
        this.tabs.update((tabs) => {
          if (tabs.some((t) => t.path === path)) return tabs;
          const preview = tabs.findIndex((t) => t.preview && !this.isDirty(t));
          if (!pin && preview >= 0) return tabs.map((t, i) => (i === preview ? tab : t));
          return [...tabs, tab];
        });
      } catch (e) {
        this.notify(String(e), true);
        return;
      }
    }
    if (pin) this.patchTab(path, { preview: false });
    this.activePath.set(path);
    this.refreshVars();
  }

  async closeTab(path: string) {
    const tab = this.tabs().find((t) => t.path === path);
    if (!tab) return;
    const lost = `Tes modifications de « ${tab.doc.name || tab.path} » seront perdues si tu fermes l'onglet.`;
    if (this.isDirty(tab) && !(await this.confirmDiscard(lost, 'Fermer sans enregistrer'))) return;
    const tabs = this.tabs();
    const i = tabs.findIndex((t) => t.path === path);
    if (i < 0) return;
    const sendingId = tabs[i].sendingId;
    if (sendingId) void api.cancel(sendingId);
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

  /** Modifie le document de l'onglet `path` (l'onglet actif par défaut) et le marque comme non enregistré. */
  edit(change: (doc: RequestDoc) => RequestDoc, path = this.activePath()) {
    const tab = this.tabs().find((t) => t.path === path);
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

  async pasteCurl(command: string): Promise<boolean> {
    const path = this.activePath();
    if (!path) return false;
    try {
      const curl = await api.parseCurl(command);
      if (!curl?.url) {
        this.notify('Commande cURL invalide', true);
        return false;
      }
      this.edit((d) => withCurl(withUrl(d, curl.url), curl), path);
      this.notify('Commande cURL appliquée à la requête');
      return true;
    } catch (e) {
      this.notify(String(e), true);
      return false;
    }
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
      const before: RequestDoc = JSON.parse(tab.saved);
      const listed = before.name !== tab.doc.name || before.method !== tab.doc.method || before.url !== tab.doc.url;
      await api.saveRequest(c.root, tab.path, tab.doc);
      this.patchTab(tab.path, { saved: JSON.stringify(tab.doc) });
      this.notify(`Enregistré dans ${tab.path}`);
      if (listed) await this.reload();
    } catch (e) {
      this.notify(`Échec de l'enregistrement : ${e}`, true);
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
      if (this.collection()?.root === c.root) {
        this.history.update((h) => [
          { path: tab.path, method: result.method, url: result.url, status: result.response.status, at: time() },
          ...h,
        ].slice(0, 30));
      }
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
    const seq = ++this.varsSeq;
    this.varsTimer = setTimeout(async () => {
      const c = this.collection();
      const tab = this.active();
      if (!c || !tab) return;
      const vars = await api.variables(c.root, tab.path, tab.doc, this.env()).catch(() => []);
      if (seq === this.varsSeq) this.vars.set(vars);
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
