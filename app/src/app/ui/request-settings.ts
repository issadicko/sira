import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { SettingsPatch, effectiveSettings, wholeNumber, withSettings, wsSettings } from '../core/request-settings';
import { Workspace } from '../core/store';

/** Réglages d'envoi d'une requête : délai et redirections, écrits dans le bloc `settings` du fichier. */
@Component({
  selector: 'app-request-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: 'display: block' },
  styles: `
    .field { display: grid; grid-template-columns: minmax(120px, 200px) minmax(0, 1fr); align-items: center; gap: 10px; margin-bottom: 8px; min-height: 30px; font-size: calc(12.5 * var(--px)); color: var(--muted); }
    .field input[type='text'] { width: 120px; height: 30px; padding: 0 10px; border-radius: 6px; border: 1px solid var(--line); background: var(--sunken); font: var(--code-size) var(--code-font); font-variant-numeric: tabular-nums; outline: 0; }
    .field input[type='text']:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .field input[type='text']:disabled { opacity: .5; }
    .field .unit { margin-left: 8px; color: var(--faint); }
    .hint { margin: 0 0 16px; font-size: calc(11.5 * var(--px)); color: var(--faint); }
    .field.off { opacity: .55; }
  `,
  template: `
    @if (socket(); as w) {
      <section class="sec">
        <div class="sec-head"><span class="sec-title">Connexion</span><span class="sec-meta">écrits dans le bloc settings du fichier</span></div>
        <label class="field">
          <span>Délai de connexion</span>
          <span><input type="text" inputmode="numeric" spellcheck="false" aria-label="Délai de connexion en millisecondes" [value]="w.timeoutMs" (change)="timeout($any($event.target))" /><span class="unit">ms</span></span>
        </label>
        <p class="hint">0 : 30 secondes. Le délai couvre la résolution, la connexion, TLS et la mise à niveau.</p>
        <label class="field">
          <span>Ping toutes les</span>
          <span><input type="text" inputmode="numeric" spellcheck="false" aria-label="Intervalle des pings en millisecondes" [value]="w.keepAliveMs" (change)="keepAlive($any($event.target))" /><span class="unit">ms</span></span>
        </label>
        <p class="hint">0 : aucun ping. Les pings gardent une connexion ouverte à travers les proxys qui coupent les connexions silencieuses.</p>
      </section>
    } @else if (settings(); as s) {
      <section class="sec">
        <div class="sec-head"><span class="sec-title">Envoi</span><span class="sec-meta">écrits dans le bloc settings du fichier</span></div>
        <label class="field">
          <span>Délai</span>
          <span><input type="text" inputmode="numeric" spellcheck="false" aria-label="Délai en millisecondes" [value]="s.timeoutMs" (change)="timeout($any($event.target))" /><span class="unit">ms</span></span>
        </label>
        <p class="hint">0 : pas de délai. Le délai s'applique à chaque saut de redirection.</p>
        <label class="field">
          <span>Suivre les redirections</span>
          <input type="checkbox" class="cb" [checked]="s.followRedirects" (change)="set({ followRedirects: !s.followRedirects })" aria-label="Suivre les redirections" />
        </label>
        <label class="field" [class.off]="!s.followRedirects">
          <span>Redirections au plus</span>
          <span><input type="text" inputmode="numeric" spellcheck="false" aria-label="Nombre maximal de redirections" [disabled]="!s.followRedirects" [value]="s.maxRedirects" (change)="max($any($event.target))" /></span>
        </label>
        <label class="field" [class.off]="!s.followRedirects">
          <span>Transmettre Authorization</span>
          <input type="checkbox" class="cb" [checked]="s.forwardAuthorizationHeader" [disabled]="!s.followRedirects" (change)="set({ forwardAuthorizationHeader: !s.forwardAuthorizationHeader })" aria-label="Transmettre l'en-tête Authorization aux autres origines" />
        </label>
        <p class="hint">Décoché, Authorization et Proxy-Authorization ne suivent pas une redirection vers un autre hôte, port ou schéma.</p>
      </section>
      <div class="note"><span>Suivies : 301, 302, 303, 307 et 308. Un 301, 302 ou 303 rejoue la requête en GET sans corps (HEAD reste HEAD) ; 307 et 308 gardent la méthode et le corps. La réponse qui dépasse la limite est rendue telle quelle.</span></div>
    }
  `,
})
export class RequestSettings {
  private readonly ws = inject(Workspace);
  protected readonly settings = computed(() => {
    const doc = this.ws.active()?.doc;
    return doc ? effectiveSettings(doc) : null;
  });

  protected readonly socket = computed(() => {
    const doc = this.ws.active()?.doc;
    return doc?.requestType === 'websocket' ? wsSettings(doc) : null;
  });

  protected keepAlive(input: HTMLInputElement) {
    const value = wholeNumber(input.value);
    if (value === null) input.value = String(this.socket()?.keepAliveMs ?? 0);
    else this.set({ keepAliveMs: value });
  }

  protected set(patch: SettingsPatch) {
    this.ws.edit((d) => withSettings(d, patch));
  }

  protected timeout(input: HTMLInputElement) {
    const value = wholeNumber(input.value);
    if (value === null) input.value = String(this.settings()?.timeoutMs ?? this.socket()?.timeoutMs ?? 0);
    else this.set({ timeoutMs: value });
  }

  protected max(input: HTMLInputElement) {
    const value = wholeNumber(input.value);
    if (value === null) input.value = String(this.settings()?.maxRedirects ?? 0);
    else this.set({ maxRedirects: value });
  }
}
