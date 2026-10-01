import { ChangeDetectionStrategy, Component, model, output, signal } from '@angular/core';

import { api } from '../core/api';
import { OpenApiPreview } from '../core/model';
import { specLabel } from '../core/openapi';
import { Icon } from './icon';

/** Choix d'une spec OpenAPI, fichier ou URL, avec l'aperçu de ce qu'elle contient. */
@Component({
  selector: 'app-spec-source',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
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
    }
  `,
  styles: `
    .spec-card { border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); padding: 10px 12px; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
    .spec-head { display: flex; align-items: center; gap: 6px; min-width: 0; }
    .spec-title { font-weight: 600; font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; margin-right: 2px; }
    .spec-count { margin-left: auto; flex-shrink: 0; color: var(--muted); font-size: 12px; }
    .spec-meta { margin: 0; display: grid; grid-template-columns: 64px minmax(0, 1fr); gap: 6px 10px; font-size: 12px; }
    .spec-meta dt { color: var(--faint); line-height: 17px; }
    .spec-meta dd { margin: 0; min-width: 0; line-height: 17px; }
    .spec-tags { display: flex; flex-wrap: wrap; gap: 4px; }
  `,
})
export class SpecSource {
  readonly source = model('');
  readonly preview = model<OpenApiPreview | null>(null);
  readonly edited = output<void>();
  protected readonly specLabel = specLabel;
  protected readonly analysing = signal(false);
  protected readonly sourceError = signal<string | null>(null);

  private pending = 0;

  protected edit(source: string) {
    this.pending++;
    this.analysing.set(false);
    this.source.set(source);
    this.preview.set(null);
    this.sourceError.set(null);
    this.edited.emit();
  }

  protected async chooseFile() {
    const picked = await api.pickSpecFile();
    if (!picked) return;
    this.edit(picked);
    await this.analyse();
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
}
