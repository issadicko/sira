import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { shortcutLabel } from '../core/commands';
import { GroupBy, OpenApiPreview } from '../core/model';
import { importedMessage, specLabel } from '../core/openapi';
import { joinPath } from '../core/paths';
import { Workspace } from '../core/store';
import { Dialog } from './dialog';
import { Icon } from './icon';

const GROUPINGS: { value: GroupBy; label: string; hint: string }[] = [
  { value: 'tags', label: 'Tags', hint: 'un dossier par tag' },
  { value: 'path', label: 'Chemins', hint: 'des dossiers selon les chemins' },
];

/** Importe une spec OpenAPI 3.0 / 3.1 ou Swagger 2.0 (fichier ou URL) dans une nouvelle collection. */
@Component({
  selector: 'app-openapi-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  template: `
    <app-dialog heading="Importer une spec OpenAPI" [busy]="importing()" (closed)="close()" (confirmed)="run()">
      @if (ws.demo) {
        <div class="note"><app-ic name="alert" [size]="14" /><span>Mode démo : l'aperçu est fictif et l'import n'écrit rien sur ton disque.</span></div>
      }
      <section class="fld">
        <label class="sec-head" for="spec-source"><span class="sec-title">Source</span><span class="sec-meta">un fichier .yaml, .yml ou .json, ou une URL http(s)</span></label>
        <div class="row-fields">
          <input
            id="spec-source"
            class="input mono"
            data-autofocus
            type="text"
            spellcheck="false"
            autocomplete="off"
            placeholder="Chemin du fichier ou URL https://…"
            [value]="source()"
            (input)="edit($any($event.target).value)"
            (keydown.enter)="analyse()"
          />
          <button class="btn" (click)="chooseFile()" [disabled]="analysing()"><app-ic name="folder-open" [size]="14" />Choisir un fichier…</button>
          <button class="btn" (click)="analyse()" [disabled]="!source().trim() || analysing()">
            @if (analysing()) {
              <span class="spinner"></span>Analyse…
            } @else {
              Analyser
            }
          </button>
        </div>
        @if (sourceError(); as e) {
          <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
        }
      </section>

      @if (preview(); as p) {
        <section class="spec-card" aria-label="Aperçu de la spécification">
          <div class="spec-head">
            <span class="spec-title">{{ p.summary.title }}</span>
            @if (p.summary.version) {
              <span class="tag">{{ p.summary.version }}</span>
            }
            <span class="tag spec">{{ specLabel(p.summary) }}</span>
            <span class="spec-count tnum">{{ p.summary.operationCount }} {{ p.summary.operationCount > 1 ? 'opérations' : 'opération' }}</span>
          </div>
          <dl class="spec-meta">
            <dt>Tags</dt>
            <dd class="spec-tags">
              @for (t of p.summary.tags; track t) {
                <span class="tag">{{ t }}</span>
              } @empty {
                <span class="faint">aucun : les requêtes seront à la racine</span>
              }
            </dd>
            <dt>Serveurs</dt>
            <dd class="mono">
              @for (s of p.summary.servers; track s) {
                <div class="clip-text" [title]="s">{{ s }}</div>
              } @empty {
                <span class="faint">aucun déclaré</span>
              }
            </dd>
          </dl>
        </section>

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
    .row-fields { display: flex; flex-wrap: wrap; gap: 6px; min-width: 0; }
    .row-fields .input { flex: 1 1 220px; min-width: 0; }
    .path-box { display: flex; align-items: center; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
    .path-box.is-empty { color: var(--faint); font-family: var(--font-ui); }
    .spec-card { border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); padding: 10px 12px; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
    .spec-head { display: flex; align-items: center; gap: 6px; min-width: 0; }
    .spec-title { font-weight: 600; font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; margin-right: 2px; }
    .spec-count { margin-left: auto; flex-shrink: 0; color: var(--muted); font-size: 12px; }
    .spec-meta { margin: 0; display: grid; grid-template-columns: 64px minmax(0, 1fr); gap: 6px 10px; font-size: 12px; }
    .spec-meta dt { color: var(--faint); line-height: 17px; }
    .spec-meta dd { margin: 0; min-width: 0; line-height: 17px; }
    .spec-tags { display: flex; flex-wrap: wrap; gap: 4px; }
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
  protected readonly specLabel = specLabel;
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly source = signal('');
  protected readonly preview = signal<OpenApiPreview | null>(null);
  protected readonly analysing = signal(false);
  protected readonly sourceError = signal<string | null>(null);
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

  private pending = 0;

  protected edit(source: string) {
    this.pending++;
    this.analysing.set(false);
    this.source.set(source);
    this.preview.set(null);
    this.sourceError.set(null);
    this.importError.set(null);
  }

  protected close() {
    this.ws.dialog.set(null);
  }

  protected async chooseFile() {
    const picked = await api.pickSpecFile();
    if (!picked) return;
    this.edit(picked);
    await this.analyse();
  }

  protected async chooseParent() {
    const picked = await api.pickFolder('Dossier où créer la collection');
    if (picked) this.parent.set(picked);
  }

  protected async analyse() {
    const source = this.source().trim();
    if (!source || this.analysing()) return;
    const pending = ++this.pending;
    this.analysing.set(true);
    this.preview.set(null);
    this.sourceError.set(null);
    try {
      const preview = await api.previewOpenApi(source);
      if (pending === this.pending) this.preview.set(preview);
    } catch (e) {
      if (pending === this.pending) this.sourceError.set(String(e));
    } finally {
      if (pending === this.pending) this.analysing.set(false);
    }
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
