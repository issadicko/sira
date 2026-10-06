import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

import { blankVar, toggledSecret } from '../core/env-ops';
import { EnvVar } from '../core/model';
import { Icon } from './icon';

/**
 * Variables d'un environnement, éditables. La valeur d'un secret se saisit ici mais vit dans le trousseau du système : elle n'est jamais dans
 * le fichier, et l'interface ne la relit jamais (elle sait seulement qu'une valeur est gardée). La dernière ligne fantôme crée une variable dès la
 * première frappe.
 */
@Component({
  selector: 'app-env-table',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    <div class="kv cols-3 env-kv" role="group" aria-label="Variables de l'environnement">
      <div class="kv-row head"><span class="kv-cell"></span><span class="kv-cell">Nom</span><span class="kv-cell">Valeur</span></div>
      @for (row of rows(); track $index; let i = $index) {
        <div class="kv-row" [class.off]="!row.enabled" [class.is-picked]="picked() === row.name" [class.is-bad]="!!problems()[i]" (focusin)="pick.emit(row.name)">
          <span class="kv-cell"><input type="checkbox" class="cb" [checked]="row.enabled" (change)="patch(i, { enabled: !row.enabled })" [attr.aria-label]="'Activer ' + row.name" /></span>
          <span class="kv-cell m-cell">
            <input
              type="text"
              spellcheck="false"
              autocomplete="off"
              [value]="row.name"
              (input)="patch(i, { name: $any($event.target).value })"
              aria-label="Nom de la variable"
              [attr.aria-invalid]="!!problems()[i]"
              [attr.title]="problems()[i]"
            />
          </span>
          <span class="kv-cell m-cell">
            @if (row.secret) {
              @if (row.value === '') {
                <span class="secret erase"><app-ic name="trash" [size]="13" />Effacée du trousseau à l'enregistrement</span>
                <button class="btn ghost sm" type="button" (click)="patch(i, { value: null })">Annuler</button>
              } @else {
                <input
                  type="password"
                  autocomplete="new-password"
                  spellcheck="false"
                  [value]="row.value ?? ''"
                  (input)="patch(i, { value: $any($event.target).value || null })"
                  [attr.aria-label]="'Valeur du secret ' + row.name"
                  [placeholder]="isStored(row.name) ? '•••••••• gardée dans le trousseau · saisir pour remplacer' : 'Valeur · gardée dans le trousseau, hors fichier'"
                />
                @if (isStored(row.name) && row.value == null) {
                  <button class="icon-btn sm" type="button" (click)="patch(i, { value: '' })" [attr.aria-label]="'Effacer la valeur de ' + row.name + ' du trousseau'" title="Effacer la valeur du trousseau"><app-ic name="trash" [size]="13" /></button>
                }
              }
            } @else {
              <input type="text" spellcheck="false" autocomplete="off" [value]="row.value ?? ''" (input)="patch(i, { value: $any($event.target).value })" [attr.aria-label]="'Valeur de ' + row.name" />
              @if (row.dataType) {
                <span class="tag" [title]="'Valeur de type ' + row.dataType + ', conservé à l\\'enregistrement'">{{ row.dataType }}</span>
              }
            }
            <button
              class="icon-btn sm lock"
              type="button"
              [class.on]="row.secret"
              [attr.aria-pressed]="row.secret"
              (click)="toggleSecret(i)"
              [attr.aria-label]="'Secret : ' + row.name"
              [attr.title]="row.secret ? 'Secret : la valeur reste dans le trousseau, jamais dans le fichier' : 'Marquer comme secret : la valeur ira dans le trousseau, hors fichier'"
            ><app-ic name="lock" [size]="13" /></button>
            <button class="icon-btn sm row-x" (click)="remove(i)" [attr.aria-label]="'Supprimer ' + row.name"><app-ic name="x" [size]="13" /></button>
          </span>
        </div>
      }
      <div class="kv-row ghost">
        <span class="kv-cell"><app-ic name="plus" [size]="13" /></span>
        <span class="kv-cell m-cell"><input type="text" spellcheck="false" autocomplete="off" placeholder="Ajouter une variable" (input)="add($event)" aria-label="Ajouter une variable" /></span>
        <span class="kv-cell"></span>
      </div>
    </div>
  `,
  styles: `
    .row-x { opacity: 0; flex-shrink: 0; }
    .kv-row:hover .row-x, .row-x:focus-visible { opacity: 1; }
    .lock { margin-left: auto; flex-shrink: 0; opacity: 0; color: var(--faint); }
    .lock.on { opacity: 1; color: var(--accent); }
    .kv-row:hover .lock, .lock:focus-visible { opacity: 1; }
    .secret.erase { color: var(--warn); }
  `,
})
export class EnvTable {
  readonly rows = input.required<EnvVar[]>();
  /** Ce qui empêche d'enregistrer chaque ligne, `null` quand elle convient. */
  readonly problems = input<(string | null)[]>([]);
  /** Nom de la variable dont la résolution est affichée à côté. */
  readonly picked = input<string | null>(null);
  /** Secrets dont le trousseau garde une valeur. */
  readonly stored = input<string[]>([]);
  readonly rowsChange = output<EnvVar[]>();
  readonly pick = output<string>();

  protected patch(i: number, change: Partial<EnvVar>) {
    this.rowsChange.emit(this.rows().map((row, j) => (j === i ? { ...row, ...change } : row)));
  }

  protected isStored(name: string) {
    return this.stored().includes(name);
  }

  protected toggleSecret(i: number) {
    this.rowsChange.emit(this.rows().map((row, j) => (j === i ? toggledSecret(row) : row)));
  }

  protected remove(i: number) {
    this.rowsChange.emit(this.rows().filter((_, j) => j !== i));
  }

  protected add(event: Event) {
    const el = event.target as HTMLInputElement;
    const name = el.value;
    el.value = '';
    this.rowsChange.emit([...this.rows(), blankVar(name)]);
    setTimeout(() => {
      const inputs = el.closest('.kv')?.querySelectorAll<HTMLInputElement>('.kv-row:not(.ghost):not(.head) .m-cell input[type=text]');
      const last = inputs?.[inputs.length - 2];
      last?.focus();
      last?.setSelectionRange(name.length, name.length);
    }, 30);
  }
}
