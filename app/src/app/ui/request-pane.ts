import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from '../core/api';
import { switchBody, withFile } from '../core/body';
import { Assertion, Auth, Body, KeyValue, MultipartField, Param, TokenInfo } from '../core/model';
import { relativeToRoot } from '../core/paths';
import { SCRIPT_KINDS, scriptCode, withScript } from '../core/scripts';
import { Workspace } from '../core/store';
import { prettyJson } from '../core/highlight';
import { AuthEditor } from './auth-editor';
import { CodeEditor } from './code-editor';
import { GraphqlEditor } from './graphql-editor';
import { Icon } from './icon';
import { KvTable } from './kv-table';
import { MultipartTable } from './multipart-table';

type Section = 'params' | 'body' | 'headers' | 'auth' | 'tests' | 'scripts' | 'docs';
const OPERATORS = [
  'eq', 'neq', 'gt', 'gte', 'lt', 'lte', 'in', 'notIn', 'contains', 'notContains', 'length', 'matches', 'notMatches', 'startsWith',
  'endsWith', 'between', 'isEmpty', 'isNotEmpty', 'isNull', 'isUndefined', 'isDefined', 'isTruthy', 'isFalsy', 'isJson', 'isNumber',
  'isString', 'isBoolean', 'isArray',
];
const UNARY = new Set(OPERATORS.filter((o) => o.startsWith('is')));
const BODY_TYPES: { type: Body['type']; label: string }[] = [
  { type: 'none', label: 'Aucun' },
  { type: 'json', label: 'JSON' },
  { type: 'text', label: 'Texte' },
  { type: 'xml', label: 'XML' },
  { type: 'form-urlencoded', label: 'Formulaire' },
  { type: 'multipart-form', label: 'Multipart' },
];
const BODY_BADGES: Partial<Record<Body['type'], string>> = { 'form-urlencoded': 'FORM', 'multipart-form': 'MULTIPART', graphql: 'GQL' };
const TEXT_BODIES = new Set<Body['type']>(['json', 'text', 'xml']);

const OUTSIDE_COLLECTION =
  "Ce fichier est hors de la collection : le moteur n'envoie que des fichiers du dossier de la collection. Copie-le dedans, puis choisis-le à nouveau.";

@Component({
  selector: 'app-request-pane',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, KvTable, MultipartTable, CodeEditor, AuthEditor, GraphqlEditor],
  host: { class: 'pane island', 'aria-label': 'Requête' },
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
      @switch (section()) {
        @case ('params') {
          <div class="pane-body">
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
          </div>
        }
        @case ('body') {
          @if (tab.doc.requestType === 'graphql') {
            @defer (on immediate) {
              <app-graphql-editor />
            }
          } @else if (tab.doc.body.type === 'other') {
            <div class="pane-body">
              <div class="banner"><app-ic name="alert" [size]="15" /><span><b>Corps {{ $any(tab.doc.body).label }}.</b> Ce type n'est pas encore éditable ici ; il est conservé tel quel dans le fichier.</span></div>
            </div>
          } @else {
            <div class="pane-tools">
              <div class="seg" role="group" aria-label="Type de corps">
                @for (b of bodyTypes; track b.type) {
                  <button [attr.aria-pressed]="tab.doc.body.type === b.type" (click)="setBodyType(b.type)">{{ b.label }}</button>
                }
              </div>
              @if (isText(tab.doc.body.type)) {
                <span class="grow"></span>
                <span class="tools-hint">Les variables {{ '{{…}}' }} sont résolues à l'envoi.</span>
                @if (tab.doc.body.type === 'json') {
                  <button class="btn ghost sm" (click)="formatBody()">Formater</button>
                }
              }
            </div>
            @if (tab.doc.body.type === 'form-urlencoded') {
              <div class="pane-body">
                <section class="sec body-sec">
                  <div class="sec-head"><span class="sec-title">Champs du formulaire</span><span class="sec-meta">application/x-www-form-urlencoded</span></div>
                  <app-kv-table [rows]="formFields()" keyLabel="Nom" addLabel="Ajouter un champ" [descriptions]="true" (rowsChange)="setFormFields($event)" />
                </section>
                <div class="note"><app-ic name="variable" [size]="14" /><span>Les valeurs sont encodées à l'envoi ; les variables {{ '{{…}}' }} sont résolues avant.</span></div>
              </div>
            } @else if (tab.doc.body.type === 'multipart-form') {
              <div class="pane-body">
                <section class="sec body-sec">
                  <div class="sec-head"><span class="sec-title">Champs multipart</span><span class="sec-meta">multipart/form-data</span></div>
                  <app-multipart-table [fields]="multipartFields()" (fieldsChange)="setMultipartFields($event)" (pick)="pickFile($event)" />
                </section>
                <div class="note"><app-ic name="shield" [size]="14" /><span>Les fichiers sont lus dans la collection : chemins relatifs, sans « .. » ni chemin absolu.</span></div>
              </div>
            } @else if (isText(tab.doc.body.type)) {
              @for (path of [tab.path]; track path) {
                <app-code-editor class="pane-edit" [value]="$any(tab.doc.body).data" [language]="$any(tab.doc.body).type" label="Corps de la requête" (valueChange)="setBodyData($event)" />
              }
            } @else {
              <div class="pane-body fill">
                <div class="empty">
                  <span class="empty-ic"><app-ic name="file" [size]="18" /></span>
                  <h2>Pas de corps</h2>
                  <p>Choisis un type dans la barre au-dessus pour en ajouter un.</p>
                </div>
              </div>
            }
          }
        }
        @case ('headers') {
          <div class="pane-body">
            <section class="sec">
              <div class="sec-head"><span class="sec-title">En-têtes</span><span class="sec-meta">ceux de la collection et des dossiers s'ajoutent à l'envoi</span></div>
              <app-kv-table [rows]="tab.doc.headers" keyLabel="Nom" addLabel="Ajouter un en-tête" (rowsChange)="setHeaders($event)" />
            </section>
          </div>
        }
        @case ('auth') {
          <div class="pane-body">
            @defer (on immediate) {
              <app-auth-editor
                [auth]="tab.doc.auth"
                [token]="token()"
                [busy]="tokenBusy()"
                [error]="tokenError()"
                (authChange)="setAuth($event)"
                (tokenFetch)="fetchToken()"
                (tokenClear)="forgetToken()"
              />
            }
          </div>
        }
        @case ('tests') {
          <div class="pane-body">
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
          </div>
        }
        @case ('scripts') {
          <div class="pane-body">
            @for (path of [tab.path]; track path) {
              @for (k of scriptKinds; track k.kind) {
                <section class="sec">
                  <div class="sec-head wrap"><span class="sec-title">{{ k.label }}</span><span class="sec-meta">{{ k.hint }}</span></div>
                  <app-code-editor class="script-edit" [value]="code(k.kind)" language="javascript" [label]="k.label" (valueChange)="setScript(k.kind, $event)" />
                </section>
              }
            }
            <div class="note"><app-ic name="shield" [size]="14" /><span>Les scripts tournent dans un sandbox sans accès au disque ni au réseau, avec l'API de Bruno (<span class="mono">bru</span>, <span class="mono">req</span>, <span class="mono">res</span>). Ceux de la collection et des dossiers s'exécutent aussi : avant celui-ci à l'aller, après lui au retour.</span></div>
          </div>
        }
        @case ('docs') {
          <div class="pane-body fill">
            <textarea class="docs" [value]="tab.doc.docs ?? ''" (input)="setDocs($any($event.target).value)" placeholder="Documentation Markdown de la requête" aria-label="Documentation"></textarea>
          </div>
        }
      }
    }
  `,
  styles: `
    .docs { display: block; width: 100%; flex: 1; min-height: 260px; resize: none; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--line); background: var(--sunken); font: 1rem/calc(20 * var(--px)) var(--font-ui); font-variant-ligatures: none; tab-size: 2; outline: 0; }
    .docs:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .pane-body.fill { display: flex; flex-direction: column; }
    .pane-body.fill > .empty { flex: 1; height: auto; }
    .body-sec { margin-bottom: 12px; }
    .asserts .kv-row { grid-template-columns: 32px minmax(0, 1.2fr) 120px minmax(0, 1fr); }
    .op { width: 100%; height: max(28px, 1.5em); border: 0; background: transparent; font: var(--code-size) var(--code-font); outline: 0; }
    .op option { background: var(--pop); }
    .row-x { opacity: 0; margin-left: auto; }
    .kv-row:hover .row-x { opacity: 1; }
    .sec-head.wrap { flex-wrap: wrap; row-gap: 2px; }
    .sec-head.wrap .sec-title { white-space: nowrap; }
    .script-edit { display: block; height: 180px; border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); overflow: hidden; }
    .script-edit:focus-within { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
  `,
})
export class RequestPane {
  protected readonly ws = inject(Workspace);
  protected readonly section = signal<Section>('params');
  protected readonly bodyTypes = BODY_TYPES;
  protected readonly operators = OPERATORS;
  protected readonly scriptKinds = SCRIPT_KINDS;
  protected readonly lockAll = () => true;

  protected readonly token = signal<TokenInfo | null>(null);
  protected readonly tokenBusy = signal(false);
  protected readonly tokenError = signal<string | null>(null);

  constructor() {
    // Le jeton gardé pour la requête, relu quand on ouvre l'onglet Auth, après chaque envoi et un instant après une frappe.
    effect((onCleanup) => {
      const tab = this.ws.active();
      const root = this.ws.collection()?.root;
      const env = this.ws.env();
      if (this.section() !== 'auth' || !tab || !root || tab.doc.auth.type !== 'oauth2') {
        untracked(() => {
          this.token.set(null);
          this.tokenError.set(null);
        });
        return;
      }
      void tab.sentAt;
      let live = true;
      const timer = setTimeout(() => {
        api
          .oauthStatus(root, tab.path, tab.doc, env)
          .catch(() => null)
          .then((info) => live && this.token.set(info));
      }, 250);
      onCleanup(() => {
        live = false;
        clearTimeout(timer);
      });
    });
  }

  protected async fetchToken() {
    const root = this.ws.collection()?.root;
    const tab = this.ws.active();
    if (!root || !tab || this.tokenBusy()) return;
    this.tokenBusy.set(true);
    this.tokenError.set(null);
    try {
      const info = await api.oauthFetch(root, tab.path, tab.doc, this.ws.env());
      if (this.ws.activePath() === tab.path) this.token.set(info);
    } catch (e) {
      this.tokenError.set(String(e));
    } finally {
      this.tokenBusy.set(false);
    }
  }

  protected async forgetToken() {
    const root = this.ws.collection()?.root;
    const tab = this.ws.active();
    if (!root || !tab) return;
    try {
      await api.oauthClear(root, tab.path, tab.doc, this.ws.env());
      this.token.set(null);
    } catch (e) {
      this.tokenError.set(String(e));
    }
  }

  protected readonly query = computed(() => this.ws.active()?.doc.params.filter((p) => p.kind === 'query') ?? []);
  protected readonly path = computed(() => this.ws.active()?.doc.params.filter((p) => p.kind === 'path') ?? []);
  protected readonly formFields = computed(() => {
    const body = this.ws.active()?.doc.body;
    return body?.type === 'form-urlencoded' ? body.fields : [];
  });
  protected readonly multipartFields = computed(() => {
    const body = this.ws.active()?.doc.body;
    return body?.type === 'multipart-form' ? body.fields : [];
  });
  protected readonly sections = computed(() => {
    const d = this.ws.active()?.doc;
    if (!d) return [];
    return [
      { id: 'params' as const, label: 'Paramètres', count: d.params.filter((p) => p.enabled).length || '' },
      { id: 'body' as const, label: 'Corps', count: d.body.type === 'none' ? '' : BODY_BADGES[d.body.type] ?? ('data' in d.body ? d.body.type.toUpperCase() : 'autre') },
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
    this.ws.edit((d) => ({ ...d, body: switchBody(d.body, type) }));
  }

  protected setFormFields(fields: KeyValue[]) {
    this.ws.edit((d) => ({ ...d, body: { type: 'form-urlencoded', fields } }));
  }

  protected setMultipartFields(fields: MultipartField[]) {
    this.ws.edit((d) => ({ ...d, body: { type: 'multipart-form', fields } }));
  }

  protected async pickFile(index: number) {
    const root = this.ws.collection()?.root;
    const tab = this.ws.activePath();
    if (!root || !tab) return;
    const picked = await api.pickFile(root);
    if (!picked) return;
    const path = relativeToRoot(root, picked);
    if (!path) {
      this.ws.notify(OUTSIDE_COLLECTION, true);
      return;
    }
    this.ws.edit((d) => (d.body.type === 'multipart-form' ? { ...d, body: { ...d.body, fields: withFile(d.body.fields, index, path) } } : d), tab);
  }

  protected isText(type: Body['type']) {
    return TEXT_BODIES.has(type);
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

  protected setAuth(auth: Auth) {
    this.ws.edit((d) => ({ ...d, auth }));
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

  protected code(kind: string) {
    return scriptCode(this.ws.active()?.doc.scripts ?? [], kind);
  }

  protected setScript(kind: string, code: string) {
    this.ws.edit((d) => ({ ...d, scripts: withScript(d.scripts, kind, code) }));
  }
}
