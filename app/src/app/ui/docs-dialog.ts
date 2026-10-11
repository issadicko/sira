import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { shortcutLabel } from '../core/commands';
import { Workspace } from '../core/store';
import { Dialog } from './dialog';
import { Icon } from './icon';
import { MarkdownView } from './markdown-view';

/** Documentation Markdown de la collection ou d'un dossier : écrite dans `opencollection.yml` ou `folder.yml`. */
@Component({
  selector: 'app-docs-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon, MarkdownView],
  template: `
    <app-dialog [heading]="heading()" [busy]="saving()" (closed)="close()" (confirmed)="save()">
      <div class="docs-bar">
        <span class="sec-meta clip-text">{{ file() }}</span>
        <div class="seg" role="group" aria-label="Mode">
          <button [attr.aria-pressed]="!preview()" (click)="preview.set(false)">Édition</button>
          <button [attr.aria-pressed]="preview()" (click)="preview.set(true)">Aperçu</button>
        </div>
      </div>
      @if (loading()) {
        <p class="hint"><span class="spinner"></span>Lecture…</p>
      } @else if (preview()) {
        <div class="docs-preview" tabindex="0" aria-label="Aperçu de la documentation">
          @if (text().trim()) {
            <app-markdown [source]="text()" />
          } @else {
            <p class="hint">Rien à montrer : la documentation est vide.</p>
          }
        </div>
      } @else {
        <textarea class="docs-edit" data-autofocus spellcheck="false" aria-label="Documentation Markdown" [value]="text()" [readOnly]="saving()" (input)="text.set($any($event.target).value)" [placeholder]="placeholder()"></textarea>
      }
      @if (ws.demo) {
        <div class="note"><app-ic name="alert" [size]="14" /><span>Mode démo : la documentation n'existe qu'en mémoire, rien n'est écrit sur ton disque.</span></div>
      }
      @if (error(); as e) {
        <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
      }
      @if (asking()) {
        <div class="banner" role="alert">
          <app-ic name="alert" [size]="15" />
          <span>La documentation a été modifiée sans être enregistrée.</span>
          <button class="btn ghost" (click)="ws.closeDocs()">Abandonner les modifications</button>
          <button class="btn ghost" (click)="asking.set(false)">Continuer</button>
        </div>
      }
      <div dialog-foot class="foot-actions">
        <button class="btn ghost" [disabled]="saving()" (click)="close()">Fermer</button>
        <button class="btn-primary" [class.is-sending]="saving()" [disabled]="!dirty() || saving() || loading()" (click)="save()">
          @if (saving()) {
            <span class="spinner"></span>Enregistrement…
          } @else {
            Enregistrer <kbd class="kbd">{{ confirmKey }}</kbd>
          }
        </button>
      </div>
    </app-dialog>
  `,
  styles: `
    .docs-bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 8px; }
    .docs-edit, .docs-preview { display: block; width: 100%; height: min(52vh, 460px); min-height: 220px; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--line); background: var(--sunken); overflow: auto; }
    .docs-edit { resize: none; font: 1rem/calc(20 * var(--px)) var(--font-ui); font-variant-ligatures: none; tab-size: 2; outline: 0; color: var(--ink); }
    .docs-edit:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
  `,
})
export class DocsDialog {
  protected readonly ws = inject(Workspace);
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly text = signal('');
  protected readonly loading = signal(true);
  protected readonly saving = signal(false);
  protected readonly preview = signal(false);
  protected readonly asking = signal(false);
  protected readonly error = signal<string | null>(null);
  private saved = '';
  private readonly folder = this.ws.docsFolder() ?? '';

  protected readonly dirty = computed(() => this.text() !== this.saved);
  protected readonly heading = computed(() => (this.folder ? `Documentation du dossier « ${this.folder.split('/').pop()} »` : 'Documentation de la collection'));
  protected readonly file = computed(() => (this.folder ? `${this.folder}/folder.yml` : 'opencollection.yml'));
  protected readonly placeholder = computed(() => (this.folder ? 'Documentation Markdown du dossier' : 'Documentation Markdown de la collection'));

  constructor() {
    void this.load();
  }

  private async load() {
    const root = this.ws.collection()?.root;
    if (!root) return;
    try {
      this.saved = await api.readDocs(root, this.folder);
      this.text.set(this.saved);
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.loading.set(false);
    }
  }

  protected close() {
    if (this.dirty() && !this.error()) {
      this.asking.set(true);
      return;
    }
    this.ws.closeDocs();
  }

  protected async save() {
    const root = this.ws.collection()?.root;
    if (!root || !this.dirty() || this.saving() || this.loading()) return;
    this.saving.set(true);
    this.error.set(null);
    try {
      await api.saveDocs(root, this.folder, this.text());
      this.saved = this.text();
      this.ws.notify('Documentation enregistrée');
      this.ws.closeDocs();
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.saving.set(false);
    }
  }
}
