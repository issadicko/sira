import { ChangeDetectionStrategy, Component, DestroyRef, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { shortcutLabel } from '../core/commands';
import { describeCurl, nameFromUrl } from '../core/curl';
import { RequestDoc } from '../core/model';
import { dirname, folderChoices } from '../core/paths';
import { Workspace } from '../core/store';
import { Dialog } from './dialog';
import { Icon } from './icon';
import { methodClass } from './method';

const PLACEHOLDER = `curl -X POST https://api.exemple.test/v1/transactions \\
  -H 'Content-Type: application/json' \\
  -d '{"montant": 1500}'`;

/** Crée un fichier de requête dans la collection à partir d'une commande cURL. */
@Component({
  selector: 'app-curl-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  template: `
    <app-dialog heading="Nouvelle requête depuis cURL" [busy]="busy()" (closed)="close()" (confirmed)="create()">
      <section class="fld">
        <label class="sec-head" for="curl-command"><span class="sec-title">Commande cURL</span><span class="sec-meta">depuis un terminal, une doc ou « Copier en cURL »</span></label>
        <textarea
          id="curl-command"
          class="input mono curl-text"
          data-autofocus
          rows="6"
          spellcheck="false"
          autocomplete="off"
          [placeholder]="placeholder"
          [value]="command()"
          (input)="edit($any($event.target).value)"
        ></textarea>
        <div class="curl-state" aria-live="polite">
          @if (curl(); as c) {
            <span [class]="methodClass(c.method)">{{ c.method.toUpperCase() }}</span>
            <span class="curl-url mono" [title]="c.url">{{ c.url }}</span>
            <span class="curl-meta">{{ describe(c) }}</span>
          } @else if (parseError(); as e) {
            <app-ic name="alert" [size]="14" /><span class="curl-bad">{{ e }}</span>
          } @else if (parsing()) {
            <span class="spinner"></span><span class="faint">Analyse…</span>
          } @else {
            <span class="faint">Colle la commande : l'URL, la méthode, les en-têtes, le corps et l'authentification sont repris.</span>
          }
        </div>
      </section>
      <div class="cols">
        <label class="fld">
          <span class="sec-head"><span class="sec-title">Nom</span></span>
          <input class="input" type="text" spellcheck="false" autocomplete="off" aria-label="Nom de la requête" [value]="name()" (input)="rename($any($event.target).value)" placeholder="Nouvelle requête" />
        </label>
        <label class="fld">
          <span class="sec-head"><span class="sec-title">Dossier</span></span>
          <span class="select-wrap">
            <select class="input" aria-label="Dossier de destination" (change)="folder.set($any($event.target).value)">
              <option value="" [selected]="folder() === ''">(racine de la collection)</option>
              @for (f of folders(); track f.path) {
                <option [value]="f.path" [selected]="folder() === f.path">{{ indent(f.depth) + f.label }}</option>
              }
            </select>
            <app-ic name="chev-down" [size]="13" />
          </span>
        </label>
      </div>
      @if (error(); as e) {
        <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
      }
      <div dialog-foot class="foot-actions">
        <button class="btn ghost" [disabled]="busy()" (click)="close()">Annuler</button>
        <button class="btn-primary" [class.is-sending]="busy()" [disabled]="!ready() || busy()" (click)="create()">
          @if (busy()) {
            <span class="spinner"></span>Création…
          } @else {
            Créer la requête <kbd class="kbd">{{ confirmKey }}</kbd>
          }
        </button>
      </div>
    </app-dialog>
  `,
  styles: `
    .curl-text { height: auto; min-height: 120px; padding: 8px 10px; resize: vertical; line-height: 20px; white-space: pre-wrap; overflow-wrap: anywhere; }
    .curl-state { display: flex; align-items: center; gap: 8px; min-height: 22px; font-size: calc(12 * var(--px)); min-width: 0; }
    .curl-url { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: calc(12 * var(--px)); }
    .curl-meta { margin-left: auto; flex-shrink: 0; color: var(--faint); }
    .curl-bad { color: var(--bad); }
    .curl-state .ic { color: var(--bad); }
    .curl-state .m { width: auto; font-size: calc(11 * var(--px)); }
  `,
})
export class CurlDialog {
  protected readonly ws = inject(Workspace);
  protected readonly placeholder = PLACEHOLDER;
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly methodClass = methodClass;
  protected readonly describe = describeCurl;
  protected readonly command = signal('');
  protected readonly curl = signal<RequestDoc | null>(null);
  protected readonly parseError = signal<string | null>(null);
  protected readonly parsing = signal(false);
  protected readonly name = signal('');
  protected readonly folder = signal('');
  protected readonly busy = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly folders = computed(() => folderChoices(this.ws.collection()?.items ?? []));
  protected readonly ready = computed(() => !!this.curl() && this.name().trim() !== '');

  private nameEdited = false;
  private pending = 0;
  private timer?: ReturnType<typeof setTimeout>;

  constructor() {
    const active = dirname(this.ws.activePath() ?? '');
    if (this.folders().some((f) => f.path === active)) this.folder.set(active);
    inject(DestroyRef).onDestroy(() => clearTimeout(this.timer));
  }

  protected indent(depth: number) {
    return '\u00a0\u00a0'.repeat(depth);
  }

  protected edit(text: string) {
    this.command.set(text);
    this.error.set(null);
    this.curl.set(null);
    this.parseError.set(null);
    clearTimeout(this.timer);
    const pending = ++this.pending;
    this.parsing.set(text.trim() !== '');
    if (text.trim()) this.timer = setTimeout(() => void this.parse(text, pending), 250);
  }

  protected rename(name: string) {
    this.nameEdited = name !== '';
    this.name.set(name);
    this.error.set(null);
  }

  protected close() {
    this.ws.dialog.set(null);
  }

  protected async create() {
    const c = this.ws.collection();
    if (!c || !this.ready() || this.busy()) return;
    this.busy.set(true);
    this.error.set(null);
    try {
      const path = await api.createRequestFromCurl(c.root, this.folder(), this.name().trim(), this.command());
      await this.ws.reload();
      this.ws.reveal(path);
      await this.ws.openRequest(path, true);
      this.close();
      this.ws.notify(`Requête créée : ${path}`);
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.busy.set(false);
    }
  }

  private async parse(text: string, pending: number) {
    try {
      const curl = await api.parseCurl(text);
      if (pending !== this.pending) return;
      if (curl?.url) {
        this.curl.set(curl);
        if (!this.nameEdited) this.name.set(nameFromUrl(curl.url));
      } else {
        this.parseError.set('Commande cURL invalide : aucune URL trouvée.');
      }
    } catch (e) {
      if (pending === this.pending) this.parseError.set(String(e));
    } finally {
      if (pending === this.pending) this.parsing.set(false);
    }
  }
}
