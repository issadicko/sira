import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';

import { TreeItem } from '../core/model';
import { isOpenable } from '../core/runner';
import { Workspace } from '../core/store';
import { SyncStore } from '../core/sync-store';
import { cloneName, matchesQuery } from '../core/tree-ops';
import { TreeStore } from '../core/tree-store';
import { Icon } from './icon';
import { methodClass, shortMethod } from './method';
import { TreeEdit } from './tree-edit';
import { TreeRow } from './tree-row';

function count(item: TreeItem): number {
  return item.kind === 'request' ? 1 : item.children.reduce((n, c) => n + count(c), 0);
}

@Component({
  selector: 'app-tree',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, TreeRow, TreeEdit],
  host: { style: 'display: contents' },
  template: `
    @for (item of visible(); track item.path; let index = $index) {
      @if (tree.renaming() === item.path) {
        <app-tree-edit [kind]="item.kind" [depth]="depth()" [initial]="item.name" [method]="badge(item)" />
      } @else if (item.kind === 'folder') {
        <div
          class="row folder"
          [appTreeRow]="item"
          [style.--d]="depth()"
          [attr.aria-expanded]="isOpen(item.path)"
          [attr.aria-level]="depth() + 1"
          [attr.aria-posinset]="index + 1"
          [attr.aria-setsize]="visible().length"
        >
          <span class="twist"><app-ic [name]="isOpen(item.path) ? 'chev-down' : 'chev-right'" [size]="13" /></span>
          <app-ic [name]="isOpen(item.path) ? 'folder-open' : 'folder'" [size]="15" />
          <span class="row-name">{{ item.name }}</span>
          <span class="row-meta">{{ count(item) }}</span>
        </div>
      } @else {
        <div
          class="row req"
          [appTreeRow]="item"
          [class.is-deprecated]="item.deprecated"
          [style.--d]="depth()"
          [attr.aria-level]="depth() + 1"
          [attr.aria-posinset]="index + 1"
          [attr.aria-setsize]="visible().length"
          [attr.aria-disabled]="!runnable(item.requestType) && !item.error"
          [title]="item.error ?? (item.deprecated ? 'Retirée de la spec' : item.path)"
        >
          @if (item.error) {
            <span class="m m-delete">ERR</span>
          } @else if (!runnable(item.requestType)) {
            <span class="m">{{ item.requestType.slice(0, 4).toUpperCase() }}</span>
          } @else {
            <span [class]="methodClass(badge(item))">{{ shortMethod(badge(item)) }}</span>
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
        </div>
      }
      @if (item.kind === 'folder' && isOpen(item.path)) {
        <div class="group" role="group" [style.--d]="depth()">
          <app-tree [items]="item.children" [depth]="depth() + 1" [parent]="item.path" />
        </div>
      }
    }
    @if (adding(); as a) {
      <app-tree-edit [kind]="a.kind" [depth]="depth()" [initial]="a.initial" [method]="a.method" />
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

  protected readonly visible = computed(() => this.items().filter((i) => matchesQuery(i, this.tree.query())));
  protected readonly adding = computed(() => {
    const edit = this.tree.edit();
    if (edit?.mode === 'create') {
      return edit.parent === this.parent() ? { kind: edit.kind, initial: '', method: edit.requestType === 'graphql' ? 'GQL' : 'GET' } : null;
    }
    const copied = edit?.mode === 'clone' ? this.items().find((i) => i.path === edit.path) : undefined;
    return copied ? { kind: copied.kind, initial: cloneName(copied.name), method: copied.kind === 'request' ? this.badge(copied) : 'GET' } : null;
  });
  protected readonly count = count;
  protected readonly runnable = isOpenable;
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;

  protected badge(item: TreeItem) {
    if (item.kind !== 'request') return 'GET';
    return item.requestType === 'graphql' ? 'GQL' : item.requestType === 'websocket' ? 'WS' : item.requestType === 'grpc' ? 'gRPC' : item.method;
  }

  protected isOpen(path: string) {
    return this.tree.query() !== '' || this.ws.openFolders().has(path);
  }

  protected dirty(path: string) {
    const tab = this.ws.tabs().find((t) => t.path === path);
    return tab ? this.ws.isDirty(tab) : false;
  }
}
