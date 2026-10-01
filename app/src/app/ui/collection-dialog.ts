import { ChangeDetectionStrategy, Component, ElementRef, afterNextRender, computed, inject, signal, viewChild } from '@angular/core';

import { api } from '../core/api';
import { shortcutLabel } from '../core/commands';
import { joinPath } from '../core/paths';
import { Workspace } from '../core/store';
import { validateName } from '../core/tree-ops';
import { Dialog } from './dialog';
import { Icon } from './icon';

const lastSegment = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? '';

/** « Créer une collection ici » dans un dossier existant, ou « Nouvelle collection » dans un dossier parent à choisir. */
@Component({
  selector: 'app-collection-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  template: `
    <app-dialog [heading]="here() ? 'Créer une collection ici' : 'Nouvelle collection'" [busy]="busy()" (closed)="close()" (confirmed)="run()">
      @if (ws.demo) {
        <div class="note"><app-ic name="alert" [size]="14" /><span>Mode démo : la collection n'existe qu'en mémoire, rien n'est écrit sur ton disque.</span></div>
      }
      @if (here(); as t) {
        <section class="fld">
          <div class="sec-head"><span class="sec-title">Dossier</span></div>
          <span class="input mono path-box" [title]="t.dir">{{ t.dir }}</span>
        </section>
        @if (t.kind === 'other') {
          <div class="note"><app-ic name="alert" [size]="14" /><span>Le dossier n'est pas vide ; rien d'existant n'est modifié. On y ajoute <span class="mono">opencollection.yml</span>, et <span class="mono">.gitignore</span> s'il n'y en a pas déjà.</span></div>
        }
      } @else {
        <section class="fld">
          <div class="sec-head"><span class="sec-title">Dossier parent</span><span class="sec-meta">la collection y est créée dans un nouveau dossier</span></div>
          <div class="row-fields">
            <span class="input mono path-box" [class.is-empty]="!parent()" [title]="parent() ?? ''">{{ parent() ?? 'Aucun dossier choisi' }}</span>
            <button class="btn" [disabled]="busy()" (click)="chooseParent()"><app-ic name="folder-open" [size]="14" />Choisir…</button>
          </div>
        </section>
      }
      <label class="fld">
        <span class="sec-head"><span class="sec-title">Nom de la collection</span></span>
        <input
          #field
          class="input"
          [class.is-bad]="!!shown()"
          type="text"
          data-autofocus
          spellcheck="false"
          autocomplete="off"
          aria-label="Nom de la collection"
          [attr.aria-invalid]="!!shown()"
          [value]="name()"
          [readOnly]="!!created()"
          (input)="edit($any($event.target).value)"
          (keydown)="onKey($event)"
          placeholder="Mes API"
        />
        @if (shown(); as e) {
          <span class="field-error" role="alert">{{ e }}</span>
        }
      </label>
      @if (target(); as t) {
        <p class="target"><app-ic name="folder" [size]="13" />Sera créé : <span class="mono clip-text" [title]="t">{{ t }}</span></p>
      }
      <p class="hint"><app-ic name="disk" [size]="13" /><span>Seuls <span class="mono">opencollection.yml</span> et <span class="mono">.gitignore</span> sont écrits, comme dans Bruno.</span></p>
      @if (error(); as e) {
        <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
      }
      <div dialog-foot class="foot-actions">
        <button class="btn ghost" [disabled]="busy()" (click)="close()">Annuler</button>
        <button class="btn-primary" [class.is-sending]="busy()" [disabled]="!ready() || busy()" (click)="run()">
          @if (busy()) {
            <span class="spinner"></span>{{ created() ? 'Ouverture…' : 'Création…' }}
          } @else {
            {{ created() ? 'Ouvrir la collection' : 'Créer la collection' }} <kbd class="kbd">{{ confirmKey }}</kbd>
          }
        </button>
      </div>
    </app-dialog>
  `,
  styles: `
    .path-box { display: flex; align-items: center; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
    .path-box.is-empty { color: var(--faint); font-family: var(--font-ui); }
    .target { display: flex; align-items: center; gap: 6px; margin: -6px 0 0; min-width: 0; font-size: calc(12 * var(--px)); color: var(--muted); }
    .target .ic { color: var(--faint); }
    .target .mono { color: var(--ink); font-size: calc(12 * var(--px)); }
    .hint { display: flex; align-items: flex-start; gap: 8px; margin: 0; color: var(--faint); font-size: calc(12 * var(--px)); }
    .hint .ic { margin-top: 2px; }
    .hint .mono, .note .mono { color: var(--muted); }
    .field-error { margin-top: 6px; color: var(--bad); font-size: calc(12 * var(--px)); }
  `,
})
export class CollectionDialog {
  protected readonly ws = inject(Workspace);
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly here = computed(() => this.ws.folderTarget());
  protected readonly parent = signal<string | null>(null);
  protected readonly name = signal(lastSegment(this.ws.folderTarget()?.dir ?? ''));
  protected readonly busy = signal(false);
  protected readonly error = signal<string | null>(null);
  /** Racine déjà créée dont l'ouverture a échoué : un nouveau clic rouvre, sans rien recréer. */
  protected readonly created = signal<string | null>(null);
  private readonly touched = signal(this.name() !== '');
  protected readonly problem = computed(() => validateName(this.name(), 'collection'));
  protected readonly shown = computed(() => (this.touched() ? this.problem() : null));
  protected readonly target = computed(() => {
    const parent = this.parent();
    const name = this.name().trim();
    return !this.here() && parent && name ? joinPath(parent, name) : null;
  });
  protected readonly ready = computed(() => !this.problem() && (!!this.here() || !!this.parent()));

  private readonly field = viewChild<ElementRef<HTMLInputElement>>('field');

  constructor() {
    afterNextRender(() => this.field()?.nativeElement.select());
  }

  protected onKey(event: KeyboardEvent) {
    if (event.key === 'Enter' && !event.isComposing) void this.run();
  }

  protected edit(name: string) {
    this.name.set(name);
    this.touched.set(true);
    this.error.set(null);
  }

  protected close() {
    this.ws.dialog.set(null);
  }

  protected async chooseParent() {
    const picked = await api.pickFolder('Dossier où créer la collection');
    if (picked) this.parent.set(picked);
  }

  protected async run() {
    this.touched.set(true);
    if (!this.ready() || this.busy()) return;
    const here = this.here();
    const parent = this.parent();
    const name = this.name().trim();
    this.busy.set(true);
    this.error.set(null);
    try {
      const root = this.created() ?? (here ? await api.initCollection(here.dir, name) : await api.createCollection(parent!, name));
      this.created.set(root);
      if (!(await this.ws.open(root, true))) {
        this.error.set(`Collection créée dans ${root}, mais impossible de l'ouvrir : ${this.ws.error() ?? 'réessaie dans un instant'}`);
        return;
      }
      this.close();
      this.ws.notify(`Collection créée : ${name}`);
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.busy.set(false);
    }
  }
}
