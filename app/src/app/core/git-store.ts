import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from './api';
import { commitBlocker, pullBlocker } from './git';
import { GitDiff, GitFile, GitState } from './model';
import { Workspace } from './store';

/** Git pour la collection ouverte : ce qui a changé, la comparaison du fichier choisi, valider, récupérer et pousser. */
@Injectable({ providedIn: 'root' })
export class GitStore {
  private readonly ws = inject(Workspace);
  readonly state = signal<GitState | null>(null);
  readonly error = signal<string | null>(null);
  readonly selected = signal<string | null>(null);
  readonly diff = signal<GitDiff | null>(null);
  readonly message = signal('');
  readonly busy = signal<'commit' | 'pull' | 'push' | 'init' | null>(null);
  /** Dernier résultat d'une action, pour la ligne d'état de la barre latérale. */
  readonly notice = signal<string | null>(null);

  readonly files = computed(() => this.state()?.files ?? []);
  readonly count = computed(() => this.files().length);
  readonly current = computed<GitFile | null>(() => this.files().find((f) => f.path === this.selected()) ?? this.files()[0] ?? null);
  readonly blocker = computed(() => {
    const state = this.state();
    return state ? commitBlocker(state, this.message()) : 'Aucun dépôt Git.';
  });
  readonly pullBlocker = computed(() => {
    const state = this.state();
    return state ? pullBlocker(state) : 'Aucun dépôt Git.';
  });

  private refreshing = false;
  private again = false;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    effect(() => {
      const root = this.ws.collection()?.root ?? null;
      untracked(() => {
        this.state.set(null);
        this.error.set(null);
        this.selected.set(null);
        this.diff.set(null);
        this.notice.set(null);
        if (root) void this.refresh();
      });
    });
    effect(() => {
      this.ws.diskTick();
      untracked(() => this.refreshSoon());
    });
    effect(() => {
      const path = this.current()?.path ?? null;
      this.state();
      untracked(() => void this.loadDiff(path));
    });
  }

  private root(): string | null {
    return this.ws.collection()?.root ?? null;
  }

  /** Relit l'état après un instant de calme : une rafale de changements de fichiers ne lance qu'une lecture. */
  refreshSoon() {
    if (!this.root()) return;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.refresh(), 400);
  }

  async refresh() {
    const root = this.root();
    if (!root) return;
    if (this.refreshing) {
      this.again = true;
      return;
    }
    this.refreshing = true;
    try {
      const state = await api.gitStatus(root);
      if (this.root() !== root) return;
      this.state.set(state);
      this.error.set(null);
    } catch (e) {
      if (this.root() === root) this.error.set(String(e));
    } finally {
      this.refreshing = false;
      if (this.again) {
        this.again = false;
        void this.refresh();
      }
    }
  }

  private async loadDiff(path: string | null) {
    const root = this.root();
    if (!root || !path) {
      this.diff.set(null);
      return;
    }
    try {
      const diff = await api.gitDiff(root, path);
      if (this.current()?.path === path) this.diff.set(diff);
    } catch (e) {
      if (this.current()?.path === path) this.diff.set({ path, head: null, work: null, unreadable: String(e) });
    }
  }

  select(path: string) {
    this.selected.set(path);
  }

  private async run<T>(kind: 'commit' | 'pull' | 'push' | 'init', action: (root: string) => Promise<T>): Promise<T | null> {
    const root = this.root();
    if (!root || this.busy()) return null;
    this.busy.set(kind);
    this.notice.set(null);
    try {
      return await action(root);
    } catch (e) {
      this.error.set(String(e));
      return null;
    } finally {
      this.busy.set(null);
      await this.refresh();
    }
  }

  async init() {
    const state = await this.run('init', (root) => api.gitInit(root));
    if (state) this.notice.set('Dépôt Git créé dans le dossier de la collection.');
  }

  /** Valide tout ce qui a changé avec le message écrit, puis pousse si `push`. Le message n'est effacé qu'une fois validé. */
  async commit(push: boolean) {
    const state = this.state();
    if (!state || commitBlocker(state, this.message())) return;
    const done = await this.run('commit', (root) => api.gitCommit(root, this.message(), [], push));
    if (!done) return;
    this.message.set('');
    this.error.set(done.pushError);
    this.notice.set(done.pushed ? `Validé (${done.id}) et poussé.` : done.pushError ? `Validé (${done.id}), mais pas poussé.` : `Validé (${done.id}).`);
  }

  async pull() {
    if (this.pullBlocker()) return;
    const out = await this.run('pull', (root) => api.gitPull(root));
    if (out !== null) {
      this.error.set(null);
      this.notice.set(out.includes('Already up to date') ? 'Déjà à jour.' : 'Modifications récupérées.');
      await this.ws.rescan();
    }
  }

  async push() {
    const out = await this.run('push', (root) => api.gitPush(root));
    if (out !== null) {
      this.error.set(null);
      this.notice.set('Poussé.');
    }
  }
}
