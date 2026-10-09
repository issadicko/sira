import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { WsMessage } from '../core/model';
import { Workspace } from '../core/store';
import { WsStore } from '../core/ws-store';
import { messageLabel, messagesOf, selectedOf, withAddedMessage, withMessage, withoutMessage } from '../core/websocket';
import { CodeEditor, CodeLanguage } from './code-editor';
import { Icon } from './icon';

const LANGUAGES: Record<string, CodeLanguage> = { json: 'json', xml: 'xml' };
const KINDS = [
  { id: 'text', label: 'Texte' },
  { id: 'json', label: 'JSON' },
  { id: 'xml', label: 'XML' },
];

/** Les messages d'une requête WebSocket : à cocher, titrer, éditer et envoyer un à un ou ensemble. */
@Component({
  selector: 'app-ws-messages',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CodeEditor, Icon],
  host: { style: 'display: contents' },
  styles: `
    .msg { border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); margin-bottom: 10px; overflow: hidden; }
    .msg.off { opacity: .7; }
    .msg-head { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-bottom: 1px solid var(--line); }
    .msg-head .title { flex: 1; min-width: 0; height: 28px; padding: 0 8px; border: 1px solid transparent; border-radius: 6px; background: transparent; font: inherit; color: inherit; outline: 0; }
    .msg-head .title:hover { border-color: var(--line); }
    .msg-head .title:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .msg-head .kind { height: 28px; border: 1px solid var(--line); border-radius: 6px; background: var(--pop); color: inherit; font: inherit; padding: 0 6px; }
    .msg-edit { display: block; height: 120px; overflow: hidden; }
    .count { color: var(--faint); font-size: calc(11.5 * var(--px)); }
  `,
  template: `
    @if (ws.active(); as tab) {
      <div class="pane-tools">
        <span class="tools-hint">Les variables {{ '{{…}}' }} sont résolues à l'envoi. {{ checked() }} coché{{ checked() > 1 ? 's' : '' }} sur {{ messages().length }}.</span>
        <span class="grow"></span>
        <button class="btn sm" [disabled]="!open() || !checked()" (click)="store.sendSelected()" title="Envoie dans l'ordre les messages cochés">Envoyer les cochés</button>
      </div>
      <div class="pane-body">
        @for (m of messages(); track $index; let i = $index) {
          <section class="msg" [class.off]="!m.selected">
            <div class="msg-head">
              <input type="checkbox" class="cb" [checked]="m.selected" (change)="patch(i, { selected: !m.selected })" [attr.aria-label]="'Cocher ' + label(m, i)" />
              <input class="title" type="text" spellcheck="false" [value]="m.title" [placeholder]="'Message ' + (i + 1)" (input)="patch(i, { title: $any($event.target).value })" aria-label="Titre du message" />
              <select class="kind" [value]="m.kind" (change)="patch(i, { kind: $any($event.target).value })" aria-label="Type du message">
                @for (k of kinds; track k.id) {
                  <option [value]="k.id" [selected]="k.id === m.kind">{{ k.label }}</option>
                }
              </select>
              <button class="btn sm" [disabled]="!open()" (click)="store.send(m.data)" [attr.aria-label]="'Envoyer ' + label(m, i)">Envoyer</button>
              <button class="icon-btn sm" (click)="remove(i)" [attr.aria-label]="'Supprimer ' + label(m, i)"><app-ic name="x" [size]="13" /></button>
            </div>
            @for (path of [tab.path]; track path) {
              <app-code-editor class="msg-edit" [value]="m.data" [language]="language(m)" [label]="'Contenu de ' + label(m, i)" (valueChange)="patch(i, { data: $event })" />
            }
          </section>
        } @empty {
          <div class="note"><app-ic name="send" [size]="14" /><span>Aucun message : la connexion s'ouvre sans rien envoyer. Ajoute-en un pour l'envoyer à la connexion ou à la demande.</span></div>
        }
        <button class="btn ghost" (click)="add()"><app-ic name="plus" [size]="14" />Ajouter un message</button>
      </div>
    }
  `,
})
export class WsMessages {
  protected readonly ws = inject(Workspace);
  protected readonly store = inject(WsStore);
  protected readonly kinds = KINDS;
  protected readonly messages = computed(() => messagesOf(this.ws.active()?.doc ?? ({} as never)));
  protected readonly checked = computed(() => selectedOf(this.ws.active()?.doc ?? ({} as never)).length);
  protected readonly open = computed(() => this.store.active().status === 'open');

  protected label = messageLabel;

  protected language(message: WsMessage): CodeLanguage {
    return LANGUAGES[message.kind] ?? 'text';
  }

  protected patch(index: number, patch: Partial<WsMessage>) {
    this.ws.edit((d) => withMessage(d, index, patch));
  }

  protected add() {
    this.ws.edit((d) => withAddedMessage(d));
  }

  protected remove(index: number) {
    this.ws.edit((d) => withoutMessage(d, index));
  }
}
