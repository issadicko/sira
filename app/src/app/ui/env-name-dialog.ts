import { ChangeDetectionStrategy, Component, ElementRef, afterNextRender, computed, inject, signal, viewChild } from '@angular/core';

import { shortcutLabel } from '../core/commands';
import { EnvStore } from '../core/env-store';
import { Workspace } from '../core/store';
import { Dialog } from './dialog';
import { Icon } from './icon';

const TEXTS = {
  create: { heading: 'Nouvel environnement', action: 'Créer', busy: 'Création…' },
  rename: { heading: "Renommer l'environnement", action: 'Renommer', busy: 'Renommage…' },
  clone: { heading: "Dupliquer l'environnement", action: 'Dupliquer', busy: 'Duplication…' },
};

/** Saisie du nom d'un environnement : création, renommage ou duplication. */
@Component({
  selector: 'app-env-name-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  host: { style: 'display: contents' },
  template: `
    @if (store.naming(); as n) {
      <app-dialog [heading]="text().heading" [busy]="store.namingBusy()" (closed)="store.cancelNaming()" (confirmed)="run()">
        @if (ws.demo) {
          <div class="note"><app-ic name="alert" [size]="14" /><span>Mode démo : l'environnement n'existe qu'en mémoire, rien n'est écrit sur ton disque.</span></div>
        }
        <label class="fld">
          <span class="sec-head"><span class="sec-title">Nom de l'environnement</span></span>
          <input
            #field
            class="input"
            [class.is-bad]="!!shown()"
            type="text"
            data-autofocus
            spellcheck="false"
            autocomplete="off"
            aria-label="Nom de l'environnement"
            [attr.aria-invalid]="!!shown()"
            [value]="name()"
            [readOnly]="store.namingBusy()"
            (input)="edit($any($event.target).value)"
            (keydown)="onKey($event)"
            placeholder="dev, recette, prod…"
          />
          @if (shown(); as e) {
            <span class="field-error" role="alert">{{ e }}</span>
          }
        </label>
        @if (file(); as f) {
          <p class="target"><app-ic name="file" [size]="13" />Fichier : <span class="mono clip-text">environments/{{ f }}.yml</span>@if (renamed()) {<span class="faint">(nom déjà pris ou caractères remplacés)</span>}</p>
        }
        @if (n.mode === 'clone') {
          <p class="hint"><app-ic name="copy" [size]="13" /><span>Les variables de « {{ n.env }} » sont copiées. Les modifications non enregistrées n'en font pas partie.</span></p>
        }
        @if (store.namingError(); as e) {
          <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
        }
        <div dialog-foot class="foot-actions">
          <button class="btn ghost" [disabled]="store.namingBusy()" (click)="store.cancelNaming()">Annuler</button>
          <button class="btn-primary" [class.is-sending]="store.namingBusy()" [disabled]="!!problem() || store.namingBusy()" (click)="run()">
            @if (store.namingBusy()) {
              <span class="spinner"></span>{{ text().busy }}
            } @else {
              {{ text().action }} <kbd class="kbd">{{ confirmKey }}</kbd>
            }
          </button>
        </div>
      </app-dialog>
    }
  `,
  styles: `
    .target { display: flex; align-items: center; gap: 6px; margin: -6px 0 0; min-width: 0; font-size: calc(12 * var(--px)); color: var(--muted); }
    .target .ic { color: var(--faint); }
    .target .mono { color: var(--ink); font-size: calc(12 * var(--px)); }
    .hint { display: flex; align-items: flex-start; gap: 8px; margin: 0; color: var(--faint); font-size: calc(12 * var(--px)); }
    .hint .ic { margin-top: 2px; }
    .field-error { margin-top: 6px; color: var(--bad); font-size: calc(12 * var(--px)); }
  `,
})
export class EnvNameDialog {
  protected readonly ws = inject(Workspace);
  protected readonly store = inject(EnvStore);
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly name = signal(this.store.suggestion(this.store.naming() ?? { mode: 'create' }));
  private readonly touched = signal(this.name() !== '');
  protected readonly text = computed(() => TEXTS[this.store.naming()?.mode ?? 'create']);
  protected readonly problem = computed(() => this.store.problem(this.name()));
  protected readonly shown = computed(() => (this.touched() ? this.problem() : null));
  /** Nom que prendrait le fichier ; rien tant que le nom est refusé. */
  protected readonly file = computed(() => (this.name().trim() && !this.problem() ? this.store.resulting(this.name()) : null));
  protected readonly renamed = computed(() => !!this.file() && this.file() !== this.name().trim());
  private readonly field = viewChild<ElementRef<HTMLInputElement>>('field');

  constructor() {
    afterNextRender(() => this.field()?.nativeElement.select());
  }

  protected edit(name: string) {
    this.name.set(name);
    this.touched.set(true);
  }

  protected onKey(event: KeyboardEvent) {
    if (event.key === 'Enter' && !event.isComposing) void this.run();
  }

  protected async run() {
    this.touched.set(true);
    if (this.problem()) return;
    await this.store.confirmNaming(this.name());
  }
}
