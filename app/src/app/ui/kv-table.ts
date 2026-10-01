import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

import { KeyValue } from '../core/model';
import { Icon } from './icon';

/** Tableau clé / valeur éditable. La dernière ligne fantôme crée une entrée dès la première frappe. */
@Component({
  selector: 'app-kv-table',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    <div class="kv" [class.cols-3]="!descriptions()">
      <div class="kv-row head">
        <span class="kv-cell"></span><span class="kv-cell">{{ keyLabel() }}</span><span class="kv-cell">Valeur</span>
        @if (descriptions()) {
          <span class="kv-cell desc">Description</span>
        }
      </div>
      @for (row of rows(); track $index; let i = $index) {
        <div class="kv-row" [class.off]="!row.enabled">
          <span class="kv-cell"><input type="checkbox" class="cb" [checked]="row.enabled" (change)="patch(i, { enabled: !row.enabled })" [attr.aria-label]="'Activer ' + row.name" /></span>
          <span class="kv-cell m-cell">
            @if (locked()(row)) {
              <span class="clip-text">{{ row.name }}</span>
            } @else {
              <input type="text" spellcheck="false" [value]="row.name" (input)="patch(i, { name: $any($event.target).value })" [attr.aria-label]="keyLabel()" />
            }
          </span>
          <span class="kv-cell m-cell">
            <input type="text" spellcheck="false" [value]="row.value" (input)="patch(i, { value: $any($event.target).value })" [attr.aria-label]="'Valeur de ' + row.name" />
            @if (!locked()(row)) {
              <button class="icon-btn sm row-x" (click)="remove(i)" [attr.aria-label]="'Supprimer ' + row.name"><app-ic name="x" [size]="13" /></button>
            }
          </span>
          @if (descriptions()) {
            <span class="kv-cell desc"><span class="clip-text">{{ row.description ?? '' }}</span></span>
          }
        </div>
      }
      @if (addable()) {
        <div class="kv-row ghost">
          <span class="kv-cell"><app-ic name="plus" [size]="13" /></span>
          <span class="kv-cell m-cell"><input type="text" spellcheck="false" [placeholder]="addLabel()" (input)="add($event)" [attr.aria-label]="addLabel()" /></span>
          <span class="kv-cell"></span>
          @if (descriptions()) {
            <span class="kv-cell desc"></span>
          }
        </div>
      }
    </div>
  `,
  styles: `
    .row-x { opacity: 0; margin-left: 4px; }
    .kv-row:hover .row-x, .row-x:focus-visible { opacity: 1; }
  `,
})
export class KvTable<T extends KeyValue> {
  readonly rows = input.required<T[]>();
  readonly keyLabel = input('Clé');
  readonly addLabel = input('Ajouter');
  readonly descriptions = input(false);
  readonly addable = input(true);
  readonly locked = input<(row: T) => boolean>(() => false);
  readonly template = input<Partial<T>>({});
  readonly rowsChange = output<T[]>();

  protected patch(i: number, change: Partial<KeyValue>) {
    this.rowsChange.emit(this.rows().map((r, j) => (j === i ? { ...r, ...change } : r)));
  }

  protected remove(i: number) {
    this.rowsChange.emit(this.rows().filter((_, j) => j !== i));
  }

  protected add(event: Event) {
    const el = event.target as HTMLInputElement;
    const name = el.value;
    el.value = '';
    this.rowsChange.emit([...this.rows(), { name, value: '', enabled: true, ...this.template() } as T]);
    setTimeout(() => {
      const inputs = el.closest('.kv')?.querySelectorAll<HTMLInputElement>('.kv-row:not(.ghost):not(.head) .m-cell input[type=text]');
      const last = inputs?.[inputs.length - 2];
      last?.focus();
      last?.setSelectionRange(name.length, name.length);
    }, 30);
  }
}
