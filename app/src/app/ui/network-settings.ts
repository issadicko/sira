import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { NetworkPrefs, ProxyConfig, ProxyMode } from '../core/model';
import {
  DEFAULT_NETWORK,
  PROXY_MODES,
  PROXY_PROTOCOLS,
  fileName,
  portProblem,
  proxyProblem,
  sameNetwork,
  withProxy,
  withProxyMode,
} from '../core/network';
import { Workspace } from '../core/store';
import { Icon } from './icon';

/** Section « Réseau » : vérification TLS, autorité de certification et proxy de l'application. Enregistrés à la demande. */
@Component({
  selector: 'app-network-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    <h1 class="view-title">Réseau</h1>
    <p class="set-intro">
      Ces réglages valent pour tous les envois de l'application, dans toutes les collections. Une collection peut imposer son
      proxy et ses certificats client dans <code>opencollection.yml</code> : ils l'emportent.
    </p>

    @if (loaded()) {
      <section class="set-group" aria-labelledby="g-tls">
        <div class="set-head"><h2 class="sec-title" id="g-tls">Certificats des serveurs</h2></div>
        <div class="set-rows">
          <label class="set-row">
            <span class="set-lbl">Vérification</span>
            <span class="set-ctl check">
              <span class="check-line">
                <input type="checkbox" class="cb" [checked]="draft().verifyTls" (change)="patch({ verifyTls: !draft().verifyTls })" />
                Vérifier le certificat et le nom d'hôte du serveur
              </span>
            </span>
          </label>
          <div class="set-row">
            <span class="set-lbl" id="ca-label">Autorités de confiance</span>
            <div class="set-ctl wide">
              <div class="file-line">
                <span class="input mono file-name" role="textbox" aria-readonly="true" aria-labelledby="ca-label" [class.is-empty]="!draft().caFile" [title]="draft().caFile ?? ''">
                  {{ draft().caFile ? name(draft().caFile!) : 'Celles du système seulement' }}
                </span>
                <button class="btn" (click)="pickCa()" [disabled]="!draft().verifyTls">Choisir un fichier PEM…</button>
                @if (draft().caFile) {
                  <button class="btn ghost" (click)="patch({ caFile: null })" aria-label="Retirer le fichier d'autorités"><app-ic name="x" [size]="14" /></button>
                }
              </div>
              @if (draft().caFile) {
                <label class="check-line">
                  <input type="checkbox" class="cb" [checked]="draft().keepDefaultRoots" [disabled]="!draft().verifyTls" (change)="patch({ keepDefaultRoots: !draft().keepDefaultRoots })" />
                  Garder aussi les autorités du système
                </label>
              }
              <span class="set-hint">Les autorités du fichier s'ajoutent à celles du système. Décoché, seules celles du fichier font confiance.</span>
            </div>
          </div>
        </div>
        @if (!draft().verifyTls) {
          <div class="banner err" role="alert">
            <app-ic name="alert" [size]="15" />
            <span><b>La vérification est désactivée.</b> N'importe quel serveur sera accepté, y compris un faux : réserve ce réglage aux serveurs de test.</span>
          </div>
        }
      </section>

      <section class="set-group" aria-labelledby="g-proxy">
        <div class="set-head"><h2 class="sec-title" id="g-proxy">Proxy</h2></div>
        <div class="set-rows">
          <div class="set-row">
            <span class="set-lbl">Source</span>
            <div class="set-ctl wide">
              <div class="seg" role="group" aria-label="Source du proxy">
                @for (m of modes; track m.id) {
                  <button [attr.aria-pressed]="draft().proxy.mode === m.id" (click)="mode(m.id)">{{ m.label }}</button>
                }
              </div>
              @switch (draft().proxy.mode) {
                @case ('system') {
                  <span class="set-hint">
                    Variables <code>HTTP_PROXY</code>, <code>HTTPS_PROXY</code>, <code>ALL_PROXY</code> et <code>NO_PROXY</code> de l'environnement de l'application.
                    <code>localhost</code> et l'adresse locale ne passent jamais par le proxy.
                  </span>
                }
                @case ('off') {
                  <span class="set-hint">Aucun proxy, sauf celui qu'une collection impose.</span>
                }
              }
            </div>
          </div>
          @if (draft().proxy.mode === 'manual') {
            @let cfg = draft().proxy.config;
            <div class="set-row">
              <label class="set-lbl" for="proxy-protocol">Protocole</label>
              <div class="select-wrap narrow">
                <select class="input" id="proxy-protocol" (change)="proxy({ protocol: $any($event.target).value })">
                  @for (p of protocols; track p) {
                    <option [value]="p" [selected]="cfg.protocol === p">{{ p }}</option>
                  }
                </select>
                <app-ic name="chev-down" [size]="14" />
              </div>
            </div>
            <div class="set-row">
              <label class="set-lbl" for="proxy-host">Hôte</label>
              <div class="set-ctl">
                <input class="input mono" id="proxy-host" type="text" spellcheck="false" autocomplete="off" placeholder="proxy.example.com" [value]="cfg.hostname" (input)="proxy({ hostname: $any($event.target).value })" />
              </div>
            </div>
            <div class="set-row">
              <label class="set-lbl" for="proxy-port">Port</label>
              <div class="set-ctl">
                <input class="input mono port" id="proxy-port" type="text" inputmode="numeric" spellcheck="false" autocomplete="off" placeholder="8080" [class.is-bad]="!!port()" [attr.aria-invalid]="!!port()" [value]="cfg.port" (input)="proxy({ port: $any($event.target).value })" />
                @if (port()) {
                  <span class="set-error" role="alert">{{ port() }}</span>
                }
              </div>
            </div>
            <div class="set-row">
              <span class="set-lbl">Authentification</span>
              <label class="set-ctl check">
                <span class="check-line">
                  <input type="checkbox" class="cb" [checked]="!cfg.authDisabled" (change)="proxy({ authDisabled: !cfg.authDisabled })" />
                  Le proxy demande un identifiant
                </span>
              </label>
            </div>
            @if (!cfg.authDisabled) {
              <div class="set-row">
                <label class="set-lbl" for="proxy-user">Identifiant</label>
                <div class="set-ctl">
                  <input class="input" id="proxy-user" type="text" spellcheck="false" autocomplete="off" [value]="cfg.username" (input)="proxy({ username: $any($event.target).value })" />
                </div>
              </div>
              <div class="set-row">
                <label class="set-lbl" for="proxy-password">Mot de passe</label>
                <div class="set-ctl">
                  <input class="input" id="proxy-password" type="password" autocomplete="new-password" [placeholder]="passwordHint()" [value]="newPassword() ?? ''" (input)="typePassword($any($event.target).value)" />
                  @if (passwordSet() && newPassword() === null) {
                    <button class="btn ghost sm forget" (click)="newPassword.set('')"><app-ic name="trash" [size]="13" />Oublier le mot de passe</button>
                  }
                  <span class="set-hint">Gardé dans le trousseau du système, jamais dans un fichier.</span>
                </div>
              </div>
            }
            <div class="set-row">
              <label class="set-lbl" for="proxy-bypass">Sans proxy</label>
              <div class="set-ctl wide">
                <input class="input mono" id="proxy-bypass" type="text" spellcheck="false" autocomplete="off" placeholder="localhost, *.corp.local" [value]="cfg.bypassProxy" (input)="proxy({ bypassProxy: $any($event.target).value })" />
                <span class="set-hint">Hôtes joints directement, séparés par des virgules. <code>*</code> : tous ; <code>*.corp.local</code> : ses sous-domaines ; <code>hôte:port</code> : ce port seulement.</span>
              </div>
            </div>
          }
        </div>
      </section>

      <div class="set-bar">
        @if (problem(); as p) {
          <span class="set-error" role="alert">{{ p }}</span>
        } @else if (dirty()) {
          <span class="set-hint">Modifications non enregistrées</span>
        }
        <span class="spacer"></span>
        <button class="btn ghost" (click)="cancel()" [disabled]="!dirty() || saving()">Annuler</button>
        <button class="btn-primary" (click)="save()" [disabled]="!dirty() || !!problem() || saving()">Enregistrer</button>
      </div>
    }
  `,
  styles: `
    :host { display: block; max-width: 640px; }
    code { font-family: var(--font-mono); font-size: calc(11.5 * var(--px)); color: var(--muted); }
    .check-line { display: inline-flex; align-items: center; gap: 8px; min-height: 30px; cursor: pointer; }
    .set-ctl.wide { max-width: none; }
    .set-ctl.check { max-width: none; }
    .select-wrap.narrow { max-width: 160px; }
    .input.port { width: 110px; }
    .input.is-bad { border-color: var(--bad); }
    .file-line { display: flex; align-items: center; gap: 8px; min-width: 0; }
    .file-name { flex: 1; display: flex; align-items: center; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .file-name.is-empty { color: var(--faint); font-family: var(--font-ui); }
    .forget, .seg { align-self: flex-start; }
    .banner { margin-top: 10px; }
    .set-bar { position: sticky; bottom: -32px; display: flex; align-items: center; gap: 10px; margin-top: -8px; padding: 12px 0 14px; background: var(--island); border-top: 1px solid var(--line); }
    .spacer { flex: 1; }
  `,
})
export class NetworkSettings {
  private readonly ws = inject(Workspace);
  protected readonly modes = PROXY_MODES;
  protected readonly protocols = PROXY_PROTOCOLS;
  protected readonly loaded = signal(false);
  protected readonly saving = signal(false);
  private readonly saved = signal<NetworkPrefs>(DEFAULT_NETWORK);
  protected readonly draft = signal<NetworkPrefs>(DEFAULT_NETWORK);
  protected readonly passwordSet = signal(false);
  /** `null` : le mot de passe gardé reste en place ; une chaîne vide l'oublie ; un texte le remplace. */
  protected readonly newPassword = signal<string | null>(null);
  protected readonly port = computed(() => portProblem(this.draft().proxy.config.port));
  protected readonly problem = computed(() => proxyProblem(this.draft()));
  protected readonly dirty = computed(() => !sameNetwork(this.saved(), this.draft()) || this.newPassword() !== null);
  protected readonly passwordHint = computed(() =>
    this.newPassword() === '' ? 'Sera oublié à l’enregistrement' : this.passwordSet() ? 'Enregistré dans le trousseau' : '',
  );
  protected readonly name = fileName;

  constructor() {
    void this.load();
  }

  private async load() {
    try {
      this.accept(await api.networkGet());
      this.loaded.set(true);
    } catch (e) {
      this.ws.notify(`Les réglages réseau n'ont pas pu être lus : ${String(e)}`, true);
    }
  }

  private accept(view: { prefs: NetworkPrefs; proxyPasswordSet: boolean }) {
    this.saved.set(view.prefs);
    this.draft.set(structuredClone(view.prefs));
    this.passwordSet.set(view.proxyPasswordSet);
    this.newPassword.set(null);
  }

  protected patch(change: Partial<NetworkPrefs>) {
    this.draft.update((prefs) => ({ ...prefs, ...change }));
  }

  protected mode(mode: ProxyMode) {
    this.draft.update((prefs) => withProxyMode(prefs, mode));
  }

  protected proxy(change: Partial<ProxyConfig>) {
    this.draft.update((prefs) => withProxy(prefs, change));
  }

  protected typePassword(value: string) {
    this.newPassword.set(value === '' ? null : value);
  }

  protected async pickCa() {
    const file = await api.pickCertificate();
    if (file) this.patch({ caFile: file });
  }

  protected cancel() {
    this.draft.set(structuredClone(this.saved()));
    this.newPassword.set(null);
  }

  protected async save() {
    this.saving.set(true);
    try {
      this.accept(await api.networkSave(this.draft(), this.newPassword()));
      this.ws.notify('Réglages réseau enregistrés');
    } catch (e) {
      this.ws.notify(`Les réglages réseau n'ont pas pu être enregistrés : ${String(e)}`, true);
    } finally {
      this.saving.set(false);
    }
  }
}
