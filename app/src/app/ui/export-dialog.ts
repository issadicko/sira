import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { shortcutLabel } from '../core/commands';
import { EXPORT_FORMATS, exportFileName, exportSummary } from '../core/export';
import { ExportFormat } from '../core/model';
import { Workspace } from '../core/store';
import { Dialog } from './dialog';
import { Icon } from './icon';

/**
 * Exporte la collection ouverte en JSON, au format Postman v2.1 ou OpenAPI 3.0.3, dans un fichier que l'utilisateur
 * choisit. La collection n'est pas modifiée ; ce que le format ne sait pas porter est listé à la fin.
 */
@Component({
  selector: 'app-export-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  template: `
    <app-dialog heading="Exporter la collection" [busy]="exporting()" (closed)="close()" (confirmed)="done() ? close() : run()">
      @if (ws.demo) {
        <div class="note"><app-ic name="alert" [size]="14" /><span>Mode démo : l'export écrit un fichier sur ton disque et n'est disponible que dans l'application desktop.</span></div>
      }

      @if (!done()) {
        <section class="fld">
          <div class="sec-head"><span class="sec-title">Format</span><span class="sec-meta">{{ ws.collection()?.name }}</span></div>
          <div class="seg" role="group" aria-label="Format d'export">
            @for (f of formats; track f.id) {
              <button [attr.aria-pressed]="format() === f.id" (click)="setFormat(f.id)">{{ f.label }} {{ f.version }}</button>
            }
          </div>
        </section>

        @if (format() === 'postman') {
          <p class="hint"><app-ic name="shield" [size]="13" /><span>Les scripts <span class="mono">bru.*</span> sont traduits en <span class="mono">pm.*</span> pour les appels courants. Les valeurs écrites en clair dans les requêtes (jeton, mot de passe) sont exportées <b>telles quelles</b> ; les environnements et leurs secrets ne le sont pas.</span></p>
        } @else {
          <p class="hint"><app-ic name="shield" [size]="13" /><span>Les adresses deviennent des serveurs (<span class="mono" ngNonBindable>{{baseUrl}}</span> et ses valeurs par environnement), les dossiers des étiquettes, l'authentification des schémas de sécurité <b>sans aucune valeur</b>. Les scripts et les tests ne sont pas décrits : OpenAPI n'en a pas.</span></p>
        }
      } @else {
        <div class="banner" role="status"><app-ic name="check-circle" [size]="15" /><span>{{ summary() }}<br /><span class="mono path">{{ path() }}</span></span></div>
        @if (issues().length) {
          <section class="fld">
            <div class="sec-head"><span class="sec-title">Ce qui n'a pas été exporté</span></div>
            <ul class="issues">
              @for (i of issues(); track $index) {
                <li>{{ i }}</li>
              }
            </ul>
          </section>
        }
      }

      @if (failure(); as e) {
        <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
      }
      <div dialog-foot class="foot-actions">
        @if (done()) {
          <button class="btn-primary" (click)="close()">Terminer</button>
        } @else {
          <button class="btn ghost" [disabled]="exporting()" (click)="close()">Annuler</button>
          <button class="btn-primary" [class.is-sending]="exporting()" [disabled]="!ws.collection() || exporting()" (click)="run()">
            @if (exporting()) {
              <span class="spinner"></span>Export en cours…
            } @else {
              <app-ic name="export" [size]="15" />Exporter… <kbd class="kbd">{{ confirmKey }}</kbd>
            }
          </button>
        }
      </div>
    </app-dialog>
  `,
  styles: `
    .hint { display: flex; align-items: flex-start; gap: 8px; margin: 0; color: var(--faint); font-size: calc(12 * var(--px)); }
    .hint .ic { margin-top: 2px; flex: none; }
    .hint .mono, .hint b { color: var(--muted); font-weight: 500; }
    .path { overflow-wrap: anywhere; color: var(--muted); }
    .issues { list-style: none; margin: 0; padding: 0; max-height: 220px; overflow: auto; border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); }
    .issues li { padding: 7px 10px; font-size: calc(12 * var(--px)); color: var(--muted); overflow-wrap: anywhere; }
    .issues li + li { border-top: 1px solid var(--line); }
  `,
})
export class ExportDialog {
  protected readonly ws = inject(Workspace);
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly formats = EXPORT_FORMATS;
  protected readonly format = signal<ExportFormat>('postman');
  protected readonly exporting = signal(false);
  protected readonly failure = signal<string | null>(null);
  protected readonly issues = signal<string[]>([]);
  protected readonly path = signal('');
  protected readonly done = signal(false);
  protected readonly summary = computed(() => exportSummary(this.issues().length));

  protected close() {
    if (!this.exporting()) this.ws.dialog.set(null);
  }

  protected setFormat(format: ExportFormat) {
    this.format.set(format);
    this.failure.set(null);
  }

  protected async run() {
    const collection = this.ws.collection();
    if (!collection || this.exporting()) return;
    this.exporting.set(true);
    this.failure.set(null);
    try {
      const path = await api.pickExportPath(exportFileName(collection.name, this.format()));
      if (!path) return;
      const issues = await api.exportCollection(collection.root, this.format(), path);
      if (!issues.length) {
        this.ws.dialog.set(null);
        this.ws.notify(`${exportSummary(0)} dans ${path}`);
        return;
      }
      this.path.set(path);
      this.issues.set(issues);
      this.done.set(true);
    } catch (e) {
      this.failure.set(String(e));
    } finally {
      this.exporting.set(false);
    }
  }
}
