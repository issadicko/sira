import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { Assertion, Auth, Body, KeyValue, Param } from '../core/model';
import { Workspace } from '../core/store';
import { prettyJson } from '../core/highlight';
import { CodeEditor } from './code-editor';
import { Icon } from './icon';
import { KvTable } from './kv-table';

type Section = 'params' | 'body' | 'headers' | 'auth' | 'tests' | 'scripts' | 'docs';
const OPERATORS = ['eq', 'neq', 'gt', 'gte', 'lt', 'lte', 'contains', 'notContains', 'isNumber', 'isString', 'isBoolean', 'isArray', 'isJson', 'isNull', 'isDefined', 'isUndefined', 'isTruthy', 'isFalsy', 'isEmpty'];
const UNARY = new Set(OPERATORS.filter((o) => o.startsWith('is')));
const BODY_TYPES: { type: Body['type']; label: string }[] = [
  { type: 'none', label: 'Aucun' },
  { type: 'json', label: 'JSON' },
  { type: 'text', label: 'Texte' },
  { type: 'xml', label: 'XML' },
];
const AUTH_TYPES: { type: Auth['type']; label: string }[] = [
  { type: 'inherit', label: 'Hériter du parent' },
  { type: 'none', label: 'Aucune' },
  { type: 'bearer', label: 'Bearer Token' },
  { type: 'basic', label: 'Basic' },
  { type: 'apikey', label: 'API Key' },
];

@Component({
  selector: 'app-request-pane',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, KvTable, CodeEditor],
  host: { class: 'pane', 'aria-label': 'Requête' },
  template: `
    @if (ws.active(); as tab) {
      <div class="subtabs" role="tablist" aria-label="Sections de la requête">
        @for (s of sections(); track s.id) {
          <button class="subtab" role="tab" [attr.aria-selected]="section() === s.id" (click)="section.set(s.id)">
            {{ s.label }}
            @if (s.count) {
              <span class="n">{{ s.count }}</span>
            }
          </button>
        }
      </div>
      <div class="pane-body">
        @switch (section()) {
          @case ('params') {
            <section class="sec">
              <div class="sec-head"><span class="sec-title">Paramètres de requête</span><span class="sec-meta">synchronisés avec l'URL</span></div>
              <app-kv-table [rows]="query()" addLabel="Ajouter un paramètre" [descriptions]="true" [template]="{ kind: 'query' }" (rowsChange)="setQuery($event)" />
            </section>
            @if (path().length) {
              <section class="sec">
                <div class="sec-head"><span class="sec-title">Paramètres de chemin</span><span class="sec-meta">déduits des segments :nom de l'URL</span></div>
                <app-kv-table [rows]="path()" [addable]="false" [locked]="lockAll" (rowsChange)="setPath($event)" />
              </section>
            }
            @if (tab.doc.auth.type === 'inherit') {
              <div class="note"><app-ic name="shield" [size]="14" /><span>Auth héritée du dossier ou de la collection.</span><a class="link" style="margin-left: auto" (click)="section.set('auth')">Voir</a></div>
            }
          }
          @case ('body') {
            <div class="seg" role="group" aria-label="Type de corps">
              @for (b of bodyTypes; track b.type) {
                <button [attr.aria-pressed]="tab.doc.body.type === b.type" (click)="setBodyType(b.type)">{{ b.label }}</button>
              }
            </div>
            @if (tab.doc.body.type === 'other') {
              <div class="banner" style="margin-top: 12px"><app-ic name="alert" [size]="15" /><span><b>Corps {{ $any(tab.doc.body).label }}.</b> Ce type n'est pas encore éditable ici ; il est conservé tel quel dans le fichier.</span></div>
            } @else if (tab.doc.body.type !== 'none') {
              <div class="body-tools">
                <span class="faint">Les variables {{ '{{…}}' }} sont résolues à l'envoi.</span>
                @if (tab.doc.body.type === 'json') {
                  <button class="btn ghost" style="height: 24px" (click)="formatBody()">Formater</button>
                }
              </div>
              @for (path of [tab.path]; track path) {
                <app-code-editor class="body-editor" [value]="$any(tab.doc.body).data" [language]="$any(tab.doc.body).type" label="Corps de la requête" (valueChange)="setBodyData($event)" />
              }
            } @else {
              <div class="empty" style="min-height: 200px">
                <span class="empty-ic"><app-ic name="file" [size]="18" /></span>
                <h2>Pas de corps</h2>
                <p>Choisis un type au-dessus pour en ajouter un.</p>
              </div>
            }
          }
          @case ('headers') {
            <section class="sec">
              <div class="sec-head"><span class="sec-title">En-têtes</span><span class="sec-meta">ceux de la collection et des dossiers s'ajoutent à l'envoi</span></div>
              <app-kv-table [rows]="tab.doc.headers" keyLabel="Nom" addLabel="Ajouter un en-tête" (rowsChange)="setHeaders($event)" />
            </section>
          }
          @case ('auth') {
            <section class="sec">
              <div class="sec-head"><span class="sec-title">Type</span></div>
              @if (tab.doc.auth.type === 'other') {
                <div class="banner"><app-ic name="alert" [size]="15" /><span><b>{{ $any(tab.doc.auth).label }}.</b> Ce type arrive en V1 ; il est conservé tel quel dans le fichier.</span></div>
              } @else {
                <div class="seg" role="group" aria-label="Type d'authentification">
                  @for (a of authTypes; track a.type) {
                    <button [attr.aria-pressed]="tab.doc.auth.type === a.type" (click)="setAuthType(a.type)">{{ a.label }}</button>
                  }
                </div>
              }
            </section>
            @switch (tab.doc.auth.type) {
              @case ('bearer') {
                <label class="field"><span>Jeton</span><input type="text" spellcheck="false" [value]="$any(tab.doc.auth).token" (input)="setAuthField('token', $event)" placeholder="{{ '{{token}}' }}" /></label>
              }
              @case ('basic') {
                <label class="field"><span>Utilisateur</span><input type="text" spellcheck="false" [value]="$any(tab.doc.auth).username" (input)="setAuthField('username', $event)" /></label>
                <label class="field"><span>Mot de passe</span><input type="password" [value]="$any(tab.doc.auth).password" (input)="setAuthField('password', $event)" placeholder="{{ '{{process.env.MOT_DE_PASSE}}' }}" /></label>
              }
              @case ('apikey') {
                <label class="field"><span>Clé</span><input type="text" spellcheck="false" [value]="$any(tab.doc.auth).key" (input)="setAuthField('key', $event)" /></label>
                <label class="field"><span>Valeur</span><input type="text" spellcheck="false" [value]="$any(tab.doc.auth).value" (input)="setAuthField('value', $event)" /></label>
              }
              @case ('inherit') {
                <div class="note"><app-ic name="shield" [size]="14" /><span>L'auth du dossier le plus proche, sinon celle de opencollection.yml, est appliquée à l'envoi.</span></div>
              }
            }
            <p class="faint" style="font-size: 12px; margin: 12px 2px 0">Préfère une variable ({{ '{{token}}' }}, {{ '{{process.env.NOM}}' }}) à une valeur en clair : ce fichier est versionné.</p>
          }
          @case ('tests') {
            <section class="sec">
              <div class="sec-head"><span class="sec-title">Assertions</span><span class="sec-meta">évaluées après chaque réponse, et par la CLI</span></div>
              <div class="kv asserts">
                <div class="kv-row head"><span class="kv-cell"></span><span class="kv-cell">Expression</span><span class="kv-cell">Opérateur</span><span class="kv-cell">Valeur attendue</span></div>
                @for (a of tab.doc.assertions; track $index; let i = $index) {
                  <div class="kv-row" [class.off]="!a.enabled">
                    <span class="kv-cell"><input type="checkbox" class="cb" [checked]="a.enabled" (change)="patchAssertion(i, { enabled: !a.enabled })" aria-label="Activer l'assertion" /></span>
                    <span class="kv-cell m-cell"><input type="text" spellcheck="false" [value]="a.expression" (input)="patchAssertion(i, { expression: $any($event.target).value })" aria-label="Expression" /></span>
                    <span class="kv-cell m-cell">
                      <select class="op" [value]="a.operator" (change)="setOperator(i, $any($event.target).value)" aria-label="Opérateur">
                        @for (op of operators; track op) {
                          <option [value]="op">{{ op }}</option>
                        }
                      </select>
                    </span>
                    <span class="kv-cell m-cell">
                      @if (!unary(a.operator)) {
                        <input type="text" spellcheck="false" [value]="a.value ?? ''" (input)="patchAssertion(i, { value: $any($event.target).value })" aria-label="Valeur attendue" />
                      }
                      <button class="icon-btn sm row-x" (click)="removeAssertion(i)" aria-label="Supprimer l'assertion"><app-ic name="x" [size]="13" /></button>
                    </span>
                  </div>
                }
              </div>
              <button class="btn ghost" style="margin-top: 8px" (click)="addAssertion()"><app-ic name="plus" [size]="14" />Ajouter une assertion</button>
            </section>
          }
          @case ('scripts') {
            @if (tab.doc.scripts.length) {
              @for (s of tab.doc.scripts; track $index) {
                <section class="sec">
                  <div class="sec-head"><span class="sec-title">{{ scriptLabel(s.kind) }}</span></div>
                  <pre class="code boxed script">{{ s.code }}</pre>
                </section>
              }
            }
            <div class="note"><app-ic name="shield" [size]="14" /><span>Les scripts sont conservés dans le fichier. Leur exécution (sandbox QuickJS, API bru / req / res) arrive en V1.</span></div>
          }
          @case ('docs') {
            <textarea class="code-input docs" [value]="tab.doc.docs ?? ''" (input)="setDocs($any($event.target).value)" placeholder="Documentation Markdown de la requête" aria-label="Documentation"></textarea>
          }
        }
      </div>
    }
  `,
  styles: `
    .code-input { display: block; width: 100%; min-height: 260px; height: calc(100% - 64px); resize: none; margin-top: 8px; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--line); background: var(--sunken); font: 12.5px/20px var(--font-mono); font-variant-ligatures: none; tab-size: 2; outline: 0; }
    .code-input:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .code-input.docs { font-family: var(--font-ui); font-size: 13px; height: 100%; margin: 0; }
    .body-editor { height: calc(100% - 64px); min-height: 260px; margin-top: 8px; border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); }
    .body-editor:focus-within { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .body-tools { display: flex; align-items: center; justify-content: space-between; margin-top: 10px; font-size: 12px; }
    .field { display: grid; grid-template-columns: 110px minmax(0, 1fr); align-items: center; gap: 10px; margin-bottom: 8px; font-size: 12.5px; color: var(--muted); }
    .field input { height: 30px; padding: 0 10px; border-radius: 6px; border: 1px solid var(--line); background: var(--sunken); font: 12.5px var(--font-mono); outline: 0; }
    .field input:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .asserts .kv-row { grid-template-columns: 32px minmax(0, 1.2fr) 120px minmax(0, 1fr); }
    .op { width: 100%; height: 28px; border: 0; background: transparent; font: 12.5px var(--font-mono); outline: 0; }
    .op option { background: var(--pop); }
    .row-x { opacity: 0; margin-left: auto; }
    .kv-row:hover .row-x { opacity: 1; }
    .script { margin: 0; padding: 10px 12px; white-space: pre-wrap; }
  `,
})
export class RequestPane {
  protected readonly ws = inject(Workspace);
  protected readonly section = signal<Section>('params');
  protected readonly bodyTypes = BODY_TYPES;
  protected readonly authTypes = AUTH_TYPES;
  protected readonly operators = OPERATORS;
  protected readonly lockAll = () => true;

  protected readonly query = computed(() => this.ws.active()?.doc.params.filter((p) => p.kind === 'query') ?? []);
  protected readonly path = computed(() => this.ws.active()?.doc.params.filter((p) => p.kind === 'path') ?? []);
  protected readonly sections = computed(() => {
    const d = this.ws.active()?.doc;
    if (!d) return [];
    return [
      { id: 'params' as const, label: 'Paramètres', count: d.params.filter((p) => p.enabled).length || '' },
      { id: 'body' as const, label: 'Corps', count: d.body.type === 'none' ? '' : d.body.type === 'other' ? 'autre' : d.body.type.toUpperCase() },
      { id: 'headers' as const, label: 'En-têtes', count: d.headers.filter((h) => h.enabled).length || '' },
      { id: 'auth' as const, label: 'Auth', count: d.auth.type === 'inherit' ? 'hérité' : d.auth.type === 'none' ? '' : d.auth.type },
      { id: 'tests' as const, label: 'Tests', count: d.assertions.filter((a) => a.enabled).length || '' },
      { id: 'scripts' as const, label: 'Scripts', count: d.scripts.length || '' },
      { id: 'docs' as const, label: 'Docs', count: '' },
    ];
  });

  protected setQuery(rows: Param[]) {
    this.ws.setParams([...rows.map((r) => ({ ...r, kind: 'query' as const })), ...this.path()]);
  }

  protected setPath(rows: Param[]) {
    this.ws.edit((d) => ({ ...d, params: [...this.query(), ...rows] }));
  }

  protected setHeaders(headers: KeyValue[]) {
    this.ws.edit((d) => ({ ...d, headers }));
  }

  protected setBodyType(type: Body['type']) {
    this.ws.edit((d) => {
      if (type === 'none' || type === 'other') return { ...d, body: { type: 'none' } };
      const data = 'data' in d.body ? d.body.data : type === 'json' ? '{\n  \n}' : '';
      return { ...d, body: { type, data } };
    });
  }

  protected setBodyData(data: string) {
    this.ws.edit((d) => (d.body.type === 'json' || d.body.type === 'text' || d.body.type === 'xml' ? { ...d, body: { type: d.body.type, data } } : d));
  }

  protected formatBody() {
    const body = this.ws.active()?.doc.body;
    if (!body || body.type !== 'json') return;
    const pretty = prettyJson(body.data);
    if (pretty) this.setBodyData(pretty);
    else this.ws.notify('JSON invalide ou contenant des variables non guillemetées : rien à formater.');
  }

  protected setAuthType(type: Auth['type']) {
    const auth: Auth =
      type === 'bearer' ? { type, token: '{{token}}' }
      : type === 'basic' ? { type, username: '', password: '' }
      : type === 'apikey' ? { type, key: 'X-API-Key', value: '', placement: 'header' }
      : type === 'none' ? { type: 'none' }
      : { type: 'inherit' };
    this.ws.edit((d) => ({ ...d, auth }));
  }

  protected setAuthField(field: string, event: Event) {
    const value = (event.target as HTMLInputElement).value;
    this.ws.edit((d) => ({ ...d, auth: { ...d.auth, [field]: value } as Auth }));
  }

  protected unary(op: string) {
    return UNARY.has(op);
  }

  protected patchAssertion(i: number, change: Partial<Assertion>) {
    this.ws.edit((d) => ({ ...d, assertions: d.assertions.map((a, j) => (j === i ? { ...a, ...change } : a)) }));
  }

  protected setOperator(i: number, operator: string) {
    this.patchAssertion(i, UNARY.has(operator) ? { operator, value: null } : { operator });
  }

  protected removeAssertion(i: number) {
    this.ws.edit((d) => ({ ...d, assertions: d.assertions.filter((_, j) => j !== i) }));
  }

  protected addAssertion() {
    this.ws.edit((d) => ({ ...d, assertions: [...d.assertions, { expression: 'res.status', operator: 'eq', value: '200', enabled: true }] }));
  }

  protected setDocs(docs: string) {
    this.ws.edit((d) => ({ ...d, docs: docs || null }));
  }

  protected scriptLabel(kind: string) {
    return { 'before-request': 'Avant la requête', 'after-response': 'Après la réponse', tests: 'Tests' }[kind] ?? kind;
  }
}
