import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { shortcutLabel } from '../core/commands';
import { CODE_LANGUAGES, CodeLanguage } from '../core/model';
import { Workspace } from '../core/store';
import { Dialog } from './dialog';
import { Icon } from './icon';

const STORAGE_KEY = 'xc-code-language';

function remembered(): CodeLanguage {
  try {
    const id = localStorage.getItem(STORAGE_KEY);
    return CODE_LANGUAGES.find((l) => l.id === id)?.id ?? 'curl';
  } catch {
    return 'curl';
  }
}

/** Le code de la requête ouverte dans un langage au choix, variables résolues comme à l'envoi. */
@Component({
  selector: 'app-code-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  template: `
    <app-dialog heading="Générer du code" (closed)="close()" (confirmed)="copy()">
      <section class="fld">
        <div class="sec-head"><span class="sec-title">Langage</span><span class="sec-meta">{{ library() }}</span></div>
        <div class="seg langs" role="group" aria-label="Langage">
          @for (l of languages; track l.id) {
            <button [attr.aria-pressed]="language() === l.id" (click)="choose(l.id)">{{ l.label }}</button>
          }
        </div>
      </section>

      <section class="fld">
        <div class="sec-head"><span class="sec-title">Code</span><span class="sec-meta">la requête telle qu'elle partirait, variables remplacées</span></div>
        <div class="code-box">
          <pre class="code-out mono" tabindex="0" aria-label="Code généré" [class.is-stale]="loading()">{{ code() }}</pre>
          <button class="icon-btn sm code-copy" (click)="copy()" [disabled]="!code()" title="Copier le code" aria-label="Copier le code"><app-ic name="copy" [size]="15" /></button>
        </div>
      </section>

      @if (unresolved().length) {
        <div class="banner" role="status">
          <app-ic name="alert" [size]="15" />
          <span>Sans valeur dans l'environnement actif : <b class="mono">{{ unresolved().join(', ') }}</b>. Elles restent entre accolades dans le code.</span>
        </div>
      }
      @if (error(); as e) {
        <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
      }
      <div dialog-foot class="foot-actions">
        <button class="btn ghost" (click)="close()">Fermer</button>
        <button class="btn-primary" [disabled]="!code()" (click)="copy()"><app-ic name="copy" [size]="15" />Copier le code <kbd class="kbd">{{ confirmKey }}</kbd></button>
      </div>
    </app-dialog>
  `,
  styles: `
    .langs { display: flex; flex-wrap: wrap; width: fit-content; max-width: 100%; }
    .code-box { position: relative; }
    .code-out {
      margin: 0; padding: 12px 14px; min-height: 120px; max-height: min(46vh, 380px); overflow: auto;
      border: 1px solid var(--line); border-radius: 8px; background: var(--sunken); color: var(--ink);
      font: var(--code-size) / 1.55 var(--code-font); font-variant-ligatures: none; white-space: pre; tab-size: 4;
      transition: opacity .12s;
    }
    .code-out.is-stale { opacity: .55; }
    .code-copy { position: absolute; top: 8px; right: 8px; background: var(--raised); box-shadow: inset 0 0 0 1px var(--line); }
  `,
})
export class CodeDialog {
  protected readonly ws = inject(Workspace);
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly languages = CODE_LANGUAGES;
  protected readonly language = signal<CodeLanguage>(remembered());
  protected readonly library = computed(() => {
    const l = CODE_LANGUAGES.find((x) => x.id === this.language());
    return l ? `${l.label} · ${l.library}` : '';
  });
  protected readonly code = signal('');
  protected readonly unresolved = signal<string[]>([]);
  protected readonly error = signal<string | null>(null);
  protected readonly loading = signal(false);
  private sequence = 0;

  constructor() {
    void this.generate();
  }

  protected close() {
    this.ws.dialog.set(null);
  }

  protected choose(id: CodeLanguage) {
    if (this.language() === id) return;
    this.language.set(id);
    try {
      localStorage.setItem(STORAGE_KEY, id);
    } catch {
      /* préférence non retenue : sans importance */
    }
    void this.generate();
  }

  private async generate() {
    const tab = this.ws.active();
    const root = this.ws.collection()?.root;
    if (!tab || !root) {
      this.error.set('Ouvre une requête pour générer son code.');
      return;
    }
    const mine = ++this.sequence;
    this.loading.set(true);
    this.error.set(null);
    try {
      const out = await api.generateCode(root, tab.path, tab.doc, this.ws.env(), this.language());
      if (mine !== this.sequence) return;
      this.code.set(out.code);
      this.unresolved.set(out.unresolved);
    } catch (e) {
      if (mine !== this.sequence) return;
      this.code.set('');
      this.unresolved.set([]);
      this.error.set(String(e));
    } finally {
      if (mine === this.sequence) this.loading.set(false);
    }
  }

  protected copy() {
    const code = this.code();
    if (!code) return;
    navigator.clipboard?.writeText(code).then(
      () => {
        this.ws.notify('Code copié');
        this.close();
      },
      () => this.ws.notify('Copie impossible', true),
    );
  }
}
