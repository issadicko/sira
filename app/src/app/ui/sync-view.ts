import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { Workspace } from '../core/store';
import { reportSummary, sourceName } from '../core/sync';
import { SyncStore } from '../core/sync-store';
import { Icon } from './icon';
import { MergeEditor } from './merge-editor';
import { SyncConnect } from './sync-connect';

/** Éditeur de la vue « Synchro OpenAPI » : connexion, comparaison, fusion ou bilan de la dernière synchro. */
@Component({
  selector: 'app-sync-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, MergeEditor, SyncConnect],
  host: { style: 'display: contents' },
  template: `
    @if (sync.showConnect()) {
      <app-sync-connect />
    } @else if (sync.plan()) {
      <app-merge-editor />
    } @else if (sync.synced(); as done) {
      <div class="tabs"><div class="tab" role="tab" aria-selected="true"><app-ic name="merge" [size]="14" /><span class="tab-name">Fusion · {{ name(done.source) }}</span></div></div>
      <div class="empty">
        <span class="empty-ic" style="background: var(--good-soft); color: var(--good)"><app-ic name="check" [size]="20" /></span>
        <h2>Spec synchronisée</h2>
        @if (ws.demo) {
          <p>{{ summary(done.report) }}. Mode démo : rien n'a été écrit sur ton disque, la base <span class="mono nowrap">.oc-sync</span> comprise.</p>
        } @else {
          <p>{{ summary(done.report) }} ; la base <span class="mono nowrap">.oc-sync</span> a été réécrite en dernier.</p>
        }
        <div class="actions">
          <button class="btn" (click)="ws.view.set('collections')"><app-ic name="layers" [size]="14" />Retour aux requêtes</button>
          <button class="btn" (click)="sync.relaunch()"><app-ic name="sync" [size]="14" />Relancer la comparaison</button>
        </div>
      </div>
    } @else if (sync.uncompared()) {
      <div class="tabs"><div class="tab" role="tab" aria-selected="true"><app-ic name="merge" [size]="14" /><span class="tab-name">Synchro OpenAPI</span></div></div>
      <div class="empty">
        <span class="empty-ic"><app-ic name="merge" [size]="20" /></span>
        <h2>Connectée, pas encore comparée</h2>
        <p>Cette collection suit <span class="mono nowrap">{{ sync.source() }}</span>. Lance la comparaison pour voir ce que la spec a changé depuis la dernière synchro.</p>
        @if (sync.remote()) {
          <p class="faint">La comparaison télécharge cette URL.</p>
        }
        <div class="actions"><button class="btn-primary" (click)="sync.relaunch()"><app-ic name="merge" [size]="15" />Comparer avec la spec</button></div>
      </div>
    } @else if (sync.error(); as e) {
      <div class="tabs"><div class="tab" role="tab" aria-selected="true"><app-ic name="merge" [size]="14" /><span class="tab-name">Synchro OpenAPI</span></div></div>
      <div class="empty">
        <span class="empty-ic" style="background: var(--bad-soft); color: var(--bad)"><app-ic name="alert" [size]="20" /></span>
        <h2>Comparaison impossible</h2>
        <p role="alert">{{ e }}</p>
        <div class="actions"><button class="btn" (click)="sync.relaunch()"><app-ic name="sync" [size]="14" />Réessayer</button></div>
      </div>
    } @else {
      <div class="tabs"><div class="tab" role="tab" aria-selected="true"><app-ic name="merge" [size]="14" /><span class="tab-name">Synchro OpenAPI</span></div></div>
      <div class="empty"><span class="spinner"></span><p>{{ sync.comparing() ? 'Comparaison avec la spec…' : 'Lecture de la connexion…' }}</p></div>
    }
  `,
  styles: `
    .nowrap { white-space: nowrap; }
    .actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 8px; margin-top: 8px; }
  `,
})
export class SyncView {
  protected readonly sync = inject(SyncStore);
  protected readonly ws = inject(Workspace);
  protected readonly summary = reportSummary;
  protected readonly name = sourceName;
}
