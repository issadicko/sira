import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { shortcutLabel } from '../core/commands';
import { GroupBy, OpenApiPreview } from '../core/model';
import { importedMessage } from '../core/openapi';
import { joinPath } from '../core/paths';
import { Workspace } from '../core/store';
import { Dialog } from './dialog';
import { Icon } from './icon';
import { SpecSource } from './spec-source';

const GROUPINGS: { value: GroupBy; label: string; hint: string }[] = [
  { value: 'tags', label: 'Tags', hint: 'un dossier par tag' },
  { value: 'path', label: 'Chemins', hint: 'des dossiers selon les chemins' },
];

/** Importe une spec OpenAPI 3.0 / 3.1 ou Swagger 2.0 (fichier ou URL) dans une nouvelle collection. */
@Component({
  selector: 'app-openapi-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon, SpecSource],
  template: `
    <app-dialog heading="Importer une spec OpenAPI" [busy]="importing()" (closed)="close()" (confirmed)="run()">
      @if (ws.demo) {
        <div class="note"><app-ic name="alert" [size]="14" /><span>Mode démo : l'aperçu est fictif et l'import n'écrit rien sur ton disque.</span></div>
      }
      <app-spec-source [(source)]="source" [(preview)]="preview" (edited)="importError.set(null)" />

      @if (preview()) {
        <section class="fld">
          <div class="sec-head"><span class="sec-title">Regrouper par</span><span class="sec-meta">{{ hint() }}</span></div>
          <div class="seg" role="group" aria-label="Regroupement des requêtes">
            @for (g of groupings; track g.value) {
              <button [attr.aria-pressed]="groupBy() === g.value" (click)="groupBy.set(g.value)">{{ g.label }}</button>
            }
          </div>
        </section>

        <section class="fld">
          <div class="sec-head"><span class="sec-title">Dossier parent</span><span class="sec-meta">la collection y est créée dans un nouveau dossier</span></div>
          <div class="row-fields">
            <span class="input mono path-box" [class.is-empty]="!parent()" [title]="parent() ?? ''">{{ parent() ?? 'Aucun dossier choisi' }}</span>
            <button class="btn" (click)="chooseParent()"><app-ic name="folder-open" [size]="14" />Choisir…</button>
          </div>
          @if (target(); as t) {
            <p class="target"><app-ic name="folder" [size]="13" />Sera créé : <span class="mono clip-text" [title]="t">{{ t }}</span></p>
          }
        </section>

        <p class="hint"><app-ic name="disk" [size]="13" /><span>La source de la spec et un instantané de chaque opération sont gardés dans <span class="mono">.oc-sync/</span> pour la future synchronisation.</span></p>
      }

      @if (importError(); as e) {
        <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
      }
      <div dialog-foot class="foot-actions">
        <button class="btn ghost" [disabled]="importing()" (click)="close()">Annuler</button>
        <button class="btn-primary" [class.is-sending]="importing()" [disabled]="!target() || importing()" (click)="run()">
          @if (importing()) {
            <span class="spinner"></span>Import en cours…
          } @else {
            <app-ic name="import" [size]="15" />Importer <kbd class="kbd">{{ confirmKey }}</kbd>
          }
        </button>
      </div>
    </app-dialog>
  `,
  styles: `
    .path-box { display: flex; align-items: center; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
    .path-box.is-empty { color: var(--faint); font-family: var(--font-ui); }
    .target { display: flex; align-items: center; gap: 6px; margin: 8px 0 0; min-width: 0; font-size: 12px; color: var(--muted); }
    .target .ic { color: var(--faint); }
    .target .mono { color: var(--ink); font-size: 12px; }
    .hint { display: flex; align-items: flex-start; gap: 8px; margin: 0; color: var(--faint); font-size: 12px; }
    .hint .ic { margin-top: 2px; }
    .hint .mono { color: var(--muted); }
  `,
})
export class OpenApiDialog {
  protected readonly ws = inject(Workspace);
  protected readonly groupings = GROUPINGS;
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly source = signal('');
  protected readonly preview = signal<OpenApiPreview | null>(null);
  protected readonly groupBy = signal<GroupBy>('tags');
  protected readonly parent = signal<string | null>(null);
  protected readonly importing = signal(false);
  protected readonly importError = signal<string | null>(null);
  protected readonly hint = computed(() => GROUPINGS.find((g) => g.value === this.groupBy())?.hint);
  protected readonly target = computed(() => {
    const preview = this.preview();
    const parent = this.parent();
    return preview && parent ? joinPath(parent, preview.folderName) : null;
  });

  protected close() {
    this.ws.dialog.set(null);
  }

  protected async chooseParent() {
    const picked = await api.pickFolder('Dossier où créer la collection');
    if (picked) this.parent.set(picked);
  }

  protected async run() {
    const parent = this.parent();
    if (!this.preview() || !parent || this.importing()) return;
    this.importing.set(true);
    this.importError.set(null);
    try {
      if (!(await this.ws.confirmReplace())) return;
      const root = await api.importOpenApi(this.source().trim(), parent, this.groupBy());
      if (!(await this.ws.open(root, true))) {
        this.importError.set(`Collection créée dans ${root}, mais impossible de l'ouvrir : ${this.ws.error()}`);
        return;
      }
      const count = this.ws.collection()?.requestCount ?? 0;
      this.close();
      this.ws.notify(importedMessage(count));
    } catch (e) {
      this.importError.set(String(e));
    } finally {
      this.importing.set(false);
    }
  }
}
