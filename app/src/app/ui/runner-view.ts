import { DecimalPipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';

import { COMMANDS } from '../core/commands';
import { ReportFormat, RunResult } from '../core/model';
import { RunnerStore } from '../core/runner-store';
import { SKIP_LABELS, duration, firstProblem, fraction, haltText, rowLabel } from '../core/runner';
import { Icon } from './icon';
import { Menu, MenuEntry } from './menu';
import { methodClass, shortMethod } from './method';
import { ScriptReportView } from './script-report';

/** Le runner : le run en direct ou le dernier, requête par requête, avec le détail de chacune et l'export du rapport. */
@Component({
  selector: 'app-runner-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DecimalPipe, Icon, Menu, ScriptReportView],
  host: { style: 'display: contents' },
  styles: `
    .head-spacer { flex: 1; }
    .view-title { flex-shrink: 0; }
    .rv-body { flex: 1; min-height: 0; overflow: auto; padding: 0 16px 28px; }
    .rv-sum { position: sticky; top: 0; z-index: 1; background: var(--island); padding: 14px 0 10px; display: flex; flex-wrap: wrap; align-items: center; gap: 6px 14px; }
    .rv-verdict { display: inline-flex; align-items: center; gap: 8px; font-weight: 600; font-size: calc(13.5 * var(--px)); }
    .rv-verdict .ic { flex-shrink: 0; }
    .rv-verdict.pass { color: var(--good); }
    .rv-verdict.fail { color: var(--bad); }
    .rv-counts { display: inline-flex; flex-wrap: wrap; gap: 2px 14px; color: var(--muted); font-size: calc(12.5 * var(--px)); font-variant-numeric: tabular-nums; }
    .rv-counts b { color: var(--ink); font-weight: 600; }
    .rv-counts .bad b { color: var(--bad); }
    .rv-spacer { flex: 1; }
    .rv-bar { position: absolute; left: 0; right: 0; bottom: 0; height: 2px; background: var(--line); }
    .rv-bar::after { content: ""; position: absolute; inset: 0 auto 0 0; width: calc(var(--p) * 100%); background: var(--accent); transition: width .25s var(--ease); }
    .rv-note { margin: 4px 0 10px; }
    .rv-iter { margin: 18px 0 6px; font-size: calc(12 * var(--px)); font-weight: 600; color: var(--muted); display: flex; gap: 8px; align-items: baseline; }
    .rv-iter .mono { font-weight: 400; color: var(--faint); overflow-wrap: anywhere; }
    .rr { border-top: 1px solid var(--line); }
    .rr:last-child { border-bottom: 1px solid var(--line); }
    .rr-head { width: 100%; min-height: 38px; display: flex; align-items: center; gap: 10px; padding: 0 6px 0 2px; text-align: left; border-radius: 6px; }
    .rr-head:hover { background: var(--hover); }
    .rr-mark { width: 16px; display: grid; place-items: center; flex-shrink: 0; }
    .rr-mark.pass { color: var(--good); }
    .rr-mark.fail, .rr-mark.error { color: var(--bad); }
    .rr-mark.skipped i { width: 12px; height: 12px; border: 1.5px dashed var(--faint); border-radius: 50%; }
    .rr-mark.live { color: var(--accent); }
    .rr .m { flex-shrink: 0; min-width: 34px; }
    .rr-main { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 10px; overflow: hidden; }
    .rr-name { font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .rr-path { color: var(--faint); font-size: calc(11.5 * var(--px)); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
    .rr.skipped .rr-name { color: var(--muted); font-weight: 400; }
    .rr-code { font: 600 calc(11.5 * var(--px)) var(--font-mono); flex-shrink: 0; }
    .rr-code.c-2 { color: var(--good); }
    .rr-code.c-4 { color: var(--warn); }
    .rr-code.c-5 { color: var(--bad); }
    .rr-time { width: 64px; text-align: right; color: var(--faint); font-size: calc(11.5 * var(--px)); font-variant-numeric: tabular-nums; flex-shrink: 0; }
    .rr-chev { color: var(--faint); display: grid; transition: transform .15s var(--ease); }
    .rr.open .rr-chev { transform: rotate(90deg); }
    .rr-problem { margin: -4px 0 8px 28px; padding-right: 8px; color: var(--bad); font: calc(12 * var(--px))/1.5 var(--font-mono); overflow-wrap: anywhere; }
    .rr-body { padding: 4px 4px 18px 28px; }
    .rr-url { margin: 0 0 12px; font: calc(12 * var(--px)) var(--font-mono); color: var(--muted); overflow-wrap: anywhere; }
    .rr-skip { margin: 0 0 10px; color: var(--muted); font-size: calc(12.5 * var(--px)); }
    .rr-live { color: var(--muted); }
    .rr-live .rr-name { font-weight: 400; }
    .rv-empty { padding: 56px 16px; }
    .banner { margin-bottom: 10px; }
    .banner > span { min-width: 0; overflow-wrap: anywhere; }
  `,
  template: `
    <div class="tabs">
      <div class="tab" role="tab" aria-selected="true"><app-ic name="play" [size]="14" /><span class="tab-name">Runner</span></div>
    </div>
    <div class="view-head">
      <h1 class="view-title">Runner</h1>
      @if (store.selected(); as s) {
        <span class="view-sub">{{ s.path ? s.name : 'Toute la collection' }} · {{ s.requests }} {{ s.requests > 1 ? 'requêtes' : 'requête' }}@if (store.data(); as d) { × {{ d.info.rows }} }@if (store.bail()) { · arrêt au premier échec }</span>
      }
      <span class="head-spacer"></span>
      @if (store.done() && !store.running()) {
        <button class="btn" aria-haspopup="menu" [attr.aria-expanded]="menu() !== null" [disabled]="store.exporting() !== null" (click)="toggleMenu($event)">
          @if (store.exporting()) {
            <span class="spinner"></span>
          } @else {
            <app-ic name="download" [size]="14" />
          }
          Exporter<app-ic name="chev-down" [size]="13" />
        </button>
      }
      @if (store.running()) {
        <button class="btn-primary is-sending" [disabled]="store.cancelling()" (click)="store.cancel()">
          <span class="spinner"></span>{{ store.cancelling() ? 'Annulation…' : 'Annuler' }}
        </button>
      } @else {
        <button class="btn-primary" [disabled]="!store.selected().requests" (click)="store.run()">
          <app-ic name="play" [size]="14" />{{ store.started() ? 'Relancer' : 'Lancer' }} <kbd class="kbd">{{ label('runner.run') }}</kbd>
        </button>
      }
    </div>
    @if (menu(); as m) {
      <app-menu [items]="entries" [x]="m.x" [y]="m.y" label="Exporter le rapport" heading="Rapport du dernier run" (closed)="menu.set(null)" />
    }

    <div class="rv-body">
      @if (store.error(); as e) {
        <div class="banner err" role="alert" style="margin-top: 14px"><app-ic name="alert" [size]="15" /><span><b>Le run n'a pas pu démarrer :</b> {{ e }}</span></div>
      }
      @if (!store.started()) {
        <div class="empty rv-empty">
          <span class="empty-ic"><app-ic name="play" [size]="20" /></span>
          <h2>Exécuter {{ store.selected().path ? 'ce dossier' : 'la collection' }}</h2>
          <p>Chaque requête passe par ses scripts, ses assertions et ses tests, dans l'ordre du dossier. Les variables qu'un script pose servent aux requêtes suivantes.</p>
        </div>
      } @else {
        @let p = store.progress();
        @let t = store.tally();
        <section class="rv-sum" aria-live="polite">
          @if (store.running()) {
            <span class="rv-verdict"><span class="spinner"></span>{{ store.cancelling() ? 'Annulation…' : 'En cours' }}</span>
            <span class="rv-counts"><span><b>{{ p.results.length }}</b> sur {{ p.requests * p.iterations }}</span></span>
          } @else if (store.done(); as d) {
            <span class="rv-verdict" [class]="'rv-verdict ' + (d.failed ? 'fail' : 'pass')">
              <app-ic [name]="d.failed ? 'x-circle' : 'check-circle'" [size]="17" />{{ d.failed ? 'Des échecs' : 'Tout est passé' }}
            </span>
          }
          <span class="rv-counts">
            <span><b>{{ t.passed }}</b> {{ t.passed > 1 ? 'réussies' : 'réussie' }}</span>
            @if (t.failed + t.errors) {
              <span class="bad"><b>{{ t.failed + t.errors }}</b> en échec</span>
            }
            @if (t.skipped) {
              <span><b>{{ t.skipped }}</b> {{ t.skipped > 1 ? 'ignorées' : 'ignorée' }}</span>
            }
            @if (t.checks.total) {
              <span><b>{{ t.checks.passed }}</b> sur {{ t.checks.total }} vérifications</span>
            }
            @if (store.done(); as d) {
              <span>{{ time(d.elapsedMs) }}</span>
            }
          </span>
          <span class="rv-spacer"></span>
          @if (store.problems() > 0) {
            <span class="seg" role="group" aria-label="Filtrer les requêtes">
              <button [attr.aria-pressed]="store.filter() === 'all'" (click)="store.filter.set('all')">Toutes ({{ p.results.length }})</button>
              <button [attr.aria-pressed]="store.filter() === 'problems'" (click)="store.filter.set('problems')">Échecs ({{ store.problems() }})</button>
            </span>
          }
          @if (store.running()) {
            <div class="rv-bar" [style.--p]="fraction(p)" role="progressbar" [attr.aria-valuenow]="(fraction(p) * 100) | number: '1.0-0'" aria-valuemin="0" aria-valuemax="100"></div>
          }
        </section>

        @if (store.done()?.halt; as h) {
          <div class="note rv-note"><app-ic name="alert" [size]="14" /><span>{{ halt(h) }}</span></div>
        }
        @for (w of p.warnings; track $index) {
          <div class="note rv-note"><app-ic name="alert" [size]="14" /><span>{{ w }}</span></div>
        }

        @for (g of store.groups(); track g.index) {
          @if (p.iterations > 1) {
            <div class="rv-iter">Itération {{ g.index + 1 }} sur {{ p.iterations }}@if (rowText(g.row); as r) {<span class="mono">{{ r }}</span>}</div>
          }
          @for (r of g.results; track r) {
            @let open = store.expanded().has(r);
            @let problem = problemOf(r);
            <div class="rr" [class.open]="open" [class.skipped]="r.status === 'skipped'">
              <button class="rr-head" [attr.aria-expanded]="open" (click)="store.toggle(r)">
                <span [class]="'rr-mark ' + r.status">
                  @switch (r.status) {
                    @case ('pass') { <app-ic name="check-circle" [size]="16" /> }
                    @case ('fail') { <app-ic name="x-circle" [size]="16" /> }
                    @case ('error') { <app-ic name="alert" [size]="16" /> }
                    @default { <i></i> }
                  }
                </span>
                <span [class]="methodClass(r.method)">{{ shortMethod(r.method) }}</span>
                <span class="rr-main"><span class="rr-name">{{ r.name }}</span><span class="rr-path">{{ r.path }}</span></span>
                @if (r.http; as h) {
                  <span class="rr-code" [class]="'rr-code c-' + String(h.status)[0]">{{ h.status }}</span>
                }
                <span class="rr-time">{{ r.status === 'skipped' ? '' : time(r.durationMs) }}</span>
                <span class="rr-chev"><app-ic name="chev-right" [size]="14" /></span>
              </button>
              @if (problem && !open) {
                <p class="rr-problem">{{ problem }}</p>
              }
              @if (open) {
                <div class="rr-body">
                  @if (r.skipped; as why) {
                    <p class="rr-skip">{{ skipLabel(why) }}</p>
                  }
                  @if (r.status !== 'skipped' || r.url) {
                    <p class="rr-url">{{ r.method }} {{ r.url }}</p>
                  }
                  @if (r.error; as e) {
                    <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span><b>{{ stage(e.stage) }}</b> {{ e.message }}</span></div>
                  }
                  <app-script-report [report]="r.scripts" [assertions]="r.assertions" />
                </div>
              }
            </div>
          }
        }
        @if (store.running() && p.running; as live) {
          <div class="rr rr-live">
            <div class="rr-head" style="cursor: default">
              <span class="rr-mark live"><span class="spinner"></span></span>
              <span [class]="methodClass(live.method)">{{ shortMethod(live.method) }}</span>
              <span class="rr-main"><span class="rr-name">{{ live.name }}</span><span class="rr-path">{{ live.path }}</span></span>
            </div>
          </div>
        } @else if (store.running() && p.waitingMs) {
          <div class="rr rr-live"><div class="rr-head"><span class="rr-mark live"><span class="spinner"></span></span><span class="rr-main"><span class="rr-name">Attente de {{ time(p.waitingMs) }} avant la requête suivante</span></span></div></div>
        }
        @if (!store.running() && store.filter() === 'problems' && !store.shown().length) {
          <p class="pal-empty">Aucune requête en échec.</p>
        }
      }
    </div>
  `,
})
export class RunnerView {
  protected readonly store = inject(RunnerStore);
  private readonly commands = inject(COMMANDS);
  protected readonly menu = signal<{ x: number; y: number } | null>(null);
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;
  protected readonly time = duration;
  protected readonly fraction = fraction;
  protected readonly halt = haltText;
  protected readonly rowText = rowLabel;
  protected readonly String = String;
  protected readonly skipLabel = (why: string) => SKIP_LABELS[why] ?? why;
  protected readonly problemOf = (r: RunResult) => (r.status === 'pass' || r.status === 'skipped' ? null : firstProblem(r));

  protected readonly entries: MenuEntry[] = (
    [
      ['html', 'Page HTML', 'globe'],
      ['junit', 'JUnit (XML)', 'file'],
      ['json', 'JSON', 'code'],
    ] as [ReportFormat, string, string][]
  ).map(([format, label, icon]) => ({ label, icon, run: () => this.store.export(format) }));

  protected stage(stage: string) {
    return stage === 'preRequestScript' ? 'Script pré-requête :' : stage === 'send' ? 'Envoi :' : 'Préparation :';
  }

  protected label(id: string) {
    return this.commands.label(id);
  }

  protected toggleMenu(event: MouseEvent) {
    if (this.menu()) {
      this.menu.set(null);
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.menu.set({ x: Math.max(8, rect.right - 200), y: rect.bottom + 4 });
  }
}
