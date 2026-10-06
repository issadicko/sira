import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { api } from '../core/api';
import { DomainGroup, draftProblem, emptyDraft, expiryLabel, groupByDomain, keyOf, shortValue } from '../core/cookies';
import { CookieDraft, CookieKey, CookieView } from '../core/model';
import { Workspace } from '../core/store';
import { Icon } from './icon';

interface Editing {
  previous: CookieKey | null;
  draft: CookieDraft;
}

/** Section « Cookies » : le pot de l'application, par domaine. Il vit en mémoire et se vide au lancement. */
@Component({
  selector: 'app-cookies-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    <h1 class="view-title">Cookies</h1>
    <p class="set-intro">
      Le pot est partagé par toutes les collections et reste en mémoire : il est vide à chaque lancement. Une requête en envoie les cookies
      qui correspondent à son adresse, et y range ceux que les serveurs posent. Les deux se règlent dans Réseau.
    </p>

    <div class="bar">
      <button class="btn" (click)="add()"><app-ic name="plus" [size]="14" />Ajouter un cookie</button>
      <span class="spacer"></span>
      <button class="btn ghost" (click)="clear()" [disabled]="!cookies().length"><app-ic name="trash" [size]="14" />Tout effacer</button>
    </div>

    @if (editing(); as edit) {
      <form class="form" (submit)="$event.preventDefault(); save()" aria-label="Cookie">
        <div class="grid">
          <label class="f"><span>Nom</span><input class="input mono" name="key" spellcheck="false" autocomplete="off" [value]="edit.draft.key" (input)="patch({ key: $any($event.target).value })" /></label>
          <label class="f"><span>Valeur</span><input class="input mono" name="value" spellcheck="false" autocomplete="off" [value]="edit.draft.value" (input)="patch({ value: $any($event.target).value })" /></label>
          <label class="f"><span>Domaine</span><input class="input mono" name="domain" spellcheck="false" autocomplete="off" placeholder="api.example.com" [value]="edit.draft.domain" (input)="patch({ domain: $any($event.target).value })" /></label>
          <label class="f"><span>Chemin</span><input class="input mono" name="path" spellcheck="false" autocomplete="off" [value]="edit.draft.path" (input)="patch({ path: $any($event.target).value })" /></label>
          <label class="f wide"><span>Expire le (UTC)</span><input class="input mono" name="expires" spellcheck="false" autocomplete="off" placeholder="2026-12-31T23:59:59Z — vide : cookie de session" [value]="edit.draft.expires ?? ''" (input)="patch({ expires: $any($event.target).value })" /></label>
        </div>
        <div class="flags">
          <label class="flag"><input type="checkbox" class="cb" [checked]="edit.draft.secure" (change)="patch({ secure: !edit.draft.secure })" />Secure</label>
          <label class="flag"><input type="checkbox" class="cb" [checked]="edit.draft.httpOnly" (change)="patch({ httpOnly: !edit.draft.httpOnly })" />HttpOnly</label>
          <label class="flag"><input type="checkbox" class="cb" [checked]="edit.draft.hostOnly" (change)="patch({ hostOnly: !edit.draft.hostOnly })" />Cet hôte seulement</label>
        </div>
        <div class="form-actions">
          @if (problem() && touched()) {
            <span class="set-error" role="alert">{{ problem() }}</span>
          }
          <span class="spacer"></span>
          <button type="button" class="btn ghost" (click)="cancel()">Annuler</button>
          <button type="submit" class="btn-primary">{{ edit.previous ? 'Enregistrer' : 'Ajouter' }}</button>
        </div>
      </form>
    }

    @if (!loaded()) {
      <p class="none">Lecture du pot…</p>
    } @else if (!groups().length) {
      <p class="none">Aucun cookie. Ils apparaissent ici dès qu'un serveur en pose, ou quand tu en ajoutes un.</p>
    }
    @for (group of groups(); track group.domain) {
      <section class="dom" [attr.aria-label]="group.domain">
        <div class="dom-head">
          <h2 class="domain">{{ group.domain }}</h2>
          <span class="count">{{ group.cookies.length }}</span>
          <span class="spacer"></span>
          <button class="btn ghost sm" (click)="clearDomain(group)" [attr.aria-label]="'Supprimer les cookies de ' + group.domain"><app-ic name="trash" [size]="13" />Supprimer le domaine</button>
        </div>
        <ul class="list">
          @for (c of group.cookies; track c.path + c.key) {
            <li class="ck">
              <div class="main">
                <span class="name">{{ c.key }}</span>
                <span class="val" [title]="c.value">{{ short(c.value) }}</span>
              </div>
              <div class="meta">
                <span class="path">{{ c.path }}</span>
                <span class="exp" [class.gone]="label(c) === 'Expiré'">{{ label(c) }}</span>
                @if (c.secure) {
                  <span class="chip">Secure</span>
                }
                @if (c.httpOnly) {
                  <span class="chip">HttpOnly</span>
                }
              </div>
              <div class="ck-tools">
                <button class="icon-btn" (click)="edit(c)" [attr.aria-label]="'Modifier ' + c.key"><app-ic name="pencil" [size]="14" /></button>
                <button class="icon-btn" (click)="remove(c)" [attr.aria-label]="'Supprimer ' + c.key"><app-ic name="trash" [size]="14" /></button>
              </div>
            </li>
          }
        </ul>
      </section>
    }
  `,
  styles: `
    :host { display: block; max-width: 760px; }
    .bar { display: flex; align-items: center; gap: 8px; margin-bottom: 12px; }
    .spacer { flex: 1; }
    .form { border: 1px solid var(--line); border-radius: 8px; padding: 12px; margin-bottom: 16px; background: var(--sunken); }
    .grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
    .f { display: flex; flex-direction: column; gap: 4px; color: var(--muted); font-size: calc(12 * var(--px)); }
    .f.wide { grid-column: 1 / -1; }
    .flags { display: flex; gap: 16px; margin-top: 10px; }
    .flag { display: inline-flex; align-items: center; gap: 8px; cursor: pointer; }
    .form-actions { display: flex; align-items: center; gap: 8px; margin-top: 12px; }
    .none { color: var(--faint); margin: 16px 0; }
    .dom { margin-bottom: 18px; }
    .dom-head { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
    .domain { margin: 0; font-size: calc(12.5 * var(--px)); font-weight: 600; font-family: var(--font-mono); }
    .count { color: var(--faint); font-variant-numeric: tabular-nums; }
    .list { list-style: none; margin: 0; padding: 0; border: 1px solid var(--line); border-radius: 8px; }
    .ck { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: 12px; padding: 8px 12px; border-top: 1px solid var(--line); }
    .ck:first-child { border-top: 0; }
    .main { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
    .name { font-family: var(--font-mono); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .val { font-family: var(--font-mono); color: var(--muted); font-size: calc(12 * var(--px)); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .meta { display: flex; align-items: center; gap: 8px; color: var(--faint); font-size: calc(12 * var(--px)); white-space: nowrap; }
    .path { font-family: var(--font-mono); }
    .exp.gone { color: var(--bad); }
    .chip { padding: 1px 6px; border-radius: 4px; background: var(--raised); border: 1px solid var(--line); color: var(--muted); }
    .ck-tools { display: flex; gap: 2px; }
  `,
})
export class CookiesSettings {
  private readonly ws = inject(Workspace);
  protected readonly cookies = signal<CookieView[]>([]);
  protected readonly loaded = signal(false);
  protected readonly editing = signal<Editing | null>(null);
  protected readonly touched = signal(false);
  protected readonly groups = computed(() => groupByDomain(this.cookies()));
  protected readonly problem = computed(() => {
    const edit = this.editing();
    return edit ? draftProblem(edit.draft) : null;
  });
  protected readonly short = shortValue;
  private readonly now = Date.now();

  constructor() {
    void this.run(() => api.cookiesList()).then(() => this.loaded.set(true));
  }

  protected label(cookie: CookieView) {
    return expiryLabel(cookie.expires, this.now);
  }

  protected add() {
    this.touched.set(false);
    this.editing.set({ previous: null, draft: emptyDraft() });
  }

  protected edit(cookie: CookieView) {
    this.touched.set(false);
    this.editing.set({ previous: keyOf(cookie), draft: structuredClone(cookie) });
  }

  protected patch(change: Partial<CookieDraft>) {
    this.editing.update((edit) => (edit ? { ...edit, draft: { ...edit.draft, ...change } } : edit));
  }

  protected cancel() {
    this.editing.set(null);
  }

  protected async save() {
    const edit = this.editing();
    if (!edit) return;
    this.touched.set(true);
    if (draftProblem(edit.draft)) return;
    const done = await this.run(() => api.cookieSave(edit.previous, edit.draft));
    if (done) this.editing.set(null);
  }

  protected remove(cookie: CookieView) {
    void this.run(() => api.cookieDelete(keyOf(cookie)));
  }

  protected clearDomain(group: DomainGroup) {
    void this.run(() => api.cookiesDeleteDomain(group.domain));
  }

  protected clear() {
    void this.run(() => api.cookiesClear());
  }

  private async run(action: () => Promise<CookieView[]>): Promise<boolean> {
    try {
      this.cookies.set(await action());
      return true;
    } catch (e) {
      this.ws.notify(String(e), true);
      return false;
    }
  }
}
