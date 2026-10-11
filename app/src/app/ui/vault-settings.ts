import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { VaultCheck, VaultPrefs } from '../core/model';
import { DEFAULT_VAULT } from '../core/network';
import { Workspace } from '../core/store';
import { sameVault, ttlLabel, vaultProblem } from '../core/vault';
import { Icon } from './icon';

/** Section « Vault » : où lire les secrets HashiCorp Vault qu'une requête réclame par `{{$vault.montage/chemin#champ}}`. */
@Component({
  selector: 'app-vault-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    <h1 class="view-title">HashiCorp Vault</h1>
    <p class="set-intro">
      Une variable <code>{{ '{{$vault.secret/app/db#password}}' }}</code> vaut le champ <code>password</code> du secret <code>app/db</code> du montage
      <code>secret</code>. Elle est lue à l'envoi, jamais écrite dans un fichier. Sans réglage ici, <code>VAULT_ADDR</code>, <code>VAULT_TOKEN</code>
      (ou <code>~/.vault-token</code>) et <code>VAULT_NAMESPACE</code> de l'environnement de l'application servent.
    </p>

    @if (loaded()) {
      <section class="set-group" aria-labelledby="g-vault">
        <div class="set-head"><h2 class="sec-title" id="g-vault">Serveur</h2></div>
        <div class="set-rows">
          <div class="set-row">
            <label class="set-lbl" for="vault-addr">Adresse</label>
            <div class="set-ctl wide">
              <input class="input mono" id="vault-addr" type="text" spellcheck="false" autocomplete="off" placeholder="https://vault.example.com:8200" [class.is-bad]="!!problem()" [value]="draft().addr" (input)="patch({ addr: $any($event.target).value })" />
              @if (problem(); as p) {
                <span class="set-error" role="alert">{{ p }}</span>
              }
            </div>
          </div>
          <div class="set-row">
            <label class="set-lbl" for="vault-ns">Espace de noms</label>
            <div class="set-ctl">
              <input class="input mono" id="vault-ns" type="text" spellcheck="false" autocomplete="off" placeholder="facultatif (Vault Enterprise)" [value]="draft().namespace" (input)="patch({ namespace: $any($event.target).value })" />
            </div>
          </div>
          <div class="set-row">
            <span class="set-lbl">Connexion</span>
            <div class="set-ctl wide">
              <div class="seg" role="group" aria-label="Méthode de connexion">
                <button [attr.aria-pressed]="draft().auth === 'token'" (click)="patch({ auth: 'token' })">Jeton</button>
                <button [attr.aria-pressed]="draft().auth === 'approle'" (click)="patch({ auth: 'approle' })">AppRole</button>
              </div>
            </div>
          </div>
          @if (draft().auth === 'token') {
            <div class="set-row">
              <label class="set-lbl" for="vault-token">Jeton</label>
              <div class="set-ctl">
                <input class="input mono" id="vault-token" type="password" autocomplete="new-password" [placeholder]="hint(tokenSet(), newToken())" [value]="newToken() ?? ''" (input)="newToken.set($any($event.target).value === '' ? null : $any($event.target).value)" />
                @if (tokenSet() && newToken() === null) {
                  <button class="btn ghost sm forget" (click)="newToken.set('')"><app-ic name="trash" [size]="13" />Oublier le jeton</button>
                }
                <span class="set-hint">Gardé dans le trousseau du système, jamais dans un fichier.</span>
              </div>
            </div>
          } @else {
            <div class="set-row">
              <label class="set-lbl" for="vault-role">Identifiant de rôle</label>
              <div class="set-ctl">
                <input class="input mono" id="vault-role" type="text" spellcheck="false" autocomplete="off" [value]="draft().roleId" (input)="patch({ roleId: $any($event.target).value })" />
              </div>
            </div>
            <div class="set-row">
              <label class="set-lbl" for="vault-secret-id">Secret ID</label>
              <div class="set-ctl">
                <input class="input mono" id="vault-secret-id" type="password" autocomplete="new-password" [placeholder]="hint(secretIdSet(), newSecretId())" [value]="newSecretId() ?? ''" (input)="newSecretId.set($any($event.target).value === '' ? null : $any($event.target).value)" />
                @if (secretIdSet() && newSecretId() === null) {
                  <button class="btn ghost sm forget" (click)="newSecretId.set('')"><app-ic name="trash" [size]="13" />Oublier le secret ID</button>
                }
                <span class="set-hint">Gardé dans le trousseau du système, jamais dans un fichier.</span>
              </div>
            </div>
            <div class="set-row">
              <label class="set-lbl" for="vault-mount">Montage AppRole</label>
              <div class="set-ctl">
                <input class="input mono" id="vault-mount" type="text" spellcheck="false" autocomplete="off" placeholder="approle" [value]="draft().approleMount" (input)="patch({ approleMount: $any($event.target).value })" />
              </div>
            </div>
          }
        </div>
      </section>

      <section class="set-group" aria-labelledby="g-check">
        <div class="set-head"><h2 class="sec-title" id="g-check">Vérification</h2></div>
        <div class="set-rows">
          <div class="set-row">
            <span class="set-lbl">Connexion</span>
            <div class="set-ctl wide">
              <button class="btn" (click)="check()" [disabled]="checking() || dirty()" [title]="dirty() ? 'Enregistre d’abord les modifications' : ''">{{ checking() ? 'Vérification…' : 'Tester la connexion' }}</button>
              @if (result(); as r) {
                <span class="set-hint ok" role="status">Connecté : {{ r.displayName || 'jeton' }} · politiques : {{ r.policies.join(', ') || 'aucune' }} · expire : {{ ttl(r.ttl) }}</span>
              }
              @if (failure(); as f) {
                <span class="set-error" role="alert">{{ f }}</span>
              }
            </div>
          </div>
        </div>
      </section>

      <div class="set-bar">
        @if (dirty()) {
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
    .set-ctl.wide { max-width: none; }
    .input.is-bad { border-color: var(--bad); }
    .set-hint.ok { color: var(--good); }
    .forget, .seg { align-self: flex-start; }
    .set-bar { position: sticky; bottom: -32px; display: flex; align-items: center; gap: 10px; margin-top: -8px; padding: 12px 0 14px; background: var(--island); border-top: 1px solid var(--line); }
    .spacer { flex: 1; }
  `,
})
export class VaultSettings {
  private readonly ws = inject(Workspace);
  protected readonly loaded = signal(false);
  protected readonly saving = signal(false);
  protected readonly checking = signal(false);
  private readonly saved = signal<VaultPrefs>(DEFAULT_VAULT);
  protected readonly draft = signal<VaultPrefs>(DEFAULT_VAULT);
  protected readonly tokenSet = signal(false);
  protected readonly secretIdSet = signal(false);
  /** `null` : la valeur gardée reste en place ; une chaîne vide l'oublie ; un texte la remplace. */
  protected readonly newToken = signal<string | null>(null);
  protected readonly newSecretId = signal<string | null>(null);
  protected readonly result = signal<VaultCheck | null>(null);
  protected readonly failure = signal<string | null>(null);
  protected readonly problem = computed(() => vaultProblem(this.draft()));
  protected readonly dirty = computed(() => !sameVault(this.saved(), this.draft()) || this.newToken() !== null || this.newSecretId() !== null);
  protected ttl = ttlLabel;

  constructor() {
    void this.load();
  }

  private async load() {
    try {
      this.accept(await api.vaultGet());
      this.loaded.set(true);
    } catch (e) {
      this.ws.notify(`Les réglages Vault n'ont pas pu être lus : ${String(e)}`, true);
    }
  }

  private accept(view: { prefs: VaultPrefs; tokenSet: boolean; secretIdSet: boolean }) {
    this.saved.set(view.prefs);
    this.draft.set(structuredClone(view.prefs));
    this.tokenSet.set(view.tokenSet);
    this.secretIdSet.set(view.secretIdSet);
    this.newToken.set(null);
    this.newSecretId.set(null);
  }

  protected hint(isSet: boolean, change: string | null): string {
    return change === '' ? 'Sera oublié à l’enregistrement' : isSet ? 'Enregistré dans le trousseau' : '';
  }

  protected patch(change: Partial<VaultPrefs>) {
    this.draft.update((prefs) => ({ ...prefs, ...change }));
    this.result.set(null);
    this.failure.set(null);
  }

  protected cancel() {
    this.draft.set(structuredClone(this.saved()));
    this.newToken.set(null);
    this.newSecretId.set(null);
  }

  protected async save() {
    this.saving.set(true);
    try {
      this.accept(await api.vaultSave(this.draft(), this.newToken(), this.newSecretId()));
      this.ws.notify('Réglages Vault enregistrés');
    } catch (e) {
      this.ws.notify(`Les réglages Vault n'ont pas pu être enregistrés : ${String(e)}`, true);
    } finally {
      this.saving.set(false);
    }
  }

  protected async check() {
    this.checking.set(true);
    this.result.set(null);
    this.failure.set(null);
    try {
      this.result.set(await api.vaultCheck(this.ws.collection()?.root ?? null));
    } catch (e) {
      this.failure.set(String(e));
    } finally {
      this.checking.set(false);
    }
  }
}
