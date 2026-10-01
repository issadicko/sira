import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';

import { TreeItem } from '../core/model';
import { Workspace } from '../core/store';
import { SyncStore } from '../core/sync-store';
import { Icon } from './icon';
import { methodClass, shortMethod } from './method';

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
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
    @for (item of visible(); track item.path) {
      @if (item.kind === 'folder') {
        <button class="row folder" [style.--d]="depth()" [attr.aria-expanded]="isOpen(item.path)" (click)="ws.toggleFolder(item.path)">
          <span class="twist"><app-ic [name]="isOpen(item.path) ? 'chev-down' : 'chev-right'" [size]="13" /></span>
          <app-ic [name]="isOpen(item.path) ? 'folder-open' : 'folder'" [size]="15" />
          <span class="row-name">{{ item.name }}</span>
          <span class="row-meta">{{ count(item) }}</span>
        </button>
        @if (isOpen(item.path)) {
          <div class="group" [style.--d]="depth()">
            <app-tree [items]="item.children" [depth]="depth() + 1" />
          </div>
        }
      } @else {
        <button
          class="row req"
          [class.is-active]="ws.activePath() === item.path"
          [class.is-deprecated]="item.deprecated"
          [style.--d]="depth()"
          [title]="item.error ?? (item.deprecated ? 'Retirée de la spec' : item.path)"
          [disabled]="item.requestType !== 'http' && !item.error"
          (click)="ws.openRequest(item.path)"
          (dblclick)="ws.openRequest(item.path, true)"
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
    }
  `,
})
export class Tree {
  protected readonly ws = inject(Workspace);
  protected readonly sync = inject(SyncStore);
  readonly items = input.required<TreeItem[]>();
  readonly depth = input(0);

  protected readonly query = computed(() => this.ws.filter().trim().toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, ''));
  protected readonly visible = computed(() => this.items().filter((i) => matches(i, this.query())));
  protected readonly count = count;
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;

  protected isOpen(path: string) {
    return this.query() !== '' || this.ws.openFolders().has(path);
  }

  protected dirty(path: string) {
    const tab = this.ws.tabs().find((t) => t.path === path);
    return tab ? this.ws.isDirty(tab) : false;
  }
}
