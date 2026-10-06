import { Injectable, computed, signal } from '@angular/core';

import { api } from './api';
import { withCurl } from './curl';
import { affectedTabs, describeDiskChange, mergeChanges, touchesEnvironments, verdictFor } from './disk-sync';
import { CollectionInfo, DiskChange, EnvVar, RequestDoc, ScriptsReport, Sent, TreeItem, VariableInfo } from './model';
import { closeUnder, isUnder, missingPaths, remapPath, remapPaths, remapSet } from './tree-ops';
import { withParams, withUrl } from './url';

export interface Tab {
  path: string;
  doc: RequestDoc;
  saved: string;
  /** Le fichier tel que Rust l'a lu la dernière fois (ouverture, enregistrement, relecture) : ce à quoi on compare le disque. */
  base: string;
  preview: boolean;
  result?: Sent;
  /** Ce que les scripts ont produit au dernier envoi, même quand la requête n'a pas abouti. */
  report?: ScriptsReport;
  skipped?: boolean;
  error?: string;
  sendingId?: string;
  sentAt?: number;
  /** Le fichier n'existe plus sur le disque : l'onglet reste ouvert pour ne pas perdre le brouillon. */
  missing?: boolean;
  /** Le fichier a changé sur le disque alors que l'onglet a un brouillon : l'enregistrer écraserait ce changement. */
  stale?: boolean;
}

/** Résultat de la relecture d'onglets : relus sans brouillon, ou périmés parce qu'un brouillon s'y oppose. */
export interface TabRefresh {
  reloaded: number;
  stale: string[];
}

export interface HistoryEntry {
  path: string;
  method: string;
  url: string;
  status?: number;
  at: string;
}

export interface Discard {
  heading?: string;
  message: string;
  action: string;
  resolve: (accepted: boolean) => void;
}

export type View = 'collections' | 'env' | 'sync' | 'runner' | 'settings';
export type Theme = 'dark' | 'light';
export type DialogKind = 'curl' | 'openapi' | 'collection' | 'delete' | 'move' | 'env';

/** Dossier existant, vide ou non, où l'on propose de créer une collection. */
export interface FolderTarget {
  dir: string;
  kind: 'empty' | 'other';
}

const BRU_MESSAGE = 'Collection au format .bru : lecture prévue en V1';

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
  /** Une commande de gestion de collection (créer, renommer, déplacer, supprimer) écrit sur le disque. */
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  readonly tabs = signal<Tab[]>([]);
  readonly activePath = signal<string | null>(null);
  readonly env = signal<string | null>(null);
  readonly envVars = signal<EnvVar[]>([]);
  /** Pourquoi le fichier de l'environnement actif n'a pas pu être relu (YAML invalide…) ; `null` quand il l'a été. Les variables gardent alors la dernière lecture. */
  readonly envError = signal<string | null>(null);
  /** Le brouillon de l'environnement actif a des modifications non enregistrées (renseigné par `EnvStore`). */
  readonly envDirty = signal(false);
  readonly vars = signal<VariableInfo[]>([]);
  readonly history = signal<HistoryEntry[]>([]);
  readonly openFolders = signal<Set<string>>(new Set());
  readonly view = signal<View>('collections');
  readonly sidebar = signal(true);
  readonly stacked = signal(false);
  readonly filter = signal('');
  readonly recent = signal<string[]>(storage(RECENT_KEY, []));
  readonly theme = signal<Theme>(document.documentElement.dataset['theme'] === 'light' ? 'light' : 'dark');
  readonly toast = signal<string | null>(null);
  readonly toastError = signal(false);
  readonly dialog = signal<DialogKind | null>(null);
  /** Dossier du dialogue « Créer une collection ici » ; `null` pour « Nouvelle collection », où l'on choisit le dossier parent. */
  readonly folderTarget = signal<FolderTarget | null>(null);
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
  private envLoaded: string | null = null;
  private varsSeq = 0;
  private requested = '';
  private viewBeforeSettings: View = 'collections';
  private writes = 0;
  private writeSeq = 0;
  private diskPending: DiskChange | null = null;
  private diskRunning = false;
  private lastRearm = 0;
  private lastRescan = 0;

  constructor() {
    void api.onDiskChange((change) => {
      if (change.truncated) this.rearm(change.root);
      void this.onDiskChange(change);
    });
  }

  /** Exécute une écriture dans la collection : la relecture déclenchée par le disque l'attend et ignore ce qu'elle a lu pendant ce temps. */
  async writing<T>(action: () => Promise<T>): Promise<T> {
    this.writes++;
    this.writeSeq++;
    try {
      return await action();
    } finally {
      this.writes--;
      this.writeSeq++;
    }
  }

  /** Relance la surveillance : un lot tronqué peut signaler qu'elle est morte (dossier renommé ou recréé), au plus une fois par 5 s. */
  private rearm(root: string) {
    if (this.collection()?.root !== root || Date.now() - this.lastRearm < 5000) return;
    this.lastRearm = Date.now();
    api.watchCollection(root).catch(() => undefined);
  }

  /** Relit tout depuis le disque, au retour dans la fenêtre : filet de sécurité si des événements ont été perdus. Au plus une fois par 3 s. */
  async rescan() {
    const c = this.collection();
    if (!c || this.loading() || Date.now() - this.lastRescan < 3000) return;
    this.lastRescan = Date.now();
    this.rearm(c.root);
    await this.onDiskChange({ root: c.root, paths: [], truncated: true });
  }

  isDirty(tab: Tab): boolean {
    return JSON.stringify(tab.doc) !== tab.saved;
  }

  notify(message: string, error = false) {
    this.toast.set(message);
    this.toastError.set(error);
    clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => this.toast.set(null), error ? 5000 : 2600);
  }

  setTheme(theme: Theme) {
    this.theme.set(theme);
    document.documentElement.dataset['theme'] = theme;
    try {
      localStorage.setItem('xc-theme', theme);
    } catch {
      /* stockage indisponible */
    }
  }

  toggleTheme() {
    this.setTheme(this.theme() === 'dark' ? 'light' : 'dark');
  }

  openSettings() {
    if (this.view() === 'settings') return;
    this.viewBeforeSettings = this.view();
    this.view.set('settings');
  }

  closeSettings() {
    if (this.view() === 'settings') this.view.set(this.viewBeforeSettings);
  }

  /** Demande de confirmer une perte ou un écrasement ; `answerDiscard` répond. */
  confirmDiscard(message: string, action: string, heading?: string): Promise<boolean> {
    this.discard()?.resolve(false);
    return new Promise((resolve) => this.discard.set({ heading, message, action, resolve }));
  }

  answerDiscard(accepted: boolean) {
    this.discard()?.resolve(accepted);
    this.discard.set(null);
  }

  /** Vrai s'il n'y a aucun onglet modifié, ou si l'utilisateur accepte de les perdre en remplaçant la collection. */
  async confirmReplace(): Promise<boolean> {
    const count = this.tabs().filter((t) => this.isDirty(t)).length;
    const env = this.envDirty();
    if (!count && !env) return true;
    const subject = count > 1 ? `${count} onglets contiennent` : count ? 'Un onglet contient' : "L'environnement actif contient";
    const also = count && env ? " L'environnement actif a aussi des modifications non enregistrées." : '';
    const closing = count ? 'ferme tous les onglets' : 'remplace les environnements';
    return this.confirmDiscard(
      `${subject} des modifications non enregistrées.${also} Ouvrir une autre collection ${closing} : elles seront perdues.`,
      'Ouvrir sans enregistrer',
    );
  }

  /** Vrai, avec un message, tant qu'une commande de collection écrit sur le disque : on n'ouvre ni n'enregistre rien d'autre. */
  private blocked(): boolean {
    if (this.busy()) this.notify('Une opération sur la collection est en cours : réessaie dans un instant.', true);
    return this.busy();
  }

  async pickAndOpen() {
    if (this.blocked() || !(await this.confirmReplace())) return;
    const dir = await api.pickFolder();
    if (dir) await this.openFolder(dir);
  }

  /** Ouvre le dossier s'il contient une collection ; sinon propose d'en créer une ici, ou explique pourquoi on ne peut pas. */
  private async openFolder(dir: string) {
    try {
      const kind = await api.inspectFolder(dir);
      if (kind === 'collection') {
        await this.open(dir, true);
      } else if (kind === 'bru') {
        this.refuse(BRU_MESSAGE);
      } else {
        this.folderTarget.set({ dir, kind });
        this.dialog.set('collection');
      }
    } catch (e) {
      this.refuse(String(e));
    }
  }

  async newCollection() {
    if (this.blocked() || !(await this.confirmReplace())) return;
    this.folderTarget.set(null);
    this.dialog.set('collection');
  }

  private refuse(message: string) {
    if (this.collection()) this.notify(message, true);
    else this.error.set(message);
  }

  /** Ouvre une collection ; `confirmed` indique que la perte des onglets modifiés a déjà été acceptée. */
  async open(root: string, confirmed = false): Promise<boolean> {
    if (this.blocked() || (!confirmed && !(await this.confirmReplace()))) return false;
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
      this.diskPending = null;
      this.env.set(initialEnv(c));
      this.envLoaded = null;
      const recent = [root, ...this.recent().filter((r) => r !== root)].slice(0, 6);
      this.recent.set(recent);
      persist(RECENT_KEY, recent);
      api.watchCollection(root).catch((e) => this.notify(`Rechargement à chaud indisponible : ${e}. « Relire le dossier » reste disponible.`, true));
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

  /**
   * Relit depuis le disque les onglets des fichiers donnés. Un onglet sans brouillon adopte le fichier ; un onglet avec brouillon est
   * marqué périmé si le fichier a changé, et redevient normal si le fichier a retrouvé la version enregistrée.
   */
  async refreshTabs(paths: string[]): Promise<TabRefresh> {
    const outcome: TabRefresh = { reloaded: 0, stale: [] };
    const c = this.collection();
    if (!c) return outcome;
    const open = this.tabs().filter((t) => paths.includes(t.path) && !t.missing);
    await Promise.all(
      open.map(async (t) => {
        const seq = this.writeSeq;
        const disk = await api.readRequest(c.root, t.path).catch(() => null);
        const now = this.tabs().find((x) => x.path === t.path);
        if (!disk || !now || this.collection()?.root !== c.root || this.writeSeq !== seq) return;
        const text = JSON.stringify(disk);
        switch (verdictFor(now.base, now.saved, JSON.stringify(now.doc), text)) {
          case 'adopt':
            this.patchTab(t.path, { doc: disk, saved: text, base: text, stale: false });
            outcome.reloaded++;
            break;
          case 'stale':
            this.patchTab(t.path, { stale: true });
            if (!now.stale) outcome.stale.push(now.doc.name || now.path);
            break;
          case 'unchanged':
            if (now.stale) this.patchTab(t.path, { stale: false });
            break;
        }
      }),
    );
    this.refreshVars();
    return outcome;
  }

  /** Traite les changements du disque (pull Git, éditeur externe) : une seule relecture à la fois, les lots reçus entre-temps sont réunis. */
  async onDiskChange(change: DiskChange) {
    this.diskPending = mergeChanges(this.diskPending, change);
    if (this.diskRunning) return;
    this.diskRunning = true;
    try {
      for (let next: DiskChange | null = this.diskPending; next; next = this.diskPending) {
        this.diskPending = null;
        await this.applyDiskChange(next);
      }
    } finally {
      this.diskRunning = false;
    }
  }

  private async applyDiskChange(change: DiskChange) {
    await this.settled();
    const c = this.collection();
    if (!c || c.root !== change.root || this.loading()) return;
    await this.reload();
    const fresh = this.collection();
    if (!fresh || fresh.root !== change.root) return;
    const before = this.tabs();
    this.reconcileTabs();
    const closed = before.filter((t) => !this.tabs().some((x) => x.path === t.path)).map((t) => t.doc.name || t.path);
    const refreshed = await this.refreshTabs(affectedTabs(change, this.tabs().map((t) => t.path)));
    const env = this.env();
    if (env && !fresh.environments.includes(env) && !this.envDirty()) this.env.set(initialEnv(fresh));
    if (touchesEnvironments(change)) await this.loadEnv();
    else this.refreshVars();
    const told = describeDiskChange({ closed, reloaded: refreshed.reloaded, stale: refreshed.stale });
    if (told) this.notify(told);
  }

  /** Laisse finir une écriture en cours (enregistrement, commande de collection) avant de relire : la relecture la croiserait. */
  private async settled() {
    for (let waited = 0; (this.busy() || this.writes > 0) && waited < 5000; waited += 50) {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }

  /** Remplace le contenu de l'onglet par le fichier du disque, après confirmation s'il y a un brouillon à perdre. */
  async reloadFromDisk(path: string) {
    const c = this.collection();
    const tab = this.tabs().find((t) => t.path === path);
    if (!c || !tab) return;
    const lost = `Ton brouillon de « ${tab.doc.name || tab.path} » sera remplacé par le fichier tel qu'il est sur le disque.`;
    if (this.isDirty(tab) && !(await this.confirmDiscard(lost, 'Recharger depuis le disque', 'Recharger depuis le disque'))) return;
    try {
      const doc = await api.readRequest(c.root, path);
      const text = JSON.stringify(doc);
      this.patchTab(path, { doc, saved: text, base: text, stale: false, missing: false });
      this.refreshVars();
    } catch (e) {
      this.notify(String(e), true);
    }
  }

  async loadEnv() {
    const seq = ++this.envSeq;
    const c = this.collection();
    const env = this.env();
    let vars: EnvVar[] | null = [];
    let error: string | null = null;
    if (c && env) {
      try {
        vars = await api.readEnvironment(c.root, env);
      } catch (e) {
        [vars, error] = [null, String(e)];
      }
    }
    if (seq !== this.envSeq) return;
    this.envError.set(error);
    if (vars || this.envLoaded !== env) {
      this.envVars.set(vars ?? []);
      this.envLoaded = env;
    }
    this.refreshVars();
  }

  /** Change d'environnement actif ; des modifications non enregistrées de l'environnement actuel demandent confirmation. Faux si l'utilisateur renonce. */
  async setEnv(env: string | null): Promise<boolean> {
    if (env === this.env()) return true;
    const lost = `Tes modifications de « ${this.env()} » ne sont pas enregistrées : changer d'environnement les perd.`;
    if (this.envDirty() && !(await this.confirmDiscard(lost, 'Changer sans enregistrer'))) return false;
    await this.useEnv(env);
    return true;
  }

  /** Change d'environnement actif sans rien demander. */
  async useEnv(env: string | null) {
    this.env.set(env);
    await this.loadEnv();
  }

  /** Ouvre les dossiers qui mènent à `path` ; avec `folder`, `path` est un dossier à ouvrir lui aussi. */
  reveal(path: string, folder = false) {
    const parts = path.split('/');
    const folders = folder ? parts : parts.slice(0, -1);
    this.openFolders.update((open) => new Set([...open, ...folders.map((_, i) => folders.slice(0, i + 1).join('/'))]));
  }

  /** Les onglets, l'historique et les dossiers ouverts suivent un fichier ou un dossier renommé ou déplacé. */
  followPath(from: string, to: string) {
    if (from === to) return;
    this.tabs.update((tabs) => remapPaths(tabs, from, to));
    this.history.update((history) => remapPaths(history, from, to));
    this.openFolders.update((open) => remapSet(open, from, to));
    this.activePath.update((active) => active && remapPath(active, from, to));
    this.requested = remapPath(this.requested, from, to);
    this.refreshVars();
  }

  /** Ferme sans confirmation les onglets d'un élément supprimé et retire ses traces de l'historique. */
  forgetPath(path: string) {
    for (const tab of this.tabs()) if (tab.sendingId && isUnder(tab.path, path)) void api.cancel(tab.sendingId);
    const rest = closeUnder(this.tabs(), this.activePath(), path);
    this.tabs.set(rest.tabs);
    this.activePath.set(rest.active);
    this.history.update((history) => history.filter((h) => !isUnder(h.path, path)));
    this.openFolders.update((open) => new Set([...open].filter((p) => !isUnder(p, path))));
    this.refreshVars();
  }

  /**
   * Après une relecture : ferme sans confirmation les onglets propres dont le fichier a disparu, garde ceux qui ont un brouillon en les marquant
   * `missing`. Renvoie le nombre d'onglets gardés.
   */
  reconcileTabs(): number {
    const c = this.collection();
    if (!c) return 0;
    const gone = new Set(missingPaths(c.items, this.tabs().map((t) => t.path)));
    for (const tab of this.tabs()) if (gone.has(tab.path) && !this.isDirty(tab)) this.forgetPath(tab.path);
    this.tabs.update((tabs) => tabs.map((t) => (!!t.missing === gone.has(t.path) ? t : { ...t, missing: gone.has(t.path) })));
    return this.tabs().filter((t) => t.missing).length;
  }

  /** Reprend le nom et la position (`seq`) écrits sur le disque dans les onglets donnés, sans toucher au reste du brouillon. */
  async refreshHeaders(paths: string[]) {
    const c = this.collection();
    if (!c) return;
    await Promise.all(
      paths.map(async (path) => {
        const fresh = await api.readRequest(c.root, path).catch(() => null);
        if (!fresh || this.collection()?.root !== c.root) return;
        const header = { name: fresh.name, seq: fresh.seq };
        this.tabs.update((tabs) =>
          tabs.map((t) =>
            t.path === path
              ? {
                  ...t,
                  doc: { ...t.doc, ...header },
                  saved: JSON.stringify({ ...JSON.parse(t.saved), ...header }),
                  base: JSON.stringify({ ...JSON.parse(t.base), ...header }),
                }
              : t,
          ),
        );
      }),
    );
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
        const text = JSON.stringify(doc);
        const tab: Tab = { path, doc, saved: text, base: text, preview: !pin };
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
    if (!c || !tab || this.blocked()) return;
    if (tab.missing) {
      this.notify("Le fichier n'existe plus sur le disque : copie le brouillon avant de fermer l'onglet.", true);
      return;
    }
    if (!this.isDirty(tab)) {
      this.notify(`Rien à enregistrer dans ${tab.path}`);
      return;
    }
    const path = tab.path;
    const onDisk = await api.readRequest(c.root, path).then(
      (doc) => JSON.stringify(doc),
      () => null,
    );
    const asked = this.tabs().find((t) => t.path === path);
    if (!asked) return;
    const outside = asked.stale || (onDisk !== null && onDisk !== asked.base && onDisk !== JSON.stringify(asked.doc));
    const overwrite = `« ${asked.doc.name || path} » a changé sur le disque depuis que tu l'as ouvert. L'enregistrer remplace ces changements par ton brouillon.`;
    if (outside && !(await this.confirmDiscard(overwrite, 'Écraser le fichier', 'Fichier modifié sur le disque'))) return;
    await this.writing(async () => {
      const current = this.tabs().find((t) => t.path === path);
      if (!current) return;
      try {
        const before: RequestDoc = JSON.parse(current.saved);
        const listed = before.name !== current.doc.name || before.method !== current.doc.method || before.url !== current.doc.url;
        await api.saveRequest(c.root, path, current.doc);
        const written = await api.readRequest(c.root, path).catch(() => null);
        const saved = JSON.stringify(current.doc);
        this.patchTab(path, { saved, base: written ? JSON.stringify(written) : saved, stale: false });
        this.notify(`Enregistré dans ${path}`);
        if (listed) await this.reload();
      } catch (e) {
        this.notify(`Échec de l'enregistrement : ${e}`, true);
      }
    });
  }

  async send() {
    const c = this.collection();
    const tab = this.active();
    if (!c || !tab) return;
    if (tab.sendingId) return this.cancel();
    const id = crypto.randomUUID();
    const started = performance.now();
    const where = () => this.tabs().find((t) => t.sendingId === id)?.path ?? tab.path;
    this.patchTab(tab.path, { sendingId: id, error: undefined });
    try {
      const run = await api.send(id, c.root, tab.path, tab.doc, this.env());
      const path = where();
      const { response, scripts: report, skipped } = run;
      const failure = skipped ? "Requête ignorée par un script (bru.runner.skipRequest())." : run.error?.message;
      this.patchTab(path, {
        result: response ? (run as Sent) : undefined,
        report,
        skipped,
        error: response ? undefined : (failure ?? 'La requête a échoué'),
        sendingId: undefined,
        sentAt: Date.now(),
      });
      if (response && this.collection()?.root === c.root) {
        this.history.update((h) => [{ path, method: run.method, url: run.url, status: response.status, at: time() }, ...h].slice(0, 30));
      }
    } catch (e) {
      const elapsed = Math.round(performance.now() - started);
      const message = String(e);
      this.patchTab(where(), {
        sendingId: undefined,
        result: undefined,
        report: undefined,
        skipped: undefined,
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

function initialEnv(c: CollectionInfo): string | null {
  return c.defaultEnvironment && c.environments.includes(c.defaultEnvironment) ? c.defaultEnvironment : c.environments[0] ?? null;
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
