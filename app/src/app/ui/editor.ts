import { ChangeDetectionStrategy, Component, ElementRef, computed, inject, signal, viewChild } from '@angular/core';

import { COMMANDS } from '../core/commands';
import { Workspace } from '../core/store';
import { TreeStore } from '../core/tree-store';
import { Icon } from './icon';
import { badgeOf, methodClass, shortMethod } from './method';
import { RequestPane } from './request-pane';
import { ResponsePane } from './response-pane';
import { UrlBar } from './url-bar';
import { WsLog } from './ws-log';

@Component({
  selector: 'app-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, UrlBar, RequestPane, ResponsePane, WsLog],
  host: { style: 'display: contents' },
  template: `
    @if (ws.active(); as tab) {
      <div class="island editor-head">
      <div class="tabs" role="tablist" aria-label="Requêtes ouvertes">
        @for (t of ws.tabs(); track t.path) {
          <div
            class="tab"
            role="tab"
            tabindex="0"
            [class.preview]="t.preview"
            [class.is-dirty]="ws.isDirty(t)"
            [class.is-missing]="t.missing"
            [class.is-stale]="t.stale"
            [attr.aria-selected]="t.path === tab.path"
            [title]="t.missing ? 'Fichier introuvable : le brouillon reste ouvert' : t.path"
            (click)="ws.activate(t.path)"
            (dblclick)="ws.pin(t.path)"
            (keydown.enter)="ws.activate(t.path)"
            (auxclick)="$event.button === 1 && ws.closeTab(t.path)"
          >
            <span [class]="methodClass(badge(t.doc))" style="width: auto">{{ shortMethod(badge(t.doc)) }}</span>
            <span class="tab-name">{{ t.doc.name || t.path }}</span>
            <button class="tab-x" (click)="close($event, t.path)" [attr.aria-label]="'Fermer ' + t.doc.name"><span class="dirty"></span><span class="x"><app-ic name="x" [size]="13" /></span></button>
          </div>
        }
      </div>
      <div class="crumbs">
        <app-ic name="file" [size]="13" />
        @for (p of crumbs(); track $index; let last = $last) {
          @if (last) {
            <b>{{ p }}</b>
          } @else {
            <span>{{ p }}</span><app-ic name="chev-right" [size]="12" />
          }
        }
        <span class="crumbs-state" [class.dirty]="ws.isDirty(tab)" [class.stale]="tab.stale && !tab.missing">
          @if (tab.missing) {
            <app-ic name="alert" [size]="12" />Fichier introuvable sur le disque
          } @else if (tab.stale) {
            <span class="stale-text" title="Le fichier a changé sur le disque : ton brouillon est gardé, et l'enregistrer demandera confirmation."><app-ic name="alert" [size]="12" />Modifié sur le disque</span>
            <button class="btn ghost sm" (click)="ws.reloadFromDisk(tab.path)">Recharger</button>
          } @else if (ws.isDirty(tab)) {
            Non enregistré <kbd class="kbd">{{ label('request.save') }}</kbd>
          } @else {
            <app-ic name="check" [size]="12" />Enregistré sur le disque
          }
        </span>
        <button class="icon-btn sm crumbs-code" (click)="ws.dialog.set('code')" title="Générer du code" aria-label="Générer du code"><app-ic name="code" [size]="14" /></button>
      </div>
      <app-url-bar />
      </div>
      <div #split class="split" [class.vertical]="ws.stacked()" [style.--req.%]="reqPct()" [style.--reqh.%]="reqPct()">
        <app-request-pane />
        <div class="split-handle" role="separator" aria-label="Redimensionner requête et réponse" (pointerdown)="drag($event)"></div>
        @if (tab.doc.requestType === 'websocket') {
          <app-ws-log />
        } @else {
          <app-response-pane />
        }
      </div>
    } @else {
      <div class="island fill">
      <div class="empty">
        <span class="empty-ic"><app-ic name="layers" [size]="20" /></span>
        <h2>Aucune requête ouverte</h2>
        <p>Choisis une requête dans la collection, ou crée-en une.</p>
        <div class="actions">
          <button class="btn" (click)="tree.beginCreate('request')"><app-ic name="plus" [size]="14" />Nouvelle requête</button>
          <button class="btn" (click)="ws.dialog.set('curl')"><app-ic name="terminal" [size]="14" />Nouvelle requête depuis cURL…</button>
          <button class="btn" (click)="ws.dialog.set('openapi')"><app-ic name="import" [size]="14" />Importer une spec OpenAPI…</button>
        </div>
        <div class="keys">
          <span>Rechercher une requête</span><kbd class="kbd">{{ label('palette.search') }}</kbd>
          <span>Envoyer</span><kbd class="kbd">{{ label('request.send') }}</kbd>
          <span>Enregistrer</span><kbd class="kbd">{{ label('request.save') }}</kbd>
          <span>Masquer la barre latérale</span><kbd class="kbd">{{ label('view.sidebar') }}</kbd>
        </div>
      </div>
      </div>
    }
  `,
  styles: `
    .actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; margin-top: 4px; }
    .stale-text { display: inline-flex; align-items: center; gap: 6px; }
  `,
})
export class Editor {
  protected readonly ws = inject(Workspace);
  protected readonly tree = inject(TreeStore);
  private readonly commands = inject(COMMANDS);
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;
  protected readonly badge = (doc: { requestType: string; method: string }) => badgeOf(doc.requestType, doc.method);
  protected readonly reqPct = signal(46);
  private readonly split = viewChild<ElementRef<HTMLElement>>('split');

  protected readonly crumbs = computed(() => {
    const c = this.ws.collection();
    const tab = this.ws.active();
    if (!c || !tab) return [];
    const root = c.root.split(/[\\/]/).filter(Boolean).pop() ?? c.name;
    return [root, ...tab.path.split('/')];
  });

  protected label(id: string) {
    return this.commands.label(id);
  }

  protected close(event: Event, path: string) {
    event.stopPropagation();
    this.ws.closeTab(path);
  }

  protected drag(event: PointerEvent) {
    const handle = event.target as HTMLElement;
    const box = this.split()?.nativeElement.getBoundingClientRect();
    if (!box) return;
    handle.setPointerCapture(event.pointerId);
    handle.classList.add('dragging');
    const move = (e: PointerEvent) => {
      const pct = this.ws.stacked() ? ((e.clientY - box.top) / box.height) * 100 : ((e.clientX - box.left) / box.width) * 100;
      this.reqPct.set(Math.min(75, Math.max(22, pct)));
    };
    const up = () => {
      handle.classList.remove('dragging');
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', up);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up);
  }
}
