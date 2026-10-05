import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

import { AssertionResult, PhaseReport, ScriptsReport } from '../core/model';
import { expectation, formatLog, hasLogs, tally } from '../core/scripts';
import { Icon } from './icon';

interface Group {
  label: string;
  phase: PhaseReport;
}

/** Ce que les scripts et les assertions ont dit d'un envoi : résumé, résultats par phase, console. */
@Component({
  selector: 'app-script-report',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    @if (count().total) {
      <div class="tests-sum">
        <span class="badge-ic" [class.ko]="count().passed !== count().total"><app-ic [name]="count().passed === count().total ? 'check' : 'x'" [size]="15" /></span>
        <span style="font-weight: 600">{{ count().passed }} sur {{ count().total }} {{ count().passed > 1 ? 'passent' : 'passe' }}</span>
        <span class="muted">{{ count().passed === count().total ? 'Tout est vert, beau travail.' : 'La cause de chaque échec est à droite.' }}</span>
      </div>
    }
    @if (assertions().length) {
      <section class="grp">
        <div class="sec-head"><span class="sec-title">Assertions</span></div>
        @for (a of assertions(); track $index) {
          <div class="test-row">
            <span [class]="a.passed ? 'ok-ic' : 'ko-ic'"><app-ic [name]="a.passed ? 'check-circle' : 'x-circle'" [size]="15" /></span>
            <span class="mono">{{ a.expression }} {{ a.operator }} {{ a.expected ?? '' }}</span>
            <span class="src">{{ a.error ?? (a.passed ? '' : 'reçu ' + a.actual) }}</span>
          </div>
        }
      </section>
    }
    @for (g of groups(); track g.label) {
      <section class="grp">
        <div class="sec-head"><span class="sec-title">{{ g.label }}</span></div>
        @for (r of g.phase.results; track $index) {
          <div class="test-row">
            <span [class]="r.status === 'pass' ? 'ok-ic' : 'ko-ic'"><app-ic [name]="r.status === 'pass' ? 'check-circle' : 'x-circle'" [size]="15" /></span>
            <span>{{ r.description }}</span>
            <span class="src" [title]="r.error ?? ''">{{ r.error ?? '' }}</span>
          </div>
          @if (expectation(r); as e) {
            <div class="test-detail mono">{{ e }}</div>
          }
        }
        @if (g.phase.error; as e) {
          <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span><b>Le script s'est arrêté :</b> <span class="mono">{{ e }}</span></span></div>
        }
      </section>
    }
    @if (logged()) {
      <section class="grp">
        <div class="sec-head"><span class="sec-title">Console</span><span class="sec-meta">console.log des scripts</span></div>
        <div class="console">
          @for (g of all(); track g.label) {
            @for (l of g.phase.logs; track $index) {
              <div class="log" [class]="'log l-' + l.level"><span class="lv">{{ l.level }}</span><span class="tx">{{ format(l) }}</span><span class="from">{{ g.label }}</span></div>
            }
          }
        </div>
      </section>
    }
    @if (!count().total && !groupsWithError() && !logged() && !assertions().length) {
      <div class="empty">
        <h2>Aucun test</h2>
        <p>Ajoute des assertions ou un script « Tests » à la requête : ils sont évalués après chaque réponse, et par la CLI.</p>
      </div>
    }
  `,
  styles: `
    :host { display: block; }
    .grp { margin-bottom: 16px; }
    .ok-ic { color: var(--good); display: grid; }
    .ko-ic { color: var(--bad); display: grid; }
    .test-row > span:nth-child(2) { min-width: 0; overflow-wrap: anywhere; }
    .test-row .src { max-width: 55%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .test-detail { margin: -2px 0 6px 25px; font-size: calc(11.5 * var(--px)); color: var(--faint); overflow-wrap: anywhere; }
    .banner { margin-top: 8px; }
    .console { border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); padding: 4px 0; overflow: hidden; }
    .log { display: grid; grid-template-columns: 44px minmax(0, 1fr) auto; gap: 8px; padding: 3px 10px; font: calc(12 * var(--px))/1.5 var(--font-mono); }
    .log .lv { color: var(--faint); }
    .log .tx { overflow-wrap: anywhere; white-space: pre-wrap; }
    .log .from { color: var(--faint); font-size: calc(11 * var(--px)); }
    .log.l-error .lv, .log.l-error .tx { color: var(--bad); }
    .log.l-warn .lv, .log.l-warn .tx { color: var(--warn); }
  `,
})
export class ScriptReportView {
  readonly report = input.required<ScriptsReport>();
  readonly assertions = input<AssertionResult[]>([]);

  protected readonly all = computed<Group[]>(() => [
    { label: 'Script pré-requête', phase: this.report().pre },
    { label: 'Script post-réponse', phase: this.report().post },
    { label: 'Tests', phase: this.report().tests },
  ]);
  protected readonly groups = computed(() => this.all().filter((g) => g.phase.results.length || g.phase.error));
  protected readonly groupsWithError = computed(() => this.groups().length > 0);
  protected readonly count = computed(() => tally(this.report(), this.assertions()));
  protected readonly logged = computed(() => hasLogs(this.report()));
  protected readonly expectation = expectation;
  protected readonly format = formatLog;
}
