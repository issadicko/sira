import { ChangeDetectionStrategy, Component, HostListener, inject, signal } from '@angular/core';

import { shortcutLabel } from '../core/commands';
import { OpenApiPreview } from '../core/model';
import { Workspace } from '../core/store';
import { NO_BASE_NOTE } from '../core/sync';
import { SyncStore } from '../core/sync-store';
import { Icon } from './icon';
import { SpecSource } from './spec-source';

/** État vide de la synchro : choix de la spec à laquelle connecter la collection, puis plan sans base. */
@Component({
  selector: 'app-sync-connect',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, SpecSource],
  host: { style: 'display: contents' },
  template: `
    <div class="tabs"><div class="tab" role="tab" aria-selected="true"><app-ic name="merge" [size]="14" /><span class="tab-name">Synchro OpenAPI</span></div></div>
    <div class="connect">
      <div class="connect-body">
        <span class="empty-ic"><app-ic name="merge" [size]="20" /></span>
        <h2>Connecter une spec OpenAPI</h2>
        <p>La synchro compare la collection à une spec et fusionne les deux sans rien écraser. Rien n'est écrit avant ta validation.</p>
        @if (!sync.connected()) {
          <div class="note"><app-ic name="alert" [size]="14" /><span>{{ noBaseNote }}</span></div>
        }
        @if (ws.demo) {
          <div class="note"><app-ic name="alert" [size]="14" /><span>Mode démo : la comparaison est fictive et ne lit pas la source choisie.</span></div>
        }
        @if (sync.status()?.source; as current) {
          <div class="note"><app-ic name="file" [size]="14" /><span>Cette collection suit déjà <span class="mono">{{ current }}</span>. Une autre source la remplace quand tu appliques la synchro.</span></div>
        }
        <app-spec-source [(source)]="source" [(preview)]="preview" />
        @if (sync.error(); as e) {
          <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
        }
        <div class="foot-actions">
          @if (sync.connecting() && sync.connected()) {
            <button class="btn ghost" [disabled]="sync.comparing()" (click)="sync.cancelConnect()">Annuler</button>
          }
          <button class="btn-primary" [class.is-sending]="sync.comparing()" [disabled]="!preview() || sync.comparing()" (click)="compare()">
            @if (sync.comparing()) {
              <span class="spinner"></span>Comparaison…
            } @else {
              <app-ic name="merge" [size]="15" />Comparer avec la collection <kbd class="kbd">{{ confirmKey }}</kbd>
            }
          </button>
        </div>
      </div>
    </div>
  `,
  styles: `
    .connect { flex: 1; min-height: 0; overflow: auto; display: flex; justify-content: center; padding: 32px 24px; }
    .connect-body { width: min(560px, 100%); display: flex; flex-direction: column; gap: 14px; align-items: stretch; height: max-content; }
    .connect-body > .empty-ic { align-self: flex-start; }
    h2 { margin: 0; font-size: calc(20 * var(--px)); font-weight: 600; letter-spacing: -0.01em; }
    p { margin: 0; color: var(--muted); max-width: 60ch; }
    .foot-actions { justify-content: flex-start; margin-top: 4px; }
  `,
})
export class SyncConnect {
  protected readonly sync = inject(SyncStore);
  protected readonly ws = inject(Workspace);
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly noBaseNote = NO_BASE_NOTE;
  protected readonly source = signal('');
  protected readonly preview = signal<OpenApiPreview | null>(null);

  protected compare() {
    const source = this.source().trim();
    if (this.preview() && source && !this.sync.comparing()) void this.sync.connect(source);
  }

  @HostListener('document:keydown', ['$event'])
  protected onKey(event: KeyboardEvent) {
    if (this.ws.modal() || this.ws.palette() !== null || event.isComposing) return;
    if (event.key === 'Escape' && this.sync.connecting() && this.sync.connected()) this.sync.cancelConnect();
    else if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
      event.preventDefault();
      this.compare();
    }
  }
}
