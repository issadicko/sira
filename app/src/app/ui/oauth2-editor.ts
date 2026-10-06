import { ChangeDetectionStrategy, Component, DestroyRef, computed, inject, input, output, signal } from '@angular/core';

import { OAUTH_FLOWS, addOAuthParam, describeToken, oauthFields, patchOAuthParam, removeOAuthParam, withAuthField } from '../core/auth';
import { OAuth2Auth, OAuthParam, TokenInfo } from '../core/model';
import { Icon } from './icon';

const STAGES: { value: OAuthParam['stage']; label: string }[] = [
  { value: 'authorization', label: 'Autorisation' },
  { value: 'token', label: 'Jeton' },
  { value: 'refresh', label: 'Rafraîchissement' },
];
const PLACEMENTS: { value: OAuthParam['placement']; label: string }[] = [
  { value: 'body', label: 'Corps' },
  { value: 'header', label: 'En-tête' },
  { value: 'query', label: 'Adresse' },
];

/** La configuration OAuth 2.0 d'une requête, et l'état du jeton qu'elle a obtenu. */
@Component({
  selector: 'app-oauth2-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: block; container-type: inline-size' },
  styles: `
    .token { display: grid; grid-template-columns: 8px minmax(0, 1fr) auto; align-items: center; gap: 8px 10px; padding: 9px 10px 9px 12px; margin-bottom: 16px; border-radius: 8px; background: var(--sunken); border: 1px solid var(--line); }
    .token .dot { width: 8px; height: 8px; border-radius: 50%; flex: none; background: var(--faint); }
    .token.ok .dot { background: var(--good); box-shadow: 0 0 0 3px var(--good-soft); }
    .token.expired .dot { background: var(--warn); box-shadow: 0 0 0 3px var(--warn-soft); }
    .token .acts { display: flex; gap: 4px; }
    .token .say { min-width: 0; font-size: calc(12.5 * var(--px)); color: var(--muted); }
    .token.ok .say { color: var(--ink); }
    .fail { margin: -8px 0 16px; }
    .grp { margin: 18px 0 8px; font-size: calc(11.5 * var(--px)); font-weight: 600; letter-spacing: .04em; text-transform: uppercase; color: var(--faint); }
    .field { display: grid; grid-template-columns: 150px minmax(0, 1fr); align-items: center; gap: 10px; margin-bottom: 8px; font-size: calc(12.5 * var(--px)); color: var(--muted); }
    .field input[type='text'], .field input[type='password'] { height: 30px; padding: 0 10px; border-radius: 6px; border: 1px solid var(--line); background: var(--sunken); font: var(--code-size) var(--code-font); outline: 0; min-width: 0; }
    .field input:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .field .select-wrap { max-width: 280px; }
    .check { display: flex; align-items: center; gap: 9px; margin: 0 0 8px 160px; font-size: calc(12.5 * var(--px)); color: var(--ink); cursor: pointer; }
    .hint { margin: 2px 0 8px 160px; font-size: calc(11.5 * var(--px)); color: var(--faint); }
    .phead, .param { display: grid; grid-template-columns: 124px minmax(0, 1fr) minmax(0, 1fr) 96px 28px; gap: 6px; align-items: center; margin-bottom: 6px; }
    .param input { height: 28px; padding: 0 8px; border-radius: 6px; border: 1px solid var(--line); background: var(--sunken); font: var(--code-size) var(--code-font); outline: 0; min-width: 0; }
    .param input:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .param .select-wrap select.input { height: 28px; padding-left: 8px; font-size: calc(12 * var(--px)); }
    .phead { margin-bottom: 2px; font-size: calc(11.5 * var(--px)); color: var(--faint); padding: 0 2px; }
    .hint code { overflow-wrap: anywhere; }
    .add { margin-top: 8px; }
    @container (max-width: 560px) {
      .token { grid-template-columns: 8px minmax(0, 1fr); }
      .token .acts { grid-column: 2; }
      .field { grid-template-columns: minmax(0, 1fr); gap: 4px; margin-bottom: 12px; }
      .field .select-wrap { max-width: none; }
      .check, .hint { margin-left: 0; }
      .phead { display: none; }
      .param { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 28px; grid-template-areas: 'stage place del' 'name value value'; margin-bottom: 12px; padding-top: 12px; border-top: 1px solid var(--line); }
      .param > :nth-child(1) { grid-area: stage; }
      .param > :nth-child(2) { grid-area: name; }
      .param > :nth-child(3) { grid-area: value; }
      .param > :nth-child(4) { grid-area: place; }
      .param > :nth-child(5) { grid-area: del; }
    }
  `,
  template: `
    <div class="token" [class.ok]="state().tone === 'ok'" [class.expired]="state().tone === 'expired'" role="status">
      <span class="dot" aria-hidden="true"></span>
      <span class="say">{{ state().text }}</span>
      <span class="acts">
        <button class="btn sm" type="button" [disabled]="busy()" (click)="fetch.emit()">
          {{ busy() ? 'En cours…' : interactive() ? 'Se connecter' : 'Obtenir un token' }}
        </button>
        @if (token()) {
          <button class="btn sm ghost" type="button" [disabled]="busy()" (click)="clear.emit()">Oublier</button>
        }
      </span>
    </div>
    @if (error(); as message) {
      <div class="banner err fail"><app-ic name="alert" [size]="15" /><span>{{ message }}</span></div>
    }

    <label class="field">
      <span>Flux</span>
      <span class="select-wrap">
        <select class="input" aria-label="Flux OAuth 2" (change)="set('flow', $any($event.target).value)">
          @for (f of flows; track f.value) {
            <option [value]="f.value" [selected]="auth().flow === f.value">{{ f.label }}</option>
          }
        </select>
        <app-ic name="chev-down" [size]="14" />
      </span>
    </label>

    <p class="grp">Serveur d'autorisation</p>
    @if (fields().authorizationUrl) {
      <label class="field"><span>URL d'autorisation</span><input type="text" spellcheck="false" [value]="auth().authorizationUrl" (input)="text('authorizationUrl', $event)" placeholder="{{ '{{auth_url}}' }}/authorize" /></label>
    }
    @if (fields().accessTokenUrl) {
      <label class="field"><span>URL du jeton</span><input type="text" spellcheck="false" [value]="auth().accessTokenUrl" (input)="text('accessTokenUrl', $event)" placeholder="{{ '{{auth_url}}' }}/token" /></label>
      <label class="field"><span>URL de rafraîchissement</span><input type="text" spellcheck="false" [value]="auth().refreshTokenUrl" (input)="text('refreshTokenUrl', $event)" placeholder="facultatif · l'URL du jeton par défaut" /></label>
    }
    @if (fields().callbackUrl) {
      <label class="field"><span>URL de rappel</span><input type="text" spellcheck="false" [value]="auth().callbackUrl" (input)="text('callbackUrl', $event)" placeholder="http://localhost/callback" /></label>
      <p class="hint">Le serveur renvoie l'utilisateur à cette adresse après la connexion ; l'application l'intercepte, rien n'a besoin d'y écouter.</p>
    }

    <p class="grp">Identifiants</p>
    <label class="field"><span>Client ID</span><input type="text" spellcheck="false" [value]="auth().clientId" (input)="text('clientId', $event)" placeholder="{{ '{{client_id}}' }}" /></label>
    @if (fields().clientSecret) {
      <label class="field"><span>Client secret</span><input type="password" [value]="auth().clientSecret" (input)="text('clientSecret', $event)" placeholder="{{ '{{process.env.CLIENT_SECRET}}' }}" /></label>
      <label class="field">
        <span>Envoi des identifiants</span>
        <span class="select-wrap">
          <select class="input" aria-label="Envoi des identifiants" (change)="set('credentialsPlacement', $any($event.target).value)">
            <option value="basic_auth_header" [selected]="auth().credentialsPlacement !== 'body'">En-tête Basic</option>
            <option value="body" [selected]="auth().credentialsPlacement === 'body'">Corps de la requête</option>
          </select>
          <app-ic name="chev-down" [size]="14" />
        </span>
      </label>
    }
    @if (fields().owner) {
      <label class="field"><span>Utilisateur</span><input type="text" spellcheck="false" [value]="auth().username" (input)="text('username', $event)" /></label>
      <label class="field"><span>Mot de passe</span><input type="password" [value]="auth().password" (input)="text('password', $event)" placeholder="{{ '{{process.env.MOT_DE_PASSE}}' }}" /></label>
    }
    <label class="field"><span>Scope</span><input type="text" spellcheck="false" [value]="auth().scope" (input)="text('scope', $event)" placeholder="read write · séparés par des espaces" /></label>
    @if (fields().state) {
      <label class="field"><span>State</span><input type="text" spellcheck="false" [value]="auth().state" (input)="text('state', $event)" placeholder="aléatoire si vide" /></label>
    }
    @if (fields().pkce) {
      <label class="check"><input type="checkbox" class="cb" [checked]="auth().pkce" (change)="flag('pkce', $event)" />PKCE (S256)</label>
    }

    <p class="grp">Jeton</p>
    <label class="field"><span>Nom</span><input type="text" spellcheck="false" [value]="auth().tokenId" (input)="text('tokenId', $event)" placeholder="credentials" /></label>
    <p class="hint">Les scripts et les requêtes le lisent par <code class="mono">{{ '{{$oauth2.' + (auth().tokenId || 'credentials') + '.access_token}}' }}</code>.</p>
    <label class="field">
      <span>Envoyer dans</span>
      <span class="select-wrap">
        <select class="input" aria-label="Où placer le jeton" (change)="set('tokenPlacement', $any($event.target).value)">
          <option value="header" [selected]="auth().tokenPlacement !== 'query'">En-tête Authorization</option>
          <option value="query" [selected]="auth().tokenPlacement === 'query'">Paramètre d'adresse</option>
        </select>
        <app-ic name="chev-down" [size]="14" />
      </span>
    </label>
    @if (auth().tokenPlacement === 'query') {
      <label class="field"><span>Nom du paramètre</span><input type="text" spellcheck="false" [value]="auth().tokenQueryKey" (input)="text('tokenQueryKey', $event)" placeholder="access_token" /></label>
    } @else {
      <label class="field"><span>Préfixe</span><input type="text" spellcheck="false" [value]="auth().tokenPrefix" (input)="text('tokenPrefix', $event)" placeholder="Bearer" /></label>
    }
    <label class="field">
      <span>Valeur envoyée</span>
      <span class="select-wrap">
        <select class="input" aria-label="Jeton à envoyer" (change)="set('tokenSource', $any($event.target).value)">
          <option value="access_token" [selected]="auth().tokenSource !== 'id_token'">access_token</option>
          <option value="id_token" [selected]="auth().tokenSource === 'id_token'">id_token</option>
        </select>
        <app-ic name="chev-down" [size]="14" />
      </span>
    </label>
    <label class="check"><input type="checkbox" class="cb" [checked]="auth().autoFetchToken" (change)="flag('autoFetchToken', $event)" />Obtenir le jeton automatiquement à l'envoi</label>
    <label class="check"><input type="checkbox" class="cb" [checked]="auth().autoRefreshToken" (change)="flag('autoRefreshToken', $event)" />Le rafraîchir à l'expiration quand c'est possible</label>

    <p class="grp">Paramètres additionnels</p>
    @if (auth().parameters.length) {
      <div class="phead"><span>Requête</span><span>Nom</span><span>Valeur</span><span>Dans</span><span></span></div>
      @for (p of auth().parameters; track $index) {
        <div class="param">
          <span class="select-wrap">
            <select class="input" aria-label="Requête du flux" (change)="patch($index, { stage: $any($event.target).value })">
              @for (s of stages; track s.value) {
                <option [value]="s.value" [selected]="p.stage === s.value">{{ s.label }}</option>
              }
            </select>
            <app-ic name="chev-down" [size]="12" />
          </span>
          <input type="text" spellcheck="false" [value]="p.name" (input)="patch($index, { name: $any($event.target).value })" aria-label="Nom du paramètre" placeholder="audience" />
          <input type="text" spellcheck="false" [value]="p.value" (input)="patch($index, { value: $any($event.target).value })" aria-label="Valeur du paramètre" />
          <span class="select-wrap">
            <select class="input" aria-label="Emplacement du paramètre" (change)="patch($index, { placement: $any($event.target).value })">
              @for (l of placements; track l.value) {
                <option [value]="l.value" [selected]="p.placement === l.value">{{ l.label }}</option>
              }
            </select>
            <app-ic name="chev-down" [size]="12" />
          </span>
          <button class="icon-btn" type="button" aria-label="Retirer le paramètre" (click)="remove($index)"><app-ic name="x" [size]="13" /></button>
        </div>
      }
    }
    <button class="btn sm ghost add" type="button" (click)="add()"><app-ic name="plus" [size]="13" />Ajouter un paramètre</button>
  `,
})
export class OAuth2Editor {
  readonly auth = input.required<OAuth2Auth>();
  readonly token = input<TokenInfo | null>(null);
  readonly busy = input(false);
  readonly error = input<string | null>(null);
  readonly authChange = output<OAuth2Auth>();
  readonly fetch = output<void>();
  readonly clear = output<void>();

  protected readonly flows = OAUTH_FLOWS;
  protected readonly stages = STAGES;
  protected readonly placements = PLACEMENTS;

  private readonly now = signal(Date.now());
  protected readonly fields = computed(() => oauthFields(this.auth().flow));
  protected readonly interactive = computed(() => this.auth().flow === 'authorization_code' || this.auth().flow === 'implicit');
  protected readonly state = computed(() => describeToken(this.token(), this.now()));

  constructor() {
    const tick = setInterval(() => this.now.set(Date.now()), 15_000);
    inject(DestroyRef).onDestroy(() => clearInterval(tick));
  }

  private emit(auth: OAuth2Auth) {
    this.authChange.emit(auth);
  }

  protected set(field: string, value: string) {
    this.emit(withAuthField(this.auth(), field, value) as OAuth2Auth);
  }

  protected text(field: string, event: Event) {
    this.set(field, (event.target as HTMLInputElement).value);
  }

  protected flag(field: string, event: Event) {
    this.emit(withAuthField(this.auth(), field, (event.target as HTMLInputElement).checked) as OAuth2Auth);
  }

  protected add() {
    this.emit(addOAuthParam(this.auth()));
  }

  protected patch(index: number, change: Partial<OAuthParam>) {
    this.emit(patchOAuthParam(this.auth(), index, change));
  }

  protected remove(index: number) {
    this.emit(removeOAuthParam(this.auth(), index));
  }
}
