import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';

import { Block, parseMarkdown, safeHref } from '../core/markdown';

/** Markdown montré en lecture : aucun HTML brut, liens limités à http, https et mailto, ouverts hors de l'application. */
@Component({
  selector: 'app-markdown',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet],
  template: `
    <div class="md">
      <ng-container *ngTemplateOutlet="blocksTpl; context: { $implicit: blocks() }" />
    </div>
    <ng-template #blocksTpl let-list>
      @for (b of list; track $index) {
        @switch (b.kind) {
          @case ('heading') {
            @switch (b.level) {
              @case (1) { <h1><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: b.children }" /></h1> }
              @case (2) { <h2><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: b.children }" /></h2> }
              @case (3) { <h3><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: b.children }" /></h3> }
              @case (4) { <h4><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: b.children }" /></h4> }
              @case (5) { <h5><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: b.children }" /></h5> }
              @default { <h6><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: b.children }" /></h6> }
            }
          }
          @case ('paragraph') {
            <p><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: b.children }" /></p>
          }
          @case ('code') {
            <pre class="mono" [attr.data-language]="b.language || null" tabindex="0"><code>{{ b.text }}</code></pre>
          }
          @case ('quote') {
            <blockquote><ng-container *ngTemplateOutlet="blocksTpl; context: { $implicit: b.children }" /></blockquote>
          }
          @case ('rule') {
            <hr />
          }
          @case ('list') {
            @if (b.ordered) {
              <ol [attr.start]="b.start === 1 ? null : b.start">
                @for (item of b.items; track $index) {
                  <li><ng-container *ngTemplateOutlet="blocksTpl; context: { $implicit: item }" /></li>
                }
              </ol>
            } @else {
              <ul>
                @for (item of b.items; track $index) {
                  <li><ng-container *ngTemplateOutlet="blocksTpl; context: { $implicit: item }" /></li>
                }
              </ul>
            }
          }
          @case ('table') {
            <div class="md-table">
              <table>
                <thead>
                  <tr>
                    @for (cell of b.head; track $index) {
                      <th [style.text-align]="b.align[$index]"><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: cell }" /></th>
                    }
                  </tr>
                </thead>
                <tbody>
                  @for (row of b.rows; track $index) {
                    <tr>
                      @for (cell of row; track $index) {
                        <td [style.text-align]="b.align[$index]"><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: cell }" /></td>
                      }
                    </tr>
                  }
                </tbody>
              </table>
            </div>
          }
        }
      }
    </ng-template>
    <ng-template #inlineTpl let-list>
      @for (n of list; track $index) {
        @switch (n.kind) {
          @case ('text') {<ng-container>{{ n.text }}</ng-container>}
          @case ('code') { <code class="mono">{{ n.text }}</code> }
          @case ('strong') { <strong><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: n.children }" /></strong> }
          @case ('em') { <em><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: n.children }" /></em> }
          @case ('del') { <del><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: n.children }" /></del> }
          @case ('break') { <br /> }
          @case ('link') {
            @if (href(n.href); as url) {
              <a [href]="url" target="_blank" rel="noopener noreferrer"><ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: n.children }" /></a>
            } @else {
              <ng-container *ngTemplateOutlet="inlineTpl; context: { $implicit: n.children }" />
            }
          }
        }
      }
    </ng-template>
  `,
  styles: `
    :host { display: block; }
    .md { font: 1rem/1.55 var(--font-ui); color: var(--ink); overflow-wrap: anywhere; }
    .md > :first-child { margin-top: 0; }
    .md > :last-child { margin-bottom: 0; }
    h1, h2, h3, h4, h5, h6 { margin: 1.2em 0 0.4em; line-height: 1.25; font-weight: 600; }
    h1 { font-size: 1.5em; padding-bottom: 0.25em; border-bottom: 1px solid var(--line); }
    h2 { font-size: 1.25em; padding-bottom: 0.2em; border-bottom: 1px solid var(--line); }
    h3 { font-size: 1.1em; }
    h4, h5, h6 { font-size: 1em; }
    h6 { color: var(--muted); }
    p, ul, ol, blockquote, pre, .md-table { margin: 0 0 0.8em; }
    ul, ol { padding-left: 1.5em; }
    li > p { margin-bottom: 0.3em; }
    a { color: var(--accent); text-decoration: underline; text-underline-offset: 2px; }
    code { padding: 0.1em 0.35em; border-radius: 4px; background: var(--sunken); font-size: 0.92em; }
    pre { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--line); background: var(--sunken); overflow: auto; font-size: 0.92em; line-height: 1.5; }
    pre code { padding: 0; background: none; font-size: inherit; }
    blockquote { padding: 0 0 0 12px; border-left: 3px solid var(--line); color: var(--muted); }
    hr { border: 0; border-top: 1px solid var(--line); margin: 1.2em 0; }
    .md-table { overflow-x: auto; }
    table { border-collapse: collapse; min-width: 50%; }
    th, td { padding: 5px 10px; border: 1px solid var(--line); }
    th { background: var(--sunken); font-weight: 600; }
  `,
})
export class MarkdownView {
  readonly source = input('');
  protected readonly blocks = computed<Block[]>(() => parseMarkdown(this.source()));

  protected href(raw: string): string | null {
    return safeHref(raw);
  }
}

