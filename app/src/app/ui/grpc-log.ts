import { ChangeDetectionStrategy, Component, ElementRef, computed, effect, inject, signal, viewChild } from '@angular/core';

import { Workspace } from '../core/store';
import { GrpcStore } from '../core/grpc-store';
import { GrpcLogEntry, GrpcStatus } from '../core/grpc';
import { sizeLabel, timeLabel } from '../core/websocket';
import { Icon } from './icon';

const STATUS_LABELS: Record<GrpcStatus, string> = {
  idle: 'Pas d\'appel',
  connecting: 'Appel…',
  open: 'En cours',
  done: 'Terminé',
  error: 'Erreur',
};

/** Au-delà, un message long est replié : le dérouler montre le reste. */
const FOLD_AT = 600;

/** Le journal de l'appel gRPC de l'onglet actif : ce qui part, ce qui arrive, ce que dit l'application. */
@Component({
  selector: 'app-grpc-log',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { class: 'pane island', 'aria-label': 'Appel gRPC' },
  styles: `
    .state { display: inline-flex; align-items: center; gap: 6px; padding: 0 8px; height: 22px; border-radius: 11px; font-size: calc(11.5 * var(--px)); background: var(--hover); color: var(--muted); }
    .state i { width: 7px; height: 7px; border-radius: 50%; background: var(--faint); }
    .state.done { background: var(--good-soft); color: var(--good); }
    .state.done i { background: var(--good); }
    .state.open { background: var(--good-soft); color: var(--good); }
    .state.open i { background: var(--good); }
    .state.connecting i { background: var(--accent); animation: pulse 1s infinite; }
    .state.error { background: var(--bad-soft); color: var(--bad); }
    .state.error i { background: var(--bad); }
    @keyframes pulse { 50% { opacity: .3; } }
    .where { align-self: center; line-height: 1; flex: 1; min-width: 0; padding: 0 10px; color: var(--muted); font: var(--code-size) var(--code-font); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .log { flex: 1; min-height: 0; overflow: auto; padding: 4px 0; font: var(--code-size) var(--code-font); }
    .line { display: grid; grid-template-columns: 92px 18px minmax(0, 1fr) auto; gap: 8px; align-items: baseline; padding: 3px 12px; }
    .line:hover { background: var(--hover); }
    .at { color: var(--faint); font-variant-numeric: tabular-nums; }
    .arrow { text-align: center; }
    .in .arrow { color: var(--good); }
    .out .arrow { color: var(--accent); }
    .system { color: var(--muted); font-style: italic; }
    .system.error { color: var(--bad); font-style: normal; }
    .text { overflow-wrap: anywhere; min-width: 0; }
    .payload { white-space: pre-wrap; }
    .text.folded { max-height: calc(var(--code-size) * 6); overflow: hidden; }
    .tag { color: var(--faint); margin-right: 6px; }
    .meta { color: var(--faint); font-variant-numeric: tabular-nums; white-space: nowrap; }
    .more { margin-left: 8px; }
    .compose { display: flex; gap: 8px; padding: 8px 10px; border-top: 1px solid var(--line); }
    .compose input { flex: 1; min-width: 0; height: 32px; padding: 0 10px; border: 1px solid var(--line); border-radius: 6px; background: var(--sunken); font: var(--code-size) var(--code-font); color: inherit; outline: 0; }
    .compose input:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .compose input:disabled { opacity: .5; }
  `,
  template: `
    @if (ws.active(); as tab) {
      <div class="res-bar">
        <span class="state" [class]="'state ' + session().status" role="status"><i></i>{{ label() }}</span>
        <span class="where" [title]="where()">{{ where() }}</span>
        <button class="icon-btn sm" (click)="store.clear()" [disabled]="!session().log.length" title="Effacer le journal" aria-label="Effacer le journal"><app-ic name="trash" [size]="15" /></button>
      </div>
      @if (session().unresolved.length) {
        <div class="banner warn-banner"><app-ic name="alert" [size]="15" /><span><b>Variables non résolues :</b> {{ session().unresolved.join(', ') }}. Elles sont parties telles quelles.</span></div>
      }
      <div #scroller class="log" role="log" aria-live="off" aria-label="Messages de l'appel" (scroll)="onScroll()">
        @for (entry of session().log; track entry.n) {
          <div class="line" [class]="'line ' + entry.direction">
            <span class="at">{{ time(entry.at) }}</span>
            <span class="arrow" aria-hidden="true">{{ entry.direction === 'in' ? '←' : entry.direction === 'out' ? '→' : '•' }}</span>
            @if (entry.direction === 'system') {
              <span class="system" [class.error]="entry.kind === 'error' || (entry.kind === 'status' && entry.size !== 0)">{{ entry.text }}</span>
            } @else {
              <span class="text" [class.folded]="folded(entry)"><span class="payload">{{ shown(entry) }}</span>@if (foldable(entry)) {<a class="link more" (click)="toggle(entry.n)">{{ open().has(entry.n) ? 'Replier' : 'Tout afficher' }}</a>}</span>
              <span class="meta">{{ size(entry.size) }}</span>
            }
          </div>
        } @empty {
          <div class="empty" style="min-height: 160px">
            <span class="empty-ic"><app-ic name="send" [size]="18" /></span>
            <h2>{{ session().status === 'idle' ? 'Pas encore d\'appel' : 'Rien à afficher' }}</h2>
            <p>{{ session().status === 'idle' ? 'Appelle la méthode pour voir passer les messages.' : 'Les messages envoyés et reçus apparaissent ici.' }}</p>
          </div>
        }
      </div>
      @if (store.canSend()) {
        <form class="compose" (submit)="submit($event)">
          <input type="text" spellcheck="false" autocomplete="off" [value]="text()" (input)="text.set($any($event.target).value)" placeholder="Message JSON à envoyer (variables {{ '{{…}}' }} résolues)" aria-label="Message JSON à envoyer" />
          <button class="btn" type="submit" [disabled]="!text()">Envoyer</button>
          <button class="btn ghost" type="button" (click)="store.finish()" title="Annonce au serveur qu'aucun autre message ne partira">Terminer l'envoi</button>
        </form>
      }
    }
  `,
})
export class GrpcLog {
  protected readonly ws = inject(Workspace);
  protected readonly store = inject(GrpcStore);
  protected readonly session = this.store.active;
  protected readonly text = signal('');
  protected readonly open = signal<ReadonlySet<number>>(new Set());
  private readonly scroller = viewChild<ElementRef<HTMLElement>>('scroller');
  private following = true;

  protected readonly label = computed(() => STATUS_LABELS[this.session().status]);
  protected readonly where = computed(() => {
    const info = this.session().info;
    return info ? `${info.url} · ${info.method.fullName}` : (this.ws.active()?.doc.url ?? '');
  });

  constructor() {
    // Le journal suit les nouveaux messages tant qu'on n'a pas remonté pour lire.
    effect(() => {
      this.session().log.length;
      const el = this.scroller()?.nativeElement;
      if (el && this.following) queueMicrotask(() => (el.scrollTop = el.scrollHeight));
    });
  }

  protected time = timeLabel;
  protected size = sizeLabel;

  protected onScroll() {
    const el = this.scroller()?.nativeElement;
    if (el) this.following = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  protected foldable(entry: GrpcLogEntry) {
    return entry.text.length > FOLD_AT;
  }

  protected folded(entry: GrpcLogEntry) {
    return this.foldable(entry) && !this.open().has(entry.n);
  }

  protected shown(entry: GrpcLogEntry) {
    return this.folded(entry) ? `${entry.text.slice(0, FOLD_AT)}…` : entry.text;
  }

  protected toggle(n: number) {
    this.open.update((set) => {
      const next = new Set(set);
      if (!next.delete(n)) next.add(n);
      return next;
    });
  }

  protected submit(event: Event) {
    event.preventDefault();
    const text = this.text();
    if (!text) return;
    this.text.set('');
    void this.store.send(text);
  }
}
