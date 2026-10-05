import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { EnvStore } from '../core/env-store';
import { Workspace } from '../core/store';
import { Icon } from './icon';

/** Barre latérale des environnements : la liste de `environments/`, l'environnement actif et celui qui s'ouvre par défaut. */
@Component({
  selector: 'app-env-sidebar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
    @if (ws.collection(); as c) {
      <div class="pane-head">
        <span class="pane-title">Environnements</span>
        <button class="icon-btn sm" (click)="store.beginNaming({ mode: 'create' })" title="Nouvel environnement" aria-label="Nouvel environnement"><app-ic name="plus" [size]="15" /></button>
      </div>
      <div class="sb-scroll">
        @for (e of c.environments; track e) {
          <button class="row env-row" [class.is-active]="ws.env() === e" style="--d: 0" (click)="ws.setEnv(e)" (contextmenu)="openMenu($event, e)" (keydown.shift.f10)="openMenu($event, e)" (keydown.contextmenu)="openMenu($event, e)">
            <span class="env-dot" [class.prod]="e === 'prod'" style="margin: 0 4px"></span><span class="row-name">{{ e }}</span>
            @if (c.defaultEnvironment === e) {
              <span class="row-meta" title="La collection ouvre cet environnement au démarrage">par défaut</span>
            }
            @if (ws.env() === e) {
              <span class="tag good">actif</span>
            }
          </button>
        } @empty {
          <div class="sb-empty">
            <p>Aucun fichier dans <span class="mono">environments/</span>.</p>
            <div class="actions">
              <button class="btn" (click)="store.beginNaming({ mode: 'create' })"><app-ic name="plus" [size]="14" />Nouvel environnement</button>
            </div>
          </div>
        }
      </div>
    }
  `,
})
export class EnvSidebar {
  protected readonly ws = inject(Workspace);
  protected readonly store = inject(EnvStore);

  protected openMenu(event: Event, env: string) {
    event.preventDefault();
    event.stopPropagation();
    if (event instanceof MouseEvent) {
      this.store.openMenu(event.clientX, event.clientY, env);
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.store.openMenu(rect.left + 24, rect.bottom, env);
  }
}
