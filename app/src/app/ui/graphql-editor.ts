import { ChangeDetectionStrategy, Component, computed, effect, inject, untracked } from '@angular/core';

import { GraphqlSchemas } from '../core/graphql-schema';
import { GraphqlParts, ageLabel, describeSchema, formatVariables, graphqlParts, summarize, withGraphql } from '../core/graphql';
import { Workspace } from '../core/store';
import { CodeEditor } from './code-editor';
import { Icon } from './icon';

const RESTORE_DELAY_MS = 300;

/** Corps d'une requête GraphQL : la requête, ses variables JSON, et le schéma du serveur qui guide la saisie. */
@Component({
  selector: 'app-graphql-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CodeEditor, Icon],
  host: { class: 'gql' },
  template: `
    @if (ws.active(); as tab) {
      <div class="pane-tools">
        <button class="btn ghost sm" [disabled]="state().loading" (click)="load()">
          <app-ic [name]="state().schema ? 'sync' : 'download'" [size]="14" />{{ state().loading ? 'Introspection…' : state().schema ? 'Actualiser le schéma' : 'Charger le schéma' }}
        </button>
        <span class="tools-hint" [title]="status()">{{ status() }}</span>
        <span class="grow"></span>
        <button class="btn ghost sm" (click)="format()">Formater</button>
      </div>
      @if (state().error; as error) {
        <div class="gql-error">
          <div class="banner err"><app-ic name="alert" [size]="15" /><span>{{ error }}</span></div>
        </div>
      }
      @for (path of [tab.path]; track path) {
        <div class="gql-split">
          <section class="gql-box">
            <div class="sec-head"><span class="sec-title">Requête</span><span class="sec-meta">{{ queryHint() }}</span></div>
            <app-code-editor
              class="gql-edit"
              language="graphql"
              label="Requête GraphQL"
              [value]="parts().query"
              [schema]="state().schema?.introspection ?? null"
              (valueChange)="set({ query: $event })"
            />
          </section>
          <section class="gql-box">
            <div class="sec-head"><span class="sec-title">Variables</span><span class="sec-meta">JSON · résolues à l'envoi, commentaires // admis</span></div>
            <app-code-editor class="gql-edit" language="json" label="Variables GraphQL" [value]="parts().variables" (valueChange)="set({ variables: $event })" />
          </section>
        </div>
      }
    }
  `,
  styles: `
    :host { flex: 1; min-height: 0; display: flex; flex-direction: column; }
    .tools-hint { display: block; }
    .gql-error { padding: 10px 10px 0; }
    .gql-split { flex: 1; min-height: 0; display: grid; grid-template-rows: minmax(0, 1fr) minmax(96px, 32%); gap: 12px; padding: 10px 10px 12px; }
    .gql-box { display: flex; flex-direction: column; min-height: 0; }
    .gql-box .sec-head { margin: 0 0 6px 2px; }
    .gql-edit { flex: 1; min-height: 0; border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); overflow: hidden; }
    .gql-edit:focus-within { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
  `,
})
export class GraphqlEditor {
  protected readonly ws = inject(Workspace);
  private readonly schemas = inject(GraphqlSchemas);
  protected readonly state = this.schemas.current;
  protected readonly parts = computed(() => graphqlParts(this.ws.active()?.doc.body ?? { type: 'none' }));

  protected readonly status = computed(() => {
    const { schema, loading } = this.state();
    if (loading) return 'Introspection en cours…';
    if (!schema) return 'Sans schéma : charge-le pour compléter les champs et repérer les erreurs.';
    const summary = summarize(schema.introspection);
    return [summary ? describeSchema(summary) : 'Schéma chargé', ageLabel(schema.fetchedAt, Date.now())].filter(Boolean).join(' · ');
  });

  protected readonly queryHint = computed(() =>
    this.state().schema ? 'Ctrl+Espace : suggestions du schéma' : 'les {{…}} sont résolues à l\'envoi',
  );

  private readonly lookup = computed(() => {
    const tab = this.ws.active();
    return tab ? `${tab.path}\n${this.ws.env() ?? ''}\n${tab.doc.url}` : '';
  });

  constructor() {
    effect((onCleanup) => {
      if (!this.lookup()) return;
      const timer = setTimeout(() => {
        const tab = untracked(() => this.ws.active());
        if (tab) void this.schemas.restore(tab.path, tab.doc);
      }, RESTORE_DELAY_MS);
      onCleanup(() => clearTimeout(timer));
    });
  }

  protected load() {
    const tab = this.ws.active();
    if (tab) void this.schemas.fetch(tab.path, tab.doc);
  }

  protected set(patch: Partial<GraphqlParts>) {
    this.ws.edit((d) => withGraphql(d, patch));
  }

  protected async format() {
    const tab = this.ws.active();
    if (!tab) return;
    const { query, variables } = graphqlParts(tab.doc.body);
    const patch: Partial<GraphqlParts> = {};
    const problems: string[] = [];
    if (query.trim()) {
      const { formatQuery } = await import('./graphql-language');
      const formatted = await formatQuery(query);
      if (!formatted.ok) problems.push(formatted.reason);
      else if (formatted.text !== query) patch.query = formatted.text;
    }
    const vars = formatVariables(variables);
    if (!vars.ok) problems.push(vars.reason);
    else if (vars.text !== variables) patch.variables = vars.text;
    if (Object.keys(patch).length) this.ws.edit((d) => withGraphql(d, patch), tab.path);
    if (problems.length) this.ws.notify(problems.join(' '), true);
  }
}
