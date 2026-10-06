import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

import { AUTH_TYPES, defaultAuth, withAuthField } from '../core/auth';
import { Auth, TokenInfo } from '../core/model';
import { Icon } from './icon';
import { OAuth2Editor } from './oauth2-editor';

/** L'auth d'une requête : son type, puis les champs de ce type. */
@Component({
  selector: 'app-auth-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, OAuth2Editor],
  host: { style: 'display: block' },
  styles: `
    .field { display: grid; grid-template-columns: 130px minmax(0, 1fr); align-items: center; gap: 10px; margin-bottom: 8px; font-size: calc(12.5 * var(--px)); color: var(--muted); }
    .field input { height: 30px; padding: 0 10px; border-radius: 6px; border: 1px solid var(--line); background: var(--sunken); font: var(--code-size) var(--code-font); outline: 0; min-width: 0; }
    .field input:focus { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .type { max-width: 320px; }
    .hint { margin: 2px 0 8px 140px; font-size: calc(11.5 * var(--px)); color: var(--faint); }
    .foot { font-size: calc(12 * var(--px)); margin: 12px 2px 0; color: var(--faint); }
  `,
  template: `
    <section class="sec">
      <div class="sec-head"><span class="sec-title">Type</span></div>
      @if (auth().type === 'other') {
        <div class="banner"><app-ic name="alert" [size]="15" /><span><b>{{ $any(auth()).label }}.</b> Ce type n'est pas encore pris en charge ; il est conservé tel quel dans le fichier.</span></div>
      } @else {
        <div class="select-wrap type">
          <select class="input" aria-label="Type d'authentification" (change)="setType($any($event.target).value)">
            @for (a of types; track a.type) {
              <option [value]="a.type" [selected]="auth().type === a.type">{{ a.label }}</option>
            }
          </select>
          <app-ic name="chev-down" [size]="14" />
        </div>
      }
    </section>
    @switch (auth().type) {
      @case ('bearer') {
        <label class="field"><span>Jeton</span><input type="text" spellcheck="false" [value]="value('token')" (input)="set('token', $event)" placeholder="{{ '{{token}}' }}" /></label>
      }
      @case ('basic') {
        <label class="field"><span>Utilisateur</span><input type="text" spellcheck="false" [value]="value('username')" (input)="set('username', $event)" /></label>
        <label class="field"><span>Mot de passe</span><input type="password" [value]="value('password')" (input)="set('password', $event)" placeholder="{{ '{{process.env.MOT_DE_PASSE}}' }}" /></label>
      }
      @case ('digest') {
        <label class="field"><span>Utilisateur</span><input type="text" spellcheck="false" [value]="value('username')" (input)="set('username', $event)" /></label>
        <label class="field"><span>Mot de passe</span><input type="password" [value]="value('password')" (input)="set('password', $event)" placeholder="{{ '{{process.env.MOT_DE_PASSE}}' }}" /></label>
        <p class="hint">La requête part d'abord sans identifiants ; sur un 401 Digest, elle est renvoyée avec la réponse au défi du serveur.</p>
      }
      @case ('apikey') {
        <label class="field"><span>Clé</span><input type="text" spellcheck="false" [value]="value('key')" (input)="set('key', $event)" /></label>
        <label class="field"><span>Valeur</span><input type="text" spellcheck="false" [value]="value('value')" (input)="set('value', $event)" /></label>
      }
      @case ('awsv4') {
        <label class="field"><span>Access Key ID</span><input type="text" spellcheck="false" [value]="value('accessKeyId')" (input)="set('accessKeyId', $event)" placeholder="{{ '{{process.env.AWS_ACCESS_KEY_ID}}' }}" /></label>
        <label class="field"><span>Secret Access Key</span><input type="password" [value]="value('secretAccessKey')" (input)="set('secretAccessKey', $event)" placeholder="{{ '{{process.env.AWS_SECRET_ACCESS_KEY}}' }}" /></label>
        <label class="field"><span>Session Token</span><input type="password" [value]="value('sessionToken')" (input)="set('sessionToken', $event)" placeholder="facultatif" /></label>
        <label class="field"><span>Service</span><input type="text" spellcheck="false" [value]="value('service')" (input)="set('service', $event)" placeholder="s3, execute-api… (déduit de l'hôte si vide)" /></label>
        <label class="field"><span>Région</span><input type="text" spellcheck="false" [value]="value('region')" (input)="set('region', $event)" placeholder="eu-west-1 (déduite de l'hôte si vide)" /></label>
        <label class="field"><span>Profil</span><input type="text" spellcheck="false" [value]="value('profileName')" (input)="set('profileName', $event)" placeholder="facultatif" /></label>
        <p class="hint">Avec un profil, les clés viennent de <span class="mono">~/.aws/credentials</span> ou <span class="mono">~/.aws/config</span> et remplacent celles saisies ici.</p>
      }
      @case ('oauth2') {
        <app-oauth2-editor
          [auth]="$any(auth())"
          [token]="token()"
          [busy]="busy()"
          [error]="error()"
          (authChange)="authChange.emit($event)"
          (fetch)="tokenFetch.emit()"
          (clear)="tokenClear.emit()"
        />
      }
      @case ('inherit') {
        <div class="note"><app-ic name="shield" [size]="14" /><span>L'auth du dossier le plus proche, sinon celle de opencollection.yml, est appliquée à l'envoi.</span></div>
      }
    }
    <p class="foot">Préfère une variable ({{ '{{token}}' }}, {{ '{{process.env.NOM}}' }}) à une valeur en clair : ce fichier est versionné.</p>
  `,
})
export class AuthEditor {
  readonly auth = input.required<Auth>();
  readonly authChange = output<Auth>();
  /** L'état du jeton OAuth 2 de la requête, et ce qu'il faut pour le demander ou l'oublier. */
  readonly token = input<TokenInfo | null>(null);
  readonly busy = input(false);
  readonly error = input<string | null>(null);
  readonly tokenFetch = output<void>();
  readonly tokenClear = output<void>();
  protected readonly types = AUTH_TYPES;

  protected value(field: string): string {
    const current = this.auth() as unknown as Record<string, unknown>;
    return typeof current[field] === 'string' ? (current[field] as string) : '';
  }

  protected set(field: string, event: Event) {
    this.authChange.emit(withAuthField(this.auth(), field, (event.target as HTMLInputElement).value));
  }

  protected setType(type: string) {
    this.authChange.emit(defaultAuth(type as Auth['type']));
  }
}
