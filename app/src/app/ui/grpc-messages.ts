import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { GrpcMessage } from '../core/model';
import { GrpcStore } from '../core/grpc-store';
import { KIND_LABELS, canSend, messageLabel, messagesOf, withAddedMessage, withMessage, withMethod, withoutMessage } from '../core/grpc';
import { Workspace } from '../core/store';
import { CodeEditor } from './code-editor';
import { Icon } from './icon';

/** La méthode appelée (choisie parmi celles du schéma) et les messages JSON d'une requête gRPC. */
@Component({
  selector: 'app-grpc-messages',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CodeEditor, Icon],
  host: { style: 'display: contents' },
  styles: `
    .fields { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 8px 10px; align-items: center; margin-bottom: 14px; }
    .fields label { color: var(--muted); font-size: calc(12 * var(--px)); }
    .fields input { height: 30px; padding: 0 10px; border: 1px solid var(--line); border-radius: 6px; background: var(--sunken); font: var(--code-size) var(--code-font); color: inherit; outline: 0; min-width: 0; }
    .fields input:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .kind { display: inline-flex; align-items: center; height: 22px; padding: 0 8px; border-radius: 11px; background: var(--hover); color: var(--muted); font-size: calc(11.5 * var(--px)); white-space: nowrap; }
    .src { grid-column: 2 / 4; color: var(--faint); font-size: calc(11.5 * var(--px)); }
    .msg { border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); margin-bottom: 10px; overflow: hidden; }
    .msg-head { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-bottom: 1px solid var(--line); }
    .msg-head .title { flex: 1; min-width: 0; height: 28px; padding: 0 8px; border: 1px solid transparent; border-radius: 6px; background: transparent; font: inherit; color: inherit; outline: 0; }
    .msg-head .title:hover { border-color: var(--line); }
    .msg-head .title:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .msg-edit { display: block; height: 140px; overflow: hidden; }
    .err { color: var(--bad); font-size: calc(12 * var(--px)); margin: -6px 0 12px; }
  `,
  template: `
    @if (ws.active(); as tab) {
      <div class="pane-tools">
        <span class="tools-hint">Les variables {{ '{{…}}' }} sont résolues à l'envoi. {{ streaming() ? 'Tous les messages partent dans l\\'ordre.' : 'Seul le premier message part.' }}</span>
        <span class="grow"></span>
        @if (canSendMore()) {
          <button class="btn sm" (click)="grpc.finish()" title="Annonce au serveur qu'aucun autre message ne partira">Terminer l'envoi</button>
        }
      </div>
      <div class="pane-body">
        <div class="fields">
          <label for="grpc-method">Méthode</label>
          <input id="grpc-method" type="text" list="grpc-methods" spellcheck="false" autocomplete="off" placeholder="paquet.Service/Méthode" [value]="tab.doc.method" (change)="setMethod($any($event.target).value)" />
          <span class="kind">{{ kindLabel() }}</span>
          <datalist id="grpc-methods">
            @for (m of methods().data?.methods ?? []; track m.fullName) {
              <option [value]="m.fullName"></option>
            }
          </datalist>
          <label for="grpc-proto">Fichier .proto</label>
          <input id="grpc-proto" type="text" spellcheck="false" autocomplete="off" placeholder="vide : réflexion du serveur" [value]="tab.doc.protoFile ?? ''" (change)="setProto($any($event.target).value)" />
          <button class="btn sm" (click)="grpc.loadMethods()" [disabled]="methods().loading">{{ methods().loading ? 'Chargement…' : 'Charger les méthodes' }}</button>
          @if (methods().data; as data) {
            <span class="src">{{ data.methods.length }} méthode{{ data.methods.length > 1 ? 's' : '' }} · schéma : {{ data.source }}</span>
          }
        </div>
        @if (methods().error; as error) {
          <p class="err" role="alert">{{ error }}</p>
        }
        @for (m of messages(); track $index; let i = $index) {
          <section class="msg">
            <div class="msg-head">
              <input class="title" type="text" spellcheck="false" [value]="m.description" [placeholder]="'Message ' + (i + 1)" (input)="patch(i, { description: $any($event.target).value })" aria-label="Description du message" />
              <button class="btn sm" [disabled]="!canSendMore()" (click)="grpc.send(m.message)" [attr.aria-label]="'Envoyer ' + label(m, i)">Envoyer</button>
              <button class="icon-btn sm" (click)="remove(i)" [attr.aria-label]="'Supprimer ' + label(m, i)"><app-ic name="x" [size]="13" /></button>
            </div>
            @for (path of [tab.path]; track path) {
              <app-code-editor class="msg-edit" [value]="m.message" language="json" [label]="'JSON de ' + label(m, i)" (valueChange)="patch(i, { message: $event })" />
            }
          </section>
        } @empty {
          <div class="note"><app-ic name="send" [size]="14" /><span>Aucun message : l'appel part avec un message vide <code>{{ '{}' }}</code>. Charge les méthodes puis choisis-en une pour obtenir un message d'exemple.</span></div>
        }
        <button class="btn ghost" (click)="add()"><app-ic name="plus" [size]="14" />Ajouter un message</button>
      </div>
    }
  `,
})
export class GrpcMessages {
  protected readonly ws = inject(Workspace);
  protected readonly grpc = inject(GrpcStore);
  protected readonly methods = this.grpc.methods;
  protected readonly messages = computed(() => messagesOf(this.ws.active()?.doc ?? ({} as never)));
  protected readonly streaming = computed(() => {
    const type = this.ws.active()?.doc.grpcMethodType;
    return type === 'client-streaming' || type === 'bidi-streaming';
  });
  protected readonly kindLabel = computed(() => KIND_LABELS[this.ws.active()?.doc.grpcMethodType ?? ''] ?? 'Type d\'appel inconnu');
  protected readonly canSendMore = computed(() => canSend(this.grpc.active()));

  protected label = messageLabel;

  protected setMethod(value: string) {
    const name = value.trim();
    const known = this.methods().data?.methods.find((m) => m.fullName === name);
    this.ws.edit((d) => (known ? withMethod(d, known) : { ...d, method: name }));
  }

  protected setProto(value: string) {
    this.ws.edit((d) => ({ ...d, protoFile: value.trim() }));
  }

  protected patch(index: number, patch: Partial<GrpcMessage>) {
    this.ws.edit((d) => withMessage(d, index, patch));
  }

  protected add() {
    this.ws.edit((d) => withAddedMessage(d));
  }

  protected remove(index: number) {
    this.ws.edit((d) => withoutMessage(d, index));
  }
}
