import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

import { blankVar } from '../core/env-ops';
import { EnvVar } from '../core/model';
import { Icon } from './icon';

/**
 * Variables d'un environnement, éditables. La valeur d'un secret n'est jamais dans le fichier : elle n'est ni affichée ni saisissable
 * (trousseau en V1). La dernière ligne fantôme crée une variable dès la première frappe.
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
              <span class="secret" title="La valeur d'un secret n'est jamais écrite dans le fichier."><app-ic name="lock" [size]="13" />hors fichier</span>
            } @else {
              <input type="text" spellcheck="false" autocomplete="off" [value]="row.value ?? ''" (input)="patch(i, { value: $any($event.target).value })" [attr.aria-label]="'Valeur de ' + row.name" />
              @if (row.dataType) {
                <span class="tag" [title]="'Valeur de type ' + row.dataType + ', conservé à l\\'enregistrement'">{{ row.dataType }}</span>
              }
            }
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
    .row-x { opacity: 0; margin-left: auto; flex-shrink: 0; }
    .kv-row:hover .row-x, .row-x:focus-visible { opacity: 1; }
  `,
})
export class EnvTable {
  readonly rows = input.required<EnvVar[]>();
  /** Ce qui empêche d'enregistrer chaque ligne, `null` quand elle convient. */
  readonly problems = input<(string | null)[]>([]);
  /** Nom de la variable dont la résolution est affichée à côté. */
  readonly picked = input<string | null>(null);
  readonly rowsChange = output<EnvVar[]>();
  readonly pick = output<string>();

  protected patch(i: number, change: Partial<EnvVar>) {
    this.rowsChange.emit(this.rows().map((row, j) => (j === i ? { ...row, ...change } : row)));
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
