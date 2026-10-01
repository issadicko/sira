import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { formatSize, hasLongLine, jsonPath } from '../core/highlight';
import { Timings } from '../core/model';
import { Workspace } from '../core/store';
import { CodeEditor, CodeLanguage } from './code-editor';
import { Icon } from './icon';

type Section = 'body' | 'headers' | 'timeline' | 'tests';
const FORMATS: Record<CodeLanguage, string> = { json: 'JSON', xml: 'XML', yaml: 'YAML', text: 'Texte' };

@Component({
  selector: 'app-response-pane',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, CodeEditor],
  host: { class: 'pane', 'aria-label': 'Réponse' },
  template: `
    @if (ws.active(); as tab) {
      <div class="res-bar">
        <div class="subtabs" role="tablist" aria-label="Sections de la réponse">
          <button class="subtab" role="tab" [attr.aria-selected]="section() === 'body'" (click)="section.set('body')">Corps</button>
          <button class="subtab" role="tab" [attr.aria-selected]="section() === 'headers'" (click)="section.set('headers')">En-têtes @if (tab.result) {<span class="n">{{ tab.result.response.headers.length }}</span>}</button>
          <button class="subtab" role="tab" [attr.aria-selected]="section() === 'timeline'" (click)="section.set('timeline')">Timeline</button>
          <button class="subtab" role="tab" [attr.aria-selected]="section() === 'tests'" (click)="section.set('tests')">
            Tests
            @if (tab.result?.assertions?.length) {
              <span class="n"><span [class]="passed() === tab.result!.assertions.length ? 'ok' : 'ko'">{{ passed() }}/{{ tab.result!.assertions.length }}</span></span>
            }
          </button>
        </div>
        @if (tab.result; as r) {
          <div class="res-meta">
            <span class="status" [class]="'status s-' + String(r.response.status)[0]">{{ r.response.status }}<span class="status-text"> {{ r.response.reason }}</span></span>
            <button class="metric time" (click)="section.set('timeline')" [attr.aria-label]="'Durée ' + ms(r.response.timings.totalMs) + ' ms'"><app-ic name="clock" [size]="12" />{{ ms(r.response.timings.totalMs) }} ms</button>
            <span class="metric size">{{ size() }}</span>
          </div>
        }
      </div>
      @if (tab.result && section() === 'body') {
        <div class="res-tools">
          <div class="seg" role="group" aria-label="Format">
            <button [attr.aria-pressed]="!raw()" (click)="raw.set(false)">{{ formats[language()] }}</button>
            <button [attr.aria-pressed]="raw()" (click)="raw.set(true)">Brut</button>
          </div>
          @if (isJson()) {
            <label class="jp" [class.has-error]="filtered()?.ok === false"><app-ic name="filter" [size]="13" /><input type="text" spellcheck="false" placeholder="$.client.nom" [value]="filter()" (input)="filter.set($any($event.target).value)" aria-label="Filtre JSONPath" /></label>
          }
          <span class="grow"></span>
          <button class="icon-btn sm" (click)="wrap.set(!wrap())" [attr.aria-pressed]="wrap()" [disabled]="longLine()" [title]="longLine() ? 'Ligne trop longue pour le retour à la ligne' : 'Retour à la ligne'" aria-label="Retour à la ligne"><app-ic name="wrap" [size]="15" /></button>
          <button class="icon-btn sm" (click)="copy()" title="Copier le corps" aria-label="Copier le corps"><app-ic name="copy" [size]="15" /></button>
        </div>
      }
      <div class="res-body" [class.is-sending]="!!tab.sendingId">
        @if (tab.sendingId) {
          <div class="progress"></div>
        }
        @if (tab.result; as r) {
          @if (r.unresolved.length) {
            <div class="banner warn-banner"><app-ic name="alert" [size]="15" /><span><b>Variables non résolues :</b> {{ r.unresolved.join(', ') }}. Elles sont parties telles quelles.</span></div>
          }
          @switch (section()) {
            @case ('body') {
              @if (filtered()?.ok === false) {
                <div class="empty" style="min-height: 160px"><h2>{{ $any(filtered()).error }}</h2><p>Exemples : <span class="mono">$.client</span>, <span class="mono">$.historique[0].statut</span></p></div>
              } @else {
                <app-code-editor class="res-code" [value]="text()" [language]="language()" [readonly]="true" [wrap]="(wrap() || raw()) && !longLine()" label="Corps de la réponse" />
              }
            }
            @case ('headers') {
              <div class="pane-body">
                <div class="kv cols-ro">
                  @for (h of r.response.headers; track $index) {
                    <div class="kv-row"><span class="kv-cell m-cell" style="color: var(--muted)">{{ h[0] }}</span><span class="kv-cell m-cell"><span class="clip-text">{{ h[1] }}</span></span></div>
                  }
                </div>
              </div>
            }
            @case ('timeline') {
              <div class="pane-body">
                <div class="sec-head"><span class="sec-title">Timeline réseau</span><span class="sec-meta">mesurée par le moteur Rust</span></div>
                <div class="timeline" style="margin-top: 6px">
                  @for (p of phases(r.response.timings); track p.label) {
                    <span class="tl-label" [class.faint]="!p.ms">{{ p.label }}</span>
                    <span class="tl-track">@if (p.ms) {<span class="tl-bar" [class]="'tl-bar ph-' + p.key" [style.left.%]="p.left" [style.width.%]="p.width"></span>}</span>
                    <span class="tl-ms" [class.faint]="!p.ms">{{ p.ms ? ms(p.ms) + ' ms' : '—' }}</span>
                  }
                  <span class="tl-label tl-total">Total</span><span class="tl-total"></span><span class="tl-ms tl-total">{{ ms(r.response.timings.totalMs) }} ms</span>
                </div>
                <dl class="dl">
                  <dt>URL envoyée</dt><dd>{{ r.method }} {{ r.url }}</dd>
                  <dt>Protocole</dt><dd>{{ r.response.httpVersion }}</dd>
                  <dt>Adresse distante</dt><dd>{{ r.response.remoteAddr }}</dd>
                  <dt>Reçu</dt><dd>{{ size() }}</dd>
                </dl>
              </div>
            }
            @case ('tests') {
              <div class="pane-body">
                @if (r.assertions.length) {
                  <div class="tests-sum">
                    <span class="badge-ic" [class.ko]="passed() !== r.assertions.length"><app-ic [name]="passed() === r.assertions.length ? 'check' : 'x'" [size]="15" /></span>
                    <span style="font-weight: 600">{{ passed() }} sur {{ r.assertions.length }} {{ passed() > 1 ? 'passent' : 'passe' }}</span>
                    <span class="muted">{{ passed() === r.assertions.length ? 'Tout est vert, beau travail.' : 'La valeur reçue est à droite de chaque échec.' }}</span>
                  </div>
                  @for (a of r.assertions; track $index) {
                    <div class="test-row">
                      <span [class]="a.passed ? 'ok-ic' : 'ko-ic'"><app-ic [name]="a.passed ? 'check-circle' : 'x-circle'" [size]="15" /></span>
                      <span class="mono">{{ a.expression }} {{ a.operator }} {{ a.expected ?? '' }}</span>
                      <span class="src">{{ a.error ?? (a.passed ? '' : 'reçu ' + a.actual) }}</span>
                    </div>
                  }
                } @else {
                  <div class="empty"><h2>Aucune assertion</h2><p>Ajoute des assertions dans l'onglet Tests de la requête : elles sont évaluées ici et par la CLI.</p></div>
                }
              </div>
            }
          }
        } @else if (tab.error) {
          <div class="empty">
            <span class="empty-ic"><app-ic name="x-circle" [size]="18" /></span>
            <h2>{{ tab.error.startsWith('Requête annulée') ? 'Requête annulée' : 'La requête a échoué' }}</h2>
            <p class="mono" style="font-size: 12px">{{ tab.error }}</p>
            <button class="btn" style="margin-top: 6px" (click)="ws.send()"><app-ic name="sync" [size]="14" />Renvoyer</button>
          </div>
        } @else if (tab.sendingId) {
          <div class="empty"><span class="spinner"></span><h2>Envoi en cours…</h2><p>Échap pour annuler.</p></div>
        } @else {
          <div class="empty">
            <span class="empty-ic"><app-ic name="send" [size]="18" /></span>
            <h2>Pas encore de réponse</h2>
            <p>Envoie la requête pour voir le corps, les en-têtes, la timeline réseau et les tests.</p>
            <button class="btn-primary" style="margin-top: 6px" (click)="ws.send()"><app-ic name="send" [size]="14" />Envoyer</button>
          </div>
        }
      </div>
    }
  `,
  styles: `
    .res-body { display: flex; flex-direction: column; }
    .res-code { flex: 1; }
    .res-code:focus-within { box-shadow: inset 0 0 0 1px var(--accent-line), inset 0 0 0 4px var(--accent-soft); }
    .is-sending > .res-code { opacity: 0.35; transition: opacity 0.2s; }
    .warn-banner { margin: 10px 12px 0; }
    .ok-ic { color: var(--good); display: grid; }
    .ko-ic { color: var(--bad); display: grid; }
  `,
})
export class ResponsePane {
  protected readonly ws = inject(Workspace);
  protected readonly String = String;
  protected readonly formats = FORMATS;
  protected readonly section = signal<Section>('body');
  protected readonly raw = signal(false);
  protected readonly wrap = signal(false);
  protected readonly filter = signal('');

  private readonly response = computed(() => this.ws.active()?.result?.response ?? null);
  protected readonly passed = computed(() => this.ws.active()?.result?.assertions.filter((a) => a.passed).length ?? 0);
  protected readonly size = computed(() => formatSize(this.response()?.size ?? 0));
  protected readonly isJson = computed(() => this.response()?.pretty != null);
  protected readonly language = computed<CodeLanguage>(() => {
    const type = this.response()?.headers.find(([name]) => name.toLowerCase() === 'content-type')?.[1] ?? '';
    return this.isJson() ? 'json' : /xml|html/i.test(type) ? 'xml' : 'text';
  });
  private readonly filtering = computed(() => this.isJson() && !['', '$'].includes(this.filter().trim()));
  private readonly parsed = computed(() => (this.filtering() ? (JSON.parse(this.response()!.body) as unknown) : undefined));
  protected readonly filtered = computed(() => (this.filtering() ? jsonPath(this.parsed(), this.filter()) : null));
  protected readonly text = computed(() => {
    const r = this.response();
    const f = this.filtered();
    if (!r) return '';
    if (this.raw()) return r.body;
    if (f?.ok) return JSON.stringify(f.value, null, 2) ?? 'undefined';
    return r.pretty ?? r.body;
  });
  protected readonly longLine = computed(() => hasLongLine(this.text()));

  protected ms(v: number) {
    return v < 10 ? v.toFixed(1).replace('.', ',') : Math.round(v).toString();
  }

  protected phases(t: Timings) {
    const rows = [
      { label: 'DNS', ms: t.dnsMs, key: 'dns' },
      { label: 'Connexion TCP', ms: t.tcpMs, key: 'tcp' },
      { label: 'Négociation TLS', ms: t.tlsMs, key: 'tls' },
      { label: 'Attente (TTFB)', ms: t.ttfbMs, key: 'wait' },
      { label: 'Téléchargement', ms: t.downloadMs, key: 'dl' },
    ];
    const total = Math.max(t.totalMs, 0.001);
    let offset = 0;
    return rows.map((r) => {
      const row = { ...r, left: (offset / total) * 100, width: Math.max((r.ms / total) * 100, 0.4) };
      offset += r.ms;
      return row;
    });
  }

  protected copy() {
    const body = this.response()?.body ?? '';
    navigator.clipboard?.writeText(body).then(
      () => this.ws.notify('Corps de la réponse copié'),
      () => this.ws.notify('Copie impossible'),
    );
  }
}
