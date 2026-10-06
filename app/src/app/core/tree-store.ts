import { Injectable, Injector, afterNextRender, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from './api';
import { DropPosition, TreeItem } from './model';
import { isRunnable } from './runner';
import { dirname } from './paths';
import { Workspace } from './store';
import { SyncStore } from './sync-store';
import {
  Box,
  ItemKind,
  TreeKey,
  canDrop,
  contentsOf,
  dropPositionAt,
  findItem,
  isNoopMove,
  isUnder,
  navigate,
  neighbourOf,
  normalizeQuery,
  reorderMove,
  reorderNotice,
  tabbablePath,
  validateName,
  visibleRows,
} from './tree-ops';

export type Edit =
  | { mode: 'create'; kind: ItemKind; parent: string; requestType?: 'graphql' }
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

/** Élément dont la boîte « Déplacer vers… » choisit le nouveau dossier. */
export interface Moving {
  path: string;
  kind: ItemKind;
  name: string;
}

const OPEN_DELAY_MS = 600;
const ROOT_DROP: DropTarget = { target: '', position: 'inside' };

interface Resolved {
  kind: ItemKind;
  parent: string;
  subject: TreeItem | null;
}

/** Édition de l'arbre : sélection, navigation au clavier, saisie sur place, menus, suppression, déplacement et glisser-déposer. */
@Injectable({ providedIn: 'root' })
export class TreeStore {
  private readonly ws = inject(Workspace);
  private readonly sync = inject(SyncStore);
  private readonly injector = inject(Injector);

  readonly selected = signal<string | null>(null);
  readonly edit = signal<Edit | null>(null);
  readonly editError = signal<string | null>(null);
  readonly busy = this.ws.busy;
  readonly menu = signal<MenuState | null>(null);
  readonly dragging = signal<string | null>(null);
  readonly drop = signal<DropTarget | null>(null);
  readonly deletion = signal<Deletion | null>(null);
  readonly deleteError = signal<string | null>(null);
  readonly moving = signal<Moving | null>(null);

  readonly query = computed(() => normalizeQuery(this.ws.filter()));
  readonly rows = computed(() => visibleRows(this.ws.collection()?.items ?? [], this.ws.openFolders(), this.query()));
  /** Seule ligne atteignable par Tab : la sélection si elle est affichée, sinon la première ligne. */
  readonly tabbable = computed(() => tabbablePath(this.rows(), this.selected()));
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
      untracked(() => {
        if (active && !this.busy()) this.selected.set(active);
      });
    });
  }

  select(path: string | null) {
    this.selected.set(path);
  }

  /** De retour dans l'éditeur, la sélection reprend la requête de l'onglet actif : les commandes visent ce que l'on voit. */
  followEditor() {
    const active = this.ws.activePath();
    if (active && !this.busy() && this.selected() !== active) this.selected.set(active);
  }

  /** Focalise une ligne, qui devient la sélection : le focus et la sélection ne se séparent pas. */
  focusRow(path: string | null) {
    if (!path) return;
    this.select(path);
    document.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)?.focus();
  }

  /** Rend le focus à la ligne sélectionnée une fois l'arbre redessiné, même si le retour du focus d'un dialogue a déplacé la sélection entre-temps. */
  focusSelected() {
    const path = this.tabbable();
    afterNextRender(() => this.focusRow(path), { injector: this.injector });
  }

  /** Clic ou Entrée sur une ligne : sélectionne, bascule un dossier, ouvre une requête (`pin` : onglet permanent). */
  activate(item: TreeItem, pin: boolean) {
    this.select(item.path);
    if (item.kind === 'folder') this.ws.toggleFolder(item.path);
    else if (isRunnable(item.requestType) || item.error) void this.ws.openRequest(item.path, pin);
  }

  /** Touche de déplacement depuis la ligne `path` : le focus et la sélection suivent, un dossier s'ouvre ou se ferme, Entrée active. */
  navigateFrom(path: string, key: TreeKey) {
    const step = navigate(this.rows(), path, key, this.query() !== '');
    if (!step) return;
    if ('focus' in step) {
      this.focusRow(step.focus);
    } else if ('toggle' in step) {
      this.ws.toggleFolder(step.toggle);
    } else {
      const item = findItem(this.ws.collection()?.items ?? [], step.activate);
      if (item) this.activate(item, true);
    }
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

  beginCreate(kind: ItemKind, parent = this.createParent(), requestType?: 'graphql') {
    if (!this.ws.collection() || this.busy()) return;
    this.closeMenu();
    this.showTree();
    if (parent) this.ws.reveal(parent, true);
    this.editError.set(null);
    this.edit.set({ mode: 'create', kind, parent, requestType });
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

  /** Abandonne la saisie ; `restoreFocus` est faux quand le focus est déjà parti ailleurs (blur). */
  cancelEdit(restoreFocus = true) {
    if (this.busy()) return;
    this.edit.set(null);
    this.editError.set(null);
    if (restoreFocus) this.focusSelected();
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
      await this.settle(c.root, edit, kind, path, name);
    } catch (e) {
      this.editError.set(String(e));
      const note = await this.recover();
      if (note) this.ws.notify(note, true);
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
        if (edit.kind === 'folder') return api.createFolder(root, edit.parent, name);
        return edit.requestType === 'graphql' ? api.createGraphqlRequest(root, edit.parent, name) : api.createRequest(root, edit.parent, name);
      case 'clone':
        return api.cloneItem(root, edit.path, name);
      case 'rename':
        return api.renameItem(root, edit.path, name);
    }
  }

  /** Vrai si une autre collection a remplacé celle de `root` pendant une commande : plus rien à lui appliquer. */
  private stale(root: string): boolean {
    return this.ws.collection()?.root !== root;
  }

  /** La ligne de saisie reste, en lecture seule, jusqu'à la relecture de l'arbre : pas de trou entre la validation et la nouvelle ligne. */
  private async settle(root: string, edit: Edit, kind: ItemKind, path: string, name: string) {
    const renamed = edit.mode === 'rename';
    const draft = edit.mode === 'clone' && this.ws.tabs().some((t) => isUnder(t.path, edit.path) && this.ws.isDirty(t));
    if (renamed) this.ws.followPath(edit.path, path);
    await this.ws.reload();
    if (this.stale(root)) return;
    this.edit.set(null);
    if (renamed && kind === 'request') await this.ws.refreshHeaders([path]);
    if (this.stale(root)) return;
    this.ws.reveal(path);
    this.select(path);
    this.focusSelected();
    if (!renamed && kind === 'request') await this.ws.openRequest(path, true);
    if (this.stale(root)) return;
    const verb = renamed ? 'Renommé' : edit.mode === 'clone' ? 'Copie créée' : kind === 'request' ? 'Requête créée' : 'Dossier créé';
    const note = draft ? ' (copie du fichier enregistré, brouillon non inclus)' : '';
    this.finish(`${verb} : ${renamed ? name : path}${note}`, renamed);
  }

  /**
   * Relit le disque après un échec : une commande a pu modifier des fichiers avant d'échouer. Ferme les onglets propres dont le fichier a disparu,
   * garde ceux qui ont un brouillon (marqués) et renvoie de quoi le dire.
   */
  private async recover(): Promise<string> {
    await this.ws.reload();
    const kept = this.ws.reconcileTabs();
    await this.ws.refreshHeaders(this.ws.tabs().filter((t) => !t.missing).map((t) => t.path));
    if (this.sync.plan()) void this.sync.invalidate();
    if (!kept) return '';
    return ` ${kept > 1 ? `${kept} onglets modifiés pointent` : 'Un onglet modifié pointe'} vers un fichier qui n'existe plus : le brouillon reste ouvert.`;
  }

  /** Notifie la fin d'une commande ; `rewrote` : elle a réécrit, déplacé ou supprimé des fichiers existants, ce qui rend caduque la comparaison OpenAPI en cours. */
  private finish(message: string, rewrote: boolean) {
    const stale = rewrote && !!this.sync.plan();
    if (stale) void this.sync.invalidate();
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
      if (this.stale(c.root)) return;
      this.ws.forgetPath(deletion.path);
      this.deletion.set(null);
      this.ws.dialog.set(null);
      await this.ws.reload();
      if (this.stale(c.root)) return;
      this.select(neighbourOf(c.items, deletion.path));
      this.focusSelected();
      this.finish(`« ${deletion.name} » est dans la corbeille.`, true);
    } catch (e) {
      const note = await this.recover();
      if (findItem(this.ws.collection()?.items ?? [], deletion.path)) {
        this.deleteError.set(String(e));
      } else {
        this.deletion.set(null);
        this.ws.dialog.set(null);
        this.select(neighbourOf(c.items, deletion.path));
        this.ws.notify(`${e}${note}`, true);
      }
    } finally {
      this.busy.set(false);
    }
  }

  requestMove(path = this.target()) {
    const c = this.ws.collection();
    const item = c && path ? findItem(c.items, path) : null;
    if (!item || this.busy()) return;
    this.closeMenu();
    this.moving.set({ path: item.path, kind: item.kind, name: item.name });
    this.ws.dialog.set('move');
  }

  cancelMove() {
    this.moving.set(null);
    this.ws.dialog.set(null);
  }

  confirmMove(folder: string) {
    const moving = this.moving();
    this.cancelMove();
    return moving ? this.move(moving.path, folder, 'inside') : Promise.resolve();
  }

  async move(path: string, target: string, position: DropPosition) {
    const c = this.ws.collection();
    if (!c || this.busy() || !canDrop(path, target) || isNoopMove(c.items, path, target, position)) return;
    this.busy.set(true);
    try {
      const next = await api.moveItem(c.root, path, target, position);
      if (this.stale(c.root)) return;
      this.ws.followPath(path, next);
      await this.ws.reload();
      if (this.stale(c.root)) return;
      await this.ws.refreshHeaders(this.ws.tabs().map((t) => t.path));
      if (this.stale(c.root)) return;
      this.ws.reveal(next);
      this.select(next);
      this.focusSelected();
      this.finish(dirname(next) === dirname(path) ? 'Ordre enregistré' : `Déplacé : ${next}`, true);
    } catch (e) {
      const note = await this.recover();
      this.ws.notify(`${e}${note}`, true);
    } finally {
      this.busy.set(false);
    }
  }

  async reorder(path: string, direction: 1 | -1) {
    const c = this.ws.collection();
    if (!c) return;
    if (this.query()) return this.ws.notify("Réordonner est indisponible pendant un filtre : efface-le d'abord.");
    const step = reorderMove(c.items, path, direction);
    if (step) await this.move(path, step.target, step.position);
    else if (findItem(c.items, path)) this.ws.notify(reorderNotice(direction));
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

  /** Cible de dépôt sur la ligne `target` au point d'ordonnée `y` ; `null` si le dépôt est refusé ou sans effet. */
  private locate(target: string, kind: ItemKind, y: number, box: Box): DropTarget | null {
    const dragged = this.dragging();
    const items = this.ws.collection()?.items ?? [];
    if (!dragged || !canDrop(dragged, target)) return null;
    const position = dropPositionAt(y, box, kind, this.ws.openFolders().has(target), this.query() === '');
    return position && !isNoopMove(items, dragged, target, position) ? { target, position } : null;
  }

  private locateRoot(): DropTarget | null {
    const dragged = this.dragging();
    return dragged && !isNoopMove(this.ws.collection()?.items ?? [], dragged, '', 'inside') ? ROOT_DROP : null;
  }

  private show(next: DropTarget | null) {
    this.drop.update((d) => (d?.target === next?.target && d?.position === next?.position ? d : next));
  }

  /** Survol d'une ligne pendant un glisser : renvoie `false` quand le dépôt est refusé, sans indicateur dans ce cas. */
  dragOver(target: string, kind: ItemKind, y: number, box: Box): boolean {
    const next = this.locate(target, kind, y, box);
    this.show(next);
    const folded = kind === 'folder' && !this.ws.openFolders().has(target) && this.query() === '';
    this.hoverFolder(next?.position === 'inside' && folded ? target : null);
    return !!next;
  }

  /** Survol de la place libre sous les lignes : dépôt à la racine. */
  dragOverRoot(): boolean {
    const next = this.locateRoot();
    this.show(next);
    this.hoverFolder(null);
    return !!next;
  }

  dragLeave() {
    this.drop.set(null);
    this.hoverFolder(null);
  }

  endDrag() {
    this.dragging.set(null);
    this.dragLeave();
  }

  /** Dépôt sur la ligne `target` : la position se recalcule d'après les coordonnées du `drop`, pas d'après le dernier survol. */
  async dropOn(target: string, kind: ItemKind, y: number, box: Box) {
    const dragged = this.dragging();
    const next = this.locate(target, kind, y, box);
    this.endDrag();
    if (dragged && next) await this.move(dragged, next.target, next.position);
  }

  async dropRoot() {
    const dragged = this.dragging();
    const next = this.locateRoot();
    this.endDrag();
    if (dragged && next) await this.move(dragged, next.target, next.position);
  }

  private hoverFolder(path: string | null) {
    if (path === this.hovered) return;
    clearTimeout(this.hoverTimer);
    this.hovered = path;
    if (path) this.hoverTimer = setTimeout(() => this.ws.reveal(path, true), OPEN_DELAY_MS);
  }
}
