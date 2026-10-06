import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { shortcutLabel } from '../core/commands';
import { ImportIssue } from '../core/model';
import { importSummary } from '../core/openapi';
import { Workspace } from '../core/store';
import { Dialog } from './dialog';
import { Icon } from './icon';

type Source = 'postman' | 'insomnia';
type Mode = 'collection' | 'environment';

/**
 * Importe l'export d'un autre client : une collection Postman (v2.0 ou v2.1) ou Insomnia (v4 ou v5) dans un nouveau
 * dossier, ou un environnement Postman dans la collection ouverte (ceux d'Insomnia viennent avec la collection).
 */
@Component({
  selector: 'app-import-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  template: `
    <app-dialog [heading]="'Importer depuis ' + label()" [busy]="importing()" (closed)="close()" (confirmed)="done() ? close() : run()">
      @if (ws.demo) {
        <div class="note"><app-ic name="alert" [size]="14" /><span>Mode démo : l'import lit un fichier de ton disque et n'est disponible que dans l'application desktop.</span></div>
      }

      @if (!done()) {
        <section class="fld">
          <div class="sec-head"><span class="sec-title">Importer depuis</span></div>
          <div class="seg" role="group" aria-label="Application d'origine">
            <button [attr.aria-pressed]="source() === 'postman'" (click)="setSource('postman')">Postman</button>
            <button [attr.aria-pressed]="source() === 'insomnia'" (click)="setSource('insomnia')">Insomnia</button>
          </div>
        </section>

        @if (source() === 'postman') {
          <section class="fld">
            <div class="sec-head"><span class="sec-title">Ce que tu importes</span></div>
            <div class="seg" role="group" aria-label="Type d'import">
              <button [attr.aria-pressed]="mode() === 'collection'" (click)="setMode('collection')">Une collection</button>
              <button [attr.aria-pressed]="mode() === 'environment'" [disabled]="!ws.collection()" (click)="setMode('environment')" [title]="ws.collection() ? '' : 'Ouvre d’abord une collection'">Un environnement</button>
            </div>
          </section>
        }

        <section class="fld">
          <div class="sec-head">
            <span class="sec-title">Fichier</span>
            <span class="sec-meta">{{ fileHint() }}</span>
          </div>
          <div class="row-fields">
            <span class="input mono path-box" [class.is-empty]="!file()" [title]="file() ?? ''">{{ file() ?? 'Aucun fichier choisi' }}</span>
            <button class="btn" (click)="chooseFile()"><app-ic name="folder-open" [size]="14" />Choisir…</button>
          </div>
        </section>

        @if (mode() === 'collection') {
          <section class="fld">
            <div class="sec-head"><span class="sec-title">Dossier parent</span><span class="sec-meta">la collection y est créée dans un nouveau dossier</span></div>
            <div class="row-fields">
              <span class="input mono path-box" [class.is-empty]="!parent()" [title]="parent() ?? ''">{{ parent() ?? 'Aucun dossier choisi' }}</span>
              <button class="btn" (click)="chooseParent()"><app-ic name="folder-open" [size]="14" />Choisir…</button>
            </div>
          </section>
          @if (source() === 'postman') {
            <p class="hint"><app-ic name="shield" [size]="13" /><span>Les scripts <span class="mono">pm.*</span> sont traduits en <span class="mono">bru.*</span> pour les appels courants ; un script plus complexe garde ses <span class="mono">pm.*</span> et se relit à la main.</span></p>
          } @else {
            <p class="hint"><app-ic name="shield" [size]="13" /><span>Les environnements de l'export sont importés avec la collection ; le sous-environnement hérite des variables de l'environnement de base. Les types d'auth qu'Insomnia connaît et pas Sira sont listés à la fin.</span></p>
          }
        } @else {
          <p class="hint"><app-ic name="lock" [size]="13" /><span>L'environnement est ajouté à <b>{{ ws.collection()?.name }}</b> sans en remplacer un autre. Les variables de type « secret » sont importées <b>sans leur valeur</b> : saisis-la ensuite dans le tableau de l'environnement, elle ira dans le trousseau.</span></p>
        }
      } @else {
        <div class="banner" role="status"><app-ic name="check-circle" [size]="15" /><span>{{ summary() }}</span></div>
        @if (issues().length) {
          <section class="fld">
            <div class="sec-head"><span class="sec-title">Ce qui n'a pas été converti</span></div>
            <ul class="issues">
              @for (i of issues(); track $index) {
                <li [class.is-error]="i.severity === 'error'"><span class="where mono">{{ i.path }}</span><span>{{ i.message }}</span></li>
              }
            </ul>
          </section>
        }
      }

      @if (importError(); as e) {
        <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
      }
      <div dialog-foot class="foot-actions">
        @if (done()) {
          <button class="btn-primary" (click)="close()">Terminer</button>
        } @else {
          <button class="btn ghost" [disabled]="importing()" (click)="close()">Annuler</button>
          <button class="btn-primary" [class.is-sending]="importing()" [disabled]="!ready() || importing()" (click)="run()">
            @if (importing()) {
              <span class="spinner"></span>Import en cours…
            } @else {
              <app-ic name="import" [size]="15" />Importer <kbd class="kbd">{{ confirmKey }}</kbd>
            }
          </button>
        }
      </div>
    </app-dialog>
  `,
  styles: `
    .path-box { display: flex; align-items: center; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
    .path-box.is-empty { color: var(--faint); font-family: var(--font-ui); }
    .hint { display: flex; align-items: flex-start; gap: 8px; margin: 0; color: var(--faint); font-size: calc(12 * var(--px)); }
    .hint .ic { margin-top: 2px; flex: none; }
    .hint .mono, .hint b { color: var(--muted); font-weight: 500; }
    .issues { list-style: none; margin: 0; padding: 0; max-height: 220px; overflow: auto; border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); }
    .issues li { display: grid; grid-template-columns: minmax(0, 40%) minmax(0, 1fr); gap: 10px; padding: 7px 10px; font-size: calc(12 * var(--px)); color: var(--muted); }
    .issues li + li { border-top: 1px solid var(--line); }
    .issues li.is-error .where { color: var(--bad); }
    .where { overflow-wrap: anywhere; color: var(--warn); }
  `,
})
export class ImportDialog {
  protected readonly ws = inject(Workspace);
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly source = signal<Source>(this.ws.dialog() === 'insomnia' ? 'insomnia' : 'postman');
  protected readonly label = computed(() => (this.source() === 'insomnia' ? 'Insomnia' : 'Postman'));
  protected readonly mode = signal<Mode>('collection');
  protected readonly file = signal<string | null>(null);
  protected readonly parent = signal<string | null>(null);
  protected readonly importing = signal(false);
  protected readonly importError = signal<string | null>(null);
  protected readonly issues = signal<ImportIssue[]>([]);
  protected readonly summary = signal('');
  protected readonly done = signal(false);
  protected readonly fileHint = computed(() => {
    if (this.source() === 'insomnia') return 'export Insomnia v4 (.json) ou v5 (.yaml)';
    return this.mode() === 'collection' ? 'export de collection Postman (.json)' : 'export d’environnement Postman (.json)';
  });
  protected readonly ready = computed(() => !!this.file() && (this.mode() === 'environment' ? !!this.ws.collection() : !!this.parent()));

  protected close() {
    if (!this.importing()) this.ws.dialog.set(null);
  }

  protected setSource(source: Source) {
    if (this.source() === source) return;
    this.source.set(source);
    this.mode.set('collection');
    this.file.set(null);
    this.importError.set(null);
  }

  protected setMode(mode: Mode) {
    this.mode.set(mode);
    this.importError.set(null);
  }

  protected async chooseFile() {
    const picked = await (this.source() === 'insomnia' ? api.pickInsomniaFile() : api.pickPostmanFile());
    if (picked) this.file.set(picked);
  }

  protected async chooseParent() {
    const picked = await api.pickFolder('Dossier où créer la collection');
    if (picked) this.parent.set(picked);
  }

  protected async run() {
    const file = this.file();
    if (!file || !this.ready() || this.importing()) return;
    this.importing.set(true);
    this.importError.set(null);
    try {
      if (this.mode() === 'environment') return await this.importEnvironment(file);
      if (!(await this.ws.confirmReplace())) return;
      const parent = this.parent()!;
      const { root, issues } = await (this.source() === 'insomnia' ? api.importInsomnia(file, parent) : api.importPostman(file, parent));
      if (!(await this.ws.open(root, true))) {
        this.importError.set(`Collection créée dans ${root}, mais impossible de l'ouvrir : ${this.ws.error()}`);
        return;
      }
      this.finish(importSummary(this.ws.collection()?.requestCount ?? 0, issues), issues);
    } catch (e) {
      this.importError.set(String(e));
    } finally {
      this.importing.set(false);
    }
  }

  private async importEnvironment(file: string) {
    const root = this.ws.collection()?.root;
    if (!root) return;
    const name = await api.importPostmanEnvironment(root, file);
    await this.ws.reload();
    this.ws.notify(`Environnement « ${name} » importé`);
    this.ws.dialog.set(null);
  }

  /** Une fois la collection ouverte : se ferme s'il n'y a rien à signaler, sinon garde la liste de ce qui a été écarté. */
  private finish(summary: string, issues: ImportIssue[]) {
    if (!issues.length) {
      this.ws.dialog.set(null);
      this.ws.notify(summary);
      return;
    }
    this.summary.set(summary);
    this.issues.set(issues);
    this.done.set(true);
  }
}
