import { ChangeDetectionStrategy, Component, ElementRef, computed, inject, signal, viewChild } from '@angular/core';

import { COMMANDS } from '../core/commands';
import { isCurlCommand } from '../core/curl';
import { Workspace } from '../core/store';
import { segments } from '../core/url';
import { Icon } from './icon';
import { METHODS } from './method';

/** Champ d'URL : un input transparent posé sur un miroir qui colore les {{variables}}. */
@Component({
  selector: 'app-url-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    @if (ws.active(); as tab) {
      <div class="url">
        <span class="method-wrap" [class]="'method-wrap m-' + tab.doc.method.toLowerCase()">
          <select class="method-select" [value]="tab.doc.method" (change)="setMethod($event)" aria-label="Méthode HTTP">
            @for (m of methods; track m) {
              <option [value]="m" [selected]="m === tab.doc.method">{{ m }}</option>
            }
          </select>
          <app-ic name="chev-down" [size]="12" />
        </span>
        <div class="url-field">
          <div class="url-mirror" aria-hidden="true" [style.transform]="'translateX(' + -scroll() + 'px)'">
            @for (seg of parts(); track $index) {
              @if (seg.variable) {
                <span
                  class="var"
                  [class.bad]="isUnresolved(seg.variable)"
                  [class.dyn]="seg.variable.startsWith('$')"
                  [class.env]="seg.variable.startsWith('process.env.')"
                  (mouseenter)="hover($event, seg.variable)"
                  (mouseleave)="ws.hover.set(null)"
                  (mousedown)="focusInput($event)"
                >{{ seg.text }}</span>
              } @else {
                <span>{{ seg.text }}</span>
              }
            }
          </div>
          <input
            #field
            class="url-input"
            type="text"
            spellcheck="false"
            autocomplete="off"
            aria-label="URL de la requête"
            placeholder="https://api.exemple.test/ressource ou {{ '{{' }}baseUrl{{ '}}' }}/ressource"
            [value]="tab.doc.url"
            (input)="ws.setUrl($any($event.target).value)"
            (paste)="paste($event)"
            (scroll)="syncScroll()"
            (keyup)="syncScroll()"
            (click)="syncScroll()"
          />
        </div>
      </div>
      <button class="btn-primary lg" [class.is-sending]="!!tab.sendingId" (click)="ws.send()" [attr.aria-label]="tab.sendingId ? 'Annuler la requête' : 'Envoyer la requête'">
        @if (tab.sendingId) {
          <span class="spinner"></span>Annuler <kbd class="kbd">{{ label('request.cancel') }}</kbd>
        } @else {
          <app-ic name="send" [size]="15" />Envoyer <kbd class="kbd">{{ label('request.send') }}</kbd>
        }
      </button>
    }
  `,
  host: { class: 'urlbar' },
  styles: `
    .url-field { position: relative; flex: 1; min-width: 0; height: 100%; overflow: hidden; }
    .url-input, .url-mirror { font: 13px var(--font-mono); font-variant-ligatures: none; padding: 0 12px; letter-spacing: 0; }
    .url-input { position: relative; z-index: 1; width: 100%; height: 100%; border: 0; outline: 0; background: transparent; color: transparent; caret-color: var(--ink); }
    .url-input::placeholder { color: var(--faint); }
    .url-input::selection { background: var(--accent-line); color: transparent; }
    .url-mirror { position: absolute; inset: 0; z-index: 2; display: flex; align-items: center; white-space: pre; pointer-events: none; color: var(--ink); }
    .url-mirror .var { pointer-events: auto; height: auto; padding: 0; margin: 0; border-radius: 4px; box-shadow: 0 0 0 2px var(--accent-soft); }
    .url-mirror .var.bad { box-shadow: 0 0 0 2px var(--bad-soft); }
    .method-wrap { position: relative; height: 100%; display: flex; align-items: center; border-right: 1px solid var(--line); }
    .method-wrap app-ic, .method-wrap svg { position: absolute; right: 9px; pointer-events: none; opacity: .7; }
    select.method-select { appearance: none; -webkit-appearance: none; height: 100%; min-width: 92px; background: transparent; border: 0; border-radius: 8px 0 0 8px; color: inherit; font: 600 12px var(--font-mono); cursor: pointer; padding: 0 28px 0 12px; outline-offset: -2px; }
    select.method-select:hover { background: var(--hover); }
    select.method-select option { color: var(--ink); background: var(--pop); }
  `,
})
export class UrlBar {
  protected readonly ws = inject(Workspace);
  private readonly commands = inject(COMMANDS);
  protected readonly methods = METHODS;
  protected readonly scroll = signal(0);
  private readonly field = viewChild<ElementRef<HTMLInputElement>>('field');

  protected readonly parts = computed(() => segments(this.ws.active()?.doc.url ?? ''));

  protected label(id: string) {
    return this.commands.label(id);
  }

  protected isUnresolved(name: string) {
    if (name.startsWith('$') || name.startsWith('process.env.')) return false;
    const v = this.ws.varMap().get(name);
    return !v || v.value == null;
  }

  protected setMethod(event: Event) {
    const method = (event.target as HTMLSelectElement).value;
    this.ws.edit((d) => ({ ...d, method }));
  }

  protected paste(event: ClipboardEvent) {
    const text = event.clipboardData?.getData('text') ?? '';
    if (this.ws.active()?.doc.requestType !== 'http' || !isCurlCommand(text)) return;
    event.preventDefault();
    const input = event.target as HTMLInputElement;
    const start = input.selectionStart ?? input.value.length;
    const end = input.selectionEnd ?? input.value.length;
    void this.ws.pasteCurl(text).then((applied) => {
      if (!applied) {
        input.setRangeText(text.replace(/\r?\n/g, ' '), start, end, 'end');
        input.dispatchEvent(new Event('input'));
      }
      setTimeout(() => {
        if (applied) input.setSelectionRange(input.value.length, input.value.length);
        this.syncScroll();
      });
    });
  }

  protected syncScroll() {
    this.scroll.set(this.field()?.nativeElement.scrollLeft ?? 0);
  }

  protected hover(event: MouseEvent, name: string) {
    this.ws.hover.set({ name, rect: (event.target as HTMLElement).getBoundingClientRect() });
  }

  protected focusInput(event: MouseEvent) {
    event.preventDefault();
    this.field()?.nativeElement.focus();
  }

  focus() {
    this.field()?.nativeElement.focus();
  }
}
