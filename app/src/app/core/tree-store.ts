import { Injectable, Injector, afterNextRender, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from './api';
import { DropPosition, TreeItem } from './model';
import { dirname } from './paths';
import { Workspace } from './store';
import { SyncStore } from './sync-store';
import { ItemKind, canDrop, contentsOf, dropPosition, findItem, isUnder, neighbourOf, reorderMove, validateName } from './tree-ops';

export type Edit =
  | { mode: 'create'; kind: ItemKind; parent: string }
  | { mode: 'clone'; path: string }
  | { mode: 'rename'; path: string };

/** Menu ouvert en `x`, `y` : sur la ligne `path`, ou sur le bouton + de l'en-tête quand `path` est `null`. */
export interface MenuState {
  x: number;
  y: number;
  path: string | null;
}

export interface DropTarget {
  target: string;
  position: DropPosition;
}

export interface Deletion {
  path: string;
  kind: ItemKind;
  name: string;
  requests: number;
  folders: number;
  unsaved: string[];
}

const OPEN_DELAY_MS = 600;

interface Resolved {
  kind: ItemKind;
  parent: string;
  subject: TreeItem | null;
}

/** Édition de l'arbre : sélection, saisie sur place, menus, suppression et glisser-déposer. */
@Injectable({ providedIn: 'root' })
export class TreeStore {
  private readonly ws = inject(Workspace);
  private readonly sync = inject(SyncStore);
  private readonly injector = inject(Injector);

  readonly selected = signal<string | null>(null);
  readonly edit = signal<Edit | null>(null);
  readonly editError = signal<string | null>(null);
  readonly busy = signal(false);
  readonly menu = signal<MenuState | null>(null);
  readonly dragging = signal<string | null>(null);
  readonly drop = signal<DropTarget | null>(null);
  readonly deletion = signal<Deletion | null>(null);
  readonly deleteError = signal<string | null>(null);

  /** Élément visé par les commandes : la ligne sélectionnée, à défaut la requête de l'onglet actif. */
  readonly target = computed(() => {
    const c = this.ws.collection();
    const known = (path: string | null) => (c && path && findItem(c.items, path) ? path : null);
    return known(this.selected()) ?? known(this.ws.activePath());
  });
  readonly renaming = computed(() => {
    const edit = this.edit();
    return edit?.mode === 'rename' ? edit.path : null;
  });

  private hoverTimer?: ReturnType<typeof setTimeout>;
  private hovered: string | null = null;

  constructor() {
    const root = computed(() => this.ws.collection()?.root ?? null);
    effect(() => {
      root();
      untracked(() => {
        this.selected.set(null);
        this.edit.set(null);
        this.menu.set(null);
      });
    });
    effect(() => {
      const active = this.ws.activePath();
      if (active) untracked(() => this.selected.set(active));
    });
  }

  select(path: string | null) {
    this.selected.set(path);
  }

  focusSelected() {
    afterNextRender(
      () => {
        const path = this.selected();
        if (path) document.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)?.focus();
      },
      { injector: this.injector },
    );
  }

  private showTree() {
    this.ws.view.set('collections');
    this.ws.sidebar.set(true);
    this.ws.filter.set('');
  }

  /** Dossier où créer : celui de la sélection, la racine sans sélection. */
  createParent(): string {
    const c = this.ws.collection();
    const path = this.target();
    const item = c && path ? findItem(c.items, path) : null;
    return !item ? '' : item.kind === 'folder' ? item.path : dirname(item.path);
  }

  beginCreate(kind: ItemKind, parent = this.createParent()) {
    if (!this.ws.collection() || this.busy()) return;
    this.closeMenu();
    this.showTree();
    if (parent) this.ws.reveal(parent, true);
    this.editError.set(null);
    this.edit.set({ mode: 'create', kind, parent });
  }

  beginRename(path = this.target()) {
    this.beginOn(path, 'rename');
  }

  beginClone(path = this.target()) {
    this.beginOn(path, 'clone');
  }

  private beginOn(path: string | null, mode: 'rename' | 'clone') {
    const c = this.ws.collection();
    if (!c || !path || !findItem(c.items, path) || this.busy()) return;
    this.closeMenu();
    this.showTree();
    this.ws.reveal(path);
    this.select(path);
    this.editError.set(null);
    this.edit.set({ mode, path });
  }

  cancelEdit() {
    if (this.busy()) return;
    this.edit.set(null);
    this.editError.set(null);
  }

  async submit(raw: string) {
    const edit = this.edit();
    const c = this.ws.collection();
    if (!edit || !c || this.busy()) return;
    const resolved = this.resolve(edit, c.items);
    if (!resolved) return this.cancelEdit();
    const { kind, parent, subject } = resolved;
    const name = raw.trim();
    const problem = validateName(name, kind, parent);
    if (problem) {
      this.editError.set(problem);
      return;
    }
    if (edit.mode === 'rename' && name === subject?.name) return this.cancelEdit();
    this.busy.set(true);
    this.editError.set(null);
    try {
      const path = await this.write(edit, c.root, name);
      this.edit.set(null);
      await this.settle(edit, kind, path, name);
    } catch (e) {
      this.editError.set(String(e));
    } finally {
      this.busy.set(false);
    }
  }

  private resolve(edit: Edit, items: TreeItem[]): Resolved | null {
    if (edit.mode === 'create') return { kind: edit.kind, parent: edit.parent, subject: null };
    const subject = findItem(items, edit.path);
    return subject ? { kind: subject.kind, parent: dirname(edit.path), subject } : null;
  }

  private write(edit: Edit, root: string, name: string): Promise<string> {
    switch (edit.mode) {
      case 'create':
        return edit.kind === 'request' ? api.createRequest(root, edit.parent, name) : api.createFolder(root, edit.parent, name);
      case 'clone':
        return api.cloneItem(root, edit.path, name);
      case 'rename':
        return api.renameItem(root, edit.path, name);
    }
  }

  private async settle(edit: Edit, kind: ItemKind, path: string, name: string) {
    if (edit.mode === 'rename') this.ws.followPath(edit.path, path);
    await this.ws.reload();
    if (edit.mode === 'rename' && kind === 'request') await this.ws.refreshHeaders([path]);
    this.ws.reveal(path);
    this.select(path);
    this.focusSelected();
    if (edit.mode !== 'rename' && kind === 'request') await this.ws.openRequest(path, true);
    const verb = edit.mode === 'rename' ? 'Renommé' : edit.mode === 'clone' ? 'Copie créée' : kind === 'request' ? 'Requête créée' : 'Dossier créé';
    this.finish(`${verb} : ${edit.mode === 'rename' ? name : path}`, edit.mode === 'rename' ? edit.path : null);
  }

  /** Notifie la fin d'une commande, et abandonne la comparaison OpenAPI en cours si un fichier qu'elle visait a changé. */
  private finish(message: string, changed: string | null) {
    const plan = this.sync.plan();
    const stale = !!changed && !!plan?.operations.some((o) => o.file && isUnder(o.file, changed));
    if (stale) this.sync.invalidate();
    else void this.sync.refreshStatus();
    this.ws.notify(stale ? `${message}. La comparaison OpenAPI en cours est abandonnée : relance la synchro.` : message);
  }

  requestDelete(path = this.target()) {
    const c = this.ws.collection();
    const item = c && path ? findItem(c.items, path) : null;
    if (!item || this.busy()) return;
    this.closeMenu();
    const unsaved = this.ws.tabs().filter((t) => isUnder(t.path, item.path) && this.ws.isDirty(t)).map((t) => t.doc.name || t.path);
    this.deleteError.set(null);
    this.deletion.set({ path: item.path, kind: item.kind, name: item.name, ...contentsOf(item), unsaved });
    this.ws.dialog.set('delete');
  }

  cancelDelete() {
    if (this.busy()) return;
    this.deletion.set(null);
    this.ws.dialog.set(null);
  }

  async confirmDelete() {
    const deletion = this.deletion();
    const c = this.ws.collection();
    if (!deletion || !c || this.busy()) return;
    this.busy.set(true);
    this.deleteError.set(null);
    try {
      await api.deleteItem(c.root, deletion.path);
      const next = neighbourOf(c.items, deletion.path);
      this.ws.forgetPath(deletion.path);
      this.deletion.set(null);
      this.ws.dialog.set(null);
      this.select(next);
      await this.ws.reload();
      this.focusSelected();
      this.finish(`« ${deletion.name} » est dans la corbeille.`, deletion.path);
    } catch (e) {
      this.deleteError.set(String(e));
    } finally {
      this.busy.set(false);
    }
  }

  async move(path: string, target: string, position: DropPosition) {
    const c = this.ws.collection();
    if (!c || this.busy() || !canDrop(path, target)) return;
    this.busy.set(true);
    try {
      const next = await api.moveItem(c.root, path, target, position);
      this.ws.followPath(path, next);
      await this.ws.reload();
      await this.ws.refreshHeaders(this.ws.tabs().map((t) => t.path));
      this.ws.reveal(next);
      this.select(next);
      this.focusSelected();
      this.finish(dirname(next) === dirname(path) ? 'Ordre enregistré' : `Déplacé : ${next}`, path);
    } catch (e) {
      this.ws.notify(String(e), true);
    } finally {
      this.busy.set(false);
    }
  }

  async reorder(path: string, direction: 1 | -1) {
    const c = this.ws.collection();
    const step = c && reorderMove(c.items, path, direction);
    if (step) await this.move(path, step.target, step.position);
  }

  openMenu(x: number, y: number, path: string | null) {
    this.menu.set({ x, y, path });
  }

  closeMenu() {
    this.menu.set(null);
  }

  startDrag(path: string) {
    this.closeMenu();
    this.dragging.set(path);
  }

  /** Survol d'une ligne pendant un glisser : renvoie `false` quand le dépôt est refusé, sans indicateur dans ce cas. */
  dragOver(target: string, kind: ItemKind, ratio: number): boolean {
    const dragged = this.dragging();
    if (!dragged || !canDrop(dragged, target)) {
      this.drop.set(null);
      this.hoverFolder(null);
      return false;
    }
    const open = kind === 'folder' && this.ws.openFolders().has(target);
    const position = dropPosition(ratio, kind, open);
    this.drop.update((d) => (d?.target === target && d.position === position ? d : { target, position }));
    this.hoverFolder(kind === 'folder' && !open && position === 'inside' ? target : null);
    return true;
  }

  /** Survol de la place libre sous les lignes : dépôt à la racine. */
  dragOverRoot(): boolean {
    if (!this.dragging()) return false;
    this.drop.set({ target: '', position: 'inside' });
    this.hoverFolder(null);
    return true;
  }

  dragLeave() {
    this.drop.set(null);
    this.hoverFolder(null);
  }

  endDrag() {
    this.dragging.set(null);
    this.dragLeave();
  }

  async dropHere() {
    const dragged = this.dragging();
    const drop = this.drop();
    this.endDrag();
    if (dragged && drop) await this.move(dragged, drop.target, drop.position);
  }

  private hoverFolder(path: string | null) {
    if (path === this.hovered) return;
    clearTimeout(this.hoverTimer);
    this.hovered = path;
    if (path) this.hoverTimer = setTimeout(() => this.ws.reveal(path, true), OPEN_DELAY_MS);
  }
}
