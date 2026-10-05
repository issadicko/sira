import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { EnvStore } from '../core/env-store';
import { Workspace } from '../core/store';
import { Menu, MenuEntry } from './menu';

/** Menu d'un environnement : renommer, dupliquer, choisir celui qui s'ouvre par défaut, supprimer. */
@Component({
  selector: 'app-env-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Menu],
  host: { style: 'display: contents' },
  template: `
    @for (m of open(); track m) {
      <app-menu [items]="entries()" [x]="m.x" [y]="m.y" [label]="'Actions sur ' + m.env" [heading]="m.env" (closed)="store.closeMenu()" />
    }
  `,
})
export class EnvMenu {
  protected readonly store = inject(EnvStore);
  private readonly ws = inject(Workspace);

  protected readonly open = computed(() => {
    const menu = this.store.menu();
    return menu ? [menu] : [];
  });

  protected readonly entries = computed<MenuEntry[]>(() => {
    const env = this.store.menu()?.env;
    if (!env) return [];
    const isDefault = this.ws.collection()?.defaultEnvironment === env;
    return [
      { label: 'Renommer…', icon: 'pencil', run: () => this.store.beginNaming({ mode: 'rename', env }) },
      { label: 'Dupliquer…', icon: 'copy', run: () => this.store.beginNaming({ mode: 'clone', env }) },
      'sep',
      isDefault
        ? { label: 'Ne plus ouvrir par défaut', icon: 'x', run: () => this.store.setDefault(null) }
        : { label: 'Ouvrir par défaut', icon: 'check', run: () => this.store.setDefault(env) },
      'sep',
      { label: 'Supprimer…', icon: 'trash', run: () => this.store.remove(env) },
    ];
  });
}
