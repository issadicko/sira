import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { RunnerStore } from '../core/runner-store';
import { Icon } from './icon';

const basename = (path: string) => path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);

/** Barre latérale du runner : ce qu'on exécute, et comment. */
@Component({
  selector: 'app-runner-sidebar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  styles: `
    .rs-head { padding: 12px 14px 4px; font-size: calc(11.5 * var(--px)); font-weight: 600; color: var(--faint); }
    .rs-head:first-child { padding-top: 4px; }
    .row:disabled { opacity: .55; cursor: default; }
    .row:disabled:hover { background: transparent; }
    .rs-field { padding: 8px 14px 0; display: flex; flex-direction: column; gap: 6px; }
    .rs-lbl { font-size: calc(12 * var(--px)); color: var(--muted); }
    .rs-delay { display: flex; align-items: center; gap: 8px; }
    .rs-delay .input { width: 96px; font-variant-numeric: tabular-nums; }
    .rs-check { display: flex; align-items: center; gap: 8px; padding: 10px 14px 0; font-size: calc(12.5 * var(--px)); cursor: pointer; }
    .rs-check.sub { padding-top: 6px; color: var(--muted); }
    .rs-file { display: flex; align-items: center; gap: 6px; height: 30px; padding: 0 2px 0 8px; border: 1px solid var(--line); border-radius: 6px; background: var(--sunken); }
    .rs-file .ic { color: var(--faint); flex-shrink: 0; }
    .rs-file-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: calc(12.5 * var(--px)); }
    .rs-hint { margin: 0; font-size: calc(11.5 * var(--px)); color: var(--faint); overflow-wrap: anywhere; }
    .rs-field .btn { align-self: flex-start; }
  `,
  template: `
    <div class="pane-head"><span class="pane-title">Runner</span></div>
    <div class="sb-scroll">
      <div class="rs-head">Ce qui s'exécute</div>
      @for (s of store.scopeList(); track s.path) {
        <button class="row" [class.is-active]="s === store.selected()" [style.--d]="s.depth" [disabled]="store.running()" (click)="store.scope.set(s.path)" [attr.aria-pressed]="s === store.selected()">
          <app-ic [name]="s.path ? 'folder' : 'layers'" [size]="14" />
          <span class="row-name">{{ s.name }}</span>
          <span class="row-meta" [title]="s.requests + (s.requests > 1 ? ' requêtes HTTP' : ' requête HTTP')">{{ s.requests }}</span>
        </button>
      }

      <div class="rs-head">Options</div>
      <div class="rs-field">
        <label class="rs-lbl" for="rs-delay">Attente entre deux requêtes</label>
        <div class="rs-delay">
          <input id="rs-delay" class="input mono" type="number" min="0" step="50" inputmode="numeric" [value]="store.delay()" [disabled]="store.running()" (input)="setDelay($any($event.target).value)" />
          <span class="muted">ms</span>
        </div>
      </div>
      <label class="rs-check"><input type="checkbox" class="cb" [checked]="store.bail()" [disabled]="store.running()" (change)="store.bail.set($any($event.target).checked)" />S'arrêter au premier échec</label>
      <div class="rs-field">
        <span class="rs-lbl">Données d'itération</span>
        @if (store.data(); as d) {
          <div class="rs-file">
            <app-ic name="file" [size]="14" />
            <span class="rs-file-name" [title]="d.path">{{ name(d.path) }}</span>
            <button class="icon-btn sm" [disabled]="store.running()" (click)="store.clearData()" title="Retirer ce fichier" aria-label="Retirer le fichier de données"><app-ic name="x" [size]="13" /></button>
          </div>
          <p class="rs-hint">{{ d.info.rows }} {{ d.info.rows > 1 ? 'itérations' : 'itération' }}@if (d.info.columns.length) { · {{ d.info.columns.join(', ') }} }</p>
        } @else {
          <button class="btn" [disabled]="store.running()" (click)="store.pickData()"><app-ic name="import" [size]="14" />Choisir un CSV ou un JSON…</button>
          <p class="rs-hint">Chaque ligne rejoue la sélection, et ses champs deviennent des variables.</p>
        }
      </div>

      <div class="rs-head">Rapport exporté</div>
      <label class="rs-check sub"><input type="checkbox" class="cb" [checked]="store.skipHeaders()" (change)="store.skipHeaders.set($any($event.target).checked)" />Sans les en-têtes</label>
      <label class="rs-check sub"><input type="checkbox" class="cb" [checked]="store.skipBodies()" (change)="store.skipBodies.set($any($event.target).checked)" />Sans les corps des requêtes ni des réponses</label>
    </div>
  `,
})
export class RunnerSidebar {
  protected readonly store = inject(RunnerStore);
  protected readonly name = basename;

  protected setDelay(raw: string) {
    const ms = Number(raw);
    this.store.delay.set(Number.isFinite(ms) && ms > 0 ? Math.round(ms) : 0);
  }
}
