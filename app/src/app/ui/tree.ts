import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';

import { TreeItem } from '../core/model';
import { Workspace } from '../core/store';
import { SyncStore } from '../core/sync-store';
import { cloneName } from '../core/tree-ops';
import { TreeStore } from '../core/tree-store';
import { Icon } from './icon';
import { methodClass, shortMethod } from './method';
import { TreeEdit } from './tree-edit';
import { TreeRow } from './tree-row';

function count(item: TreeItem): number {
  return item.kind === 'request' ? 1 : item.children.reduce((n, c) => n + count(c), 0);
}

function matches(item: TreeItem, q: string): boolean {
  if (!q) return true;
  if (item.kind === 'folder') return item.children.some((c) => matches(c, q));
  return `${item.name} ${item.path}`.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').includes(q);
}

@Component({
  selector: 'app-tree',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, TreeRow, TreeEdit],
  host: { style: 'display: contents' },
  template: `
    @for (item of visible(); track item.path) {
      @if (tree.renaming() === item.path) {
        <app-tree-edit [kind]="item.kind" [depth]="depth()" [initial]="item.name" [method]="item.kind === 'request' ? item.method : 'GET'" />
      } @else if (item.kind === 'folder') {
        <button class="row folder" [appTreeRow]="item" [style.--d]="depth()" [attr.aria-expanded]="isOpen(item.path)" [attr.aria-level]="depth() + 1" (click)="toggle(item.path)">
          <span class="twist"><app-ic [name]="isOpen(item.path) ? 'chev-down' : 'chev-right'" [size]="13" /></span>
          <app-ic [name]="isOpen(item.path) ? 'folder-open' : 'folder'" [size]="15" />
          <span class="row-name">{{ item.name }}</span>
          <span class="row-meta">{{ count(item) }}</span>
        </button>
      } @else {
        <button
          class="row req"
          [appTreeRow]="item"
          [class.is-deprecated]="item.deprecated"
          [style.--d]="depth()"
          [attr.aria-level]="depth() + 1"
          [attr.aria-disabled]="item.requestType !== 'http' && !item.error"
          [title]="item.error ?? (item.deprecated ? 'Retirée de la spec' : item.path)"
          (click)="open(item, false)"
          (dblclick)="open(item, true)"
        >
          @if (item.error) {
            <span class="m m-delete">ERR</span>
          } @else if (item.requestType !== 'http') {
            <span class="m">{{ item.requestType.slice(0, 4).toUpperCase() }}</span>
          } @else {
            <span [class]="methodClass(item.method)">{{ shortMethod(item.method) }}</span>
          }
          <span class="row-name">{{ item.name }}</span>
          @if (item.deprecated) {
            <span class="tag">dépréciée</span>
          }
          @if (sync.conflictPaths().has(item.path)) {
            <span class="tree-conflict" title="Conflit avec la spec OpenAPI"><app-ic name="alert" [size]="13" /></span>
          }
          @if (dirty(item.path)) {
            <span class="dot" style="background: var(--ink)" title="Modifications non enregistrées"></span>
          }
        </button>
      }
      @if (item.kind === 'folder' && isOpen(item.path)) {
        <div class="group" role="group" [style.--d]="depth()">
          <app-tree [items]="item.children" [depth]="depth() + 1" [parent]="item.path" />
        </div>
      }
    }
    @if (adding(); as a) {
      <app-tree-edit [kind]="a.kind" [depth]="depth()" [initial]="a.initial" />
    }
  `,
})
export class Tree {
  protected readonly ws = inject(Workspace);
  protected readonly tree = inject(TreeStore);
  protected readonly sync = inject(SyncStore);
  readonly items = input.required<TreeItem[]>();
  readonly depth = input(0);
  /** Chemin du dossier que cette liste montre, vide pour la racine. */
  readonly parent = input('');

  protected readonly query = computed(() => this.ws.filter().trim().toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, ''));
  protected readonly visible = computed(() => this.items().filter((i) => matches(i, this.query())));
  protected readonly adding = computed(() => {
    const edit = this.tree.edit();
    if (edit?.mode === 'create') return edit.parent === this.parent() ? { kind: edit.kind, initial: '' } : null;
    const copied = edit?.mode === 'clone' ? this.items().find((i) => i.path === edit.path) : undefined;
    return copied ? { kind: copied.kind, initial: cloneName(copied.name) } : null;
  });
  protected readonly count = count;
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;

  protected isOpen(path: string) {
    return this.query() !== '' || this.ws.openFolders().has(path);
  }

  protected toggle(path: string) {
    this.tree.select(path);
    this.ws.toggleFolder(path);
  }

  protected open(item: TreeItem, pin: boolean) {
    this.tree.select(item.path);
    if (item.kind === 'request' && (item.requestType === 'http' || item.error)) void this.ws.openRequest(item.path, pin);
  }

  protected dirty(path: string) {
    const tab = this.ws.tabs().find((t) => t.path === path);
    return tab ? this.ws.isDirty(tab) : false;
  }
}
