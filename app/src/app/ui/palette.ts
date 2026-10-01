import { NgTemplateOutlet } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  Injector,
  afterNextRender,
  computed,
  effect,
  inject,
  linkedSignal,
  viewChild,
} from '@angular/core';

import { COMMANDS, shortcutLabel } from '../core/commands';
import { Segment, rank, segments } from '../core/fuzzy';
import { TreeItem } from '../core/model';
import { Workspace } from '../core/store';
import { restoreFocus } from './focus';
import { Icon } from './icon';
import { methodClass, shortMethod } from './method';

type RequestItem = Extract<TreeItem, { kind: 'request' }>;

interface Entry {
  key: string;
  index: number;
  label: Segment[];
  sub: Segment[];
  path?: Segment[];
  method?: string;
  icon?: string;
  env?: string | null;
  shortcut?: string;
  active?: boolean;
  run: () => unknown;
}

interface Group {
  title: string;
  entries: Entry[];
}

function requests(items: TreeItem[]): RequestItem[] {
  return items.flatMap((i) =>
    i.kind === 'folder' ? requests(i.children) : i.requestType === 'http' && !i.error ? [i] : [],
  );
}

const shortUrl = (url: string) => url.replace(/^\{\{[^{}]*\}\}/, '');
const envLabel = (env: string | null) => (env ? `Environnement : ${env}` : 'Aucun environnement');

/** Palette de commandes (⌘K) : requêtes de la collection, commandes du registre, environnements. */
@Component({
  selector: 'app-palette',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, NgTemplateOutlet],
  template: `
    <ng-template #hl let-segs>@for (s of segs; track $index) {@if (s.hit) {<mark>{{ s.text }}</mark>} @else {<ng-container>{{ s.text }}</ng-container>}}</ng-template>
    @if (open()) {
      <div class="palette-wrap" (mousedown)="pressVeil($event)" (click)="dismiss($event)">
        <div class="palette" role="dialog" aria-modal="true" aria-label="Palette de commandes" (mousedown)="keepFocus($event)">
          <label class="pal-input">
            <app-ic name="search" [size]="16" />
            <input
              #input
              type="text"
              role="combobox"
              autocomplete="off"
              spellcheck="false"
              aria-label="Rechercher une requête ou une commande"
              aria-autocomplete="list"
              aria-expanded="true"
              aria-controls="pal-listbox"
              [attr.aria-activedescendant]="count() ? 'pal-o-' + current() : null"
              [placeholder]="ws.collection() ? 'Rechercher une requête, ou tape > pour les commandes' : 'Rechercher une commande'"
              [value]="ws.palette()"
              (input)="ws.palette.set($any($event.target).value)"
              (keydown)="onKey($event)"
            />
          </label>
          <div #list class="pal-list">
            <div id="pal-listbox" role="listbox" aria-label="Résultats">
              @for (g of groups(); track g.title; let gi = $index) {
                <div role="group" [attr.aria-labelledby]="'pal-g-' + gi">
                  <div class="pal-group" [id]="'pal-g-' + gi">{{ g.title }}</div>
                  @for (e of g.entries; track e.key) {
                    <div class="pal-item" role="option" [id]="'pal-o-' + e.index" [attr.aria-selected]="e.index === current()" (mousemove)="sel.set(e.index)" (click)="run(e)">
                      @if (e.method) {
                        <span [class]="methodClass(e.method)">{{ shortMethod(e.method) }}</span>
                      } @else if (e.icon) {
                        <app-ic [name]="e.icon" [size]="15" />
                      } @else {
                        <span class="env-dot" [class.none]="!e.env" [class.prod]="e.env === 'prod'"></span>
                      }
                      <span class="lbl"><ng-container *ngTemplateOutlet="hl; context: { $implicit: e.label }" /></span>
                      <span class="sub mono"><ng-container *ngTemplateOutlet="hl; context: { $implicit: e.sub }" /></span>
                      @if (e.path) {
                        <span class="path mono"><ng-container *ngTemplateOutlet="hl; context: { $implicit: e.path }" /></span>
                      }
                      @if (e.shortcut) {
                        <kbd class="kbd">{{ e.shortcut }}</kbd>
                      }
                      @if (e.active) {
                        <span class="tag good">actif</span>
                      }
                    </div>
                  }
                </div>
              }
            </div>
            @if (!count()) {
              <div class="pal-empty">Aucun résultat pour « {{ term() }} ».</div>
            }
          </div>
          <div class="pal-foot">
            <span><kbd class="kbd">↑</kbd><kbd class="kbd">↓</kbd>naviguer</span>
            <span><kbd class="kbd">↵</kbd>ouvrir</span>
            <span><kbd class="kbd">&gt;</kbd>commandes</span>
            <span><kbd class="kbd">Échap</kbd>fermer</span>
          </div>
          <span class="sr-only" aria-live="polite">{{ count() === 0 ? 'Aucun résultat' : count() === 1 ? '1 résultat' : count() + ' résultats' }}</span>
        </div>
      </div>
    }
  `,
  styles: `
    .pal-item { cursor: pointer; }
    .pal-item .env-dot { margin: 0 4px; }
    .pal-item .path { flex-shrink: 1; max-width: 40%; min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--faint); font-size: calc(11.5 * var(--px)); }
    .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  `,
})
export class Palette {
  protected readonly ws = inject(Workspace);
  private readonly commands = inject(COMMANDS);
  private readonly injector = inject(Injector);
  private readonly input = viewChild<ElementRef<HTMLInputElement>>('input');
  private readonly list = viewChild<ElementRef<HTMLElement>>('list');
  private previous: HTMLElement | null = null;
  private veilPressed = false;

  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;
  protected readonly open = computed(() => this.ws.palette() !== null);
  protected readonly term = computed(() => (this.ws.palette() ?? '').replace(/^>/, '').trim());
  protected readonly groups = computed(() => {
    const q = this.term();
    const groups = (this.ws.palette() ?? '').startsWith('>')
      ? this.commandGroups(q)
      : [
          { title: 'Requêtes', entries: this.requestEntries(q) },
          { title: 'Commandes', entries: this.commandEntries(q).slice(0, q ? 12 : 4) },
          { title: 'Environnements', entries: this.envEntries(q) },
        ].filter((g) => g.entries.length);
    let index = 0;
    for (const g of groups) for (const e of g.entries) e.index = index++;
    return groups;
  });
  protected readonly entries = computed(() => this.groups().flatMap((g) => g.entries));
  protected readonly count = computed(() => this.entries().length);
  protected readonly sel = linkedSignal({ source: this.ws.palette, computation: () => 0 });
  protected readonly current = computed(() => Math.min(this.sel(), Math.max(0, this.count() - 1)));

  constructor() {
    effect(() => {
      if (this.open()) {
        this.previous = document.activeElement as HTMLElement | null;
        afterNextRender(() => this.focusInput(), { injector: this.injector });
      } else if (this.previous) {
        restoreFocus(this.previous);
        this.previous = null;
      }
    });
  }

  protected onKey(e: KeyboardEvent) {
    if (e.isComposing) return;
    const n = this.count();
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      if (n) this.select((this.current() + (e.key === 'ArrowDown' ? 1 : n - 1)) % n);
    } else if (e.key === 'Enter') {
      const entry = this.entries()[this.current()];
      if (entry) this.run(entry);
    } else if (e.key === 'Escape') {
      this.ws.palette.set(null);
    } else if (e.key !== 'Tab') {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
  }

  protected run(entry: Entry) {
    this.ws.palette.set(null);
    void entry.run();
  }

  protected pressVeil(e: MouseEvent) {
    this.veilPressed = e.target === e.currentTarget;
  }

  protected dismiss(e: MouseEvent) {
    if (this.veilPressed && e.target === e.currentTarget) this.ws.palette.set(null);
    this.veilPressed = false;
  }

  protected keepFocus(e: MouseEvent) {
    if (e.target !== this.input()?.nativeElement) e.preventDefault();
  }

  private select(index: number) {
    this.sel.set(index);
    const list = this.list()?.nativeElement;
    if (index === 0 && list) list.scrollTop = 0;
    else document.getElementById(`pal-o-${index}`)?.scrollIntoView({ block: 'nearest' });
  }

  private focusInput() {
    const el = this.input()?.nativeElement;
    el?.focus();
    el?.setSelectionRange(el.value.length, el.value.length);
  }

  private requestEntries(q: string): Entry[] {
    const c = this.ws.collection();
    if (!c) return [];
    const tabs = new Map(this.ws.tabs().map((t) => [t.path, t.doc.url]));
    const all = requests(c.items).map((r) => ({ ...r, url: shortUrl(tabs.get(r.path) ?? r.url) }));
    const byPath = new Map(all.map((r) => [r.path, r]));
    const recent = [...tabs.keys(), ...this.ws.history().map((h) => h.path)].flatMap((p) => byPath.get(p) ?? []);
    return rank(q ? all : [...new Set([...recent, ...all])], q, (r) => [r.name, r.url, r.path])
      .slice(0, q ? 8 : 5)
      .map(({ item, field, marks }) => ({
        key: item.path,
        index: 0,
        method: item.method,
        label: segments(item.name, field === 0 ? marks : []),
        sub: segments(item.url, field === 1 ? marks : []),
        path: segments(item.path, field === 2 ? marks : []),
        run: () => this.ws.openRequest(item.path, true),
      }));
  }

  private commandEntries(q: string): (Entry & { group: string })[] {
    const enabled = this.commands.all().filter((c) => this.commands.enabled(c));
    return rank(enabled, q, (c) => [c.title]).map(({ item, marks }) => ({
      key: item.id,
      index: 0,
      group: item.group,
      icon: item.icon,
      label: segments(item.title, marks),
      sub: [],
      shortcut: item.keys && shortcutLabel(item.keys),
      run: () => this.commands.execute(item),
    }));
  }

  private commandGroups(q: string): Group[] {
    const groups = new Map<string, Entry[]>();
    for (const e of this.commandEntries(q)) groups.set(e.group, [...(groups.get(e.group) ?? []), e]);
    return [...groups].map(([title, entries]) => ({ title, entries }));
  }

  private envEntries(q: string): Entry[] {
    const c = this.ws.collection();
    if (!c) return [];
    return rank([...c.environments, null], q, (e) => [envLabel(e)]).map(({ item, marks }) => ({
      key: item ?? '',
      index: 0,
      env: item,
      label: segments(envLabel(item), marks),
      sub: [],
      active: this.ws.env() === item,
      run: () => this.ws.setEnv(item),
    }));
  }
}
