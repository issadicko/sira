import { ChangeDetectionStrategy, Component, ElementRef, inject, input, output } from '@angular/core';

import { MultipartField } from '../core/model';
import { isInsideCollection } from '../core/paths';
import { Icon } from './icon';

/** Tableau des champs d'un corps multipart : texte ou fichiers de la collection. */
@Component({
  selector: 'app-multipart-table',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    <div class="kv multi">
      <div class="kv-row head">
        <span class="kv-cell"></span><span class="kv-cell">Nom</span><span class="kv-cell">Type</span><span class="kv-cell">Valeur</span>
      </div>
      @for (f of fields(); track $index; let i = $index) {
        <div class="kv-row" [class.off]="!f.enabled">
          <span class="kv-cell"><input type="checkbox" class="cb" [checked]="f.enabled" (change)="patch(i, { enabled: !f.enabled })" [attr.aria-label]="'Activer ' + f.name" /></span>
          <span class="kv-cell m-cell"><input type="text" data-name spellcheck="false" [value]="f.name" (input)="patch(i, { name: $any($event.target).value })" aria-label="Nom du champ" /></span>
          <span class="kv-cell">
            <select class="kind" (change)="setKind(i, $any($event.target).value)" [attr.aria-label]="'Type de ' + f.name">
              <option value="text" [selected]="f.kind === 'text'">Texte</option>
              <option value="file" [selected]="f.kind === 'file'">Fichier</option>
            </select>
          </span>
          <span class="kv-cell m-cell value">
            @if (f.kind === 'text') {
              <input type="text" spellcheck="false" [value]="f.value" (input)="setText(i, $any($event.target).value)" [attr.aria-label]="'Valeur de ' + f.name" />
            } @else {
              @for (path of f.value; track path) {
                <span class="file-chip" [class.bad]="!inside(path)" [attr.aria-invalid]="inside(path) ? null : 'true'" [title]="inside(path) ? path : 'Hors de la collection : le moteur refusera ce chemin'">
                  <app-ic [name]="inside(path) ? 'file' : 'alert'" [size]="12" /><span class="clip-text">{{ path }}</span>
                  <button (click)="removeFile(i, path)" [attr.aria-label]="'Retirer ' + path"><app-ic name="x" [size]="11" /></button>
                </span>
              } @empty {
                <span class="faint">Aucun fichier</span>
              }
              <button class="pick" (click)="pick.emit(i)"><app-ic name="plus" [size]="12" />{{ f.value.length ? 'Ajouter' : 'Choisir un fichier…' }}</button>
            }
            <button class="icon-btn sm row-x" (click)="remove(i)" [attr.aria-label]="'Supprimer ' + f.name"><app-ic name="x" [size]="13" /></button>
          </span>
        </div>
      }
      <div class="kv-row ghost">
        <span class="kv-cell"><app-ic name="plus" [size]="13" /></span>
        <span class="kv-cell m-cell"><input type="text" spellcheck="false" placeholder="Ajouter un champ" (input)="add($event)" aria-label="Ajouter un champ" /></span>
        <span class="kv-cell"></span>
        <span class="kv-cell"></span>
      </div>
    </div>
  `,
  styles: `
    .multi .kv-row { grid-template-columns: 32px minmax(0, 1fr) 92px minmax(0, 2fr); }
    .kind { width: 100%; height: 28px; border: 0; background: transparent; font: 12.5px var(--font-ui); outline: 0; cursor: pointer; }
    .kind option { background: var(--pop); }
    .value { flex-wrap: wrap; gap: 4px; padding-top: 3px; padding-bottom: 3px; }
    .value input[type='text'] { flex: 1; }
    .row-x { opacity: 0; margin-left: auto; }
    .kv-row:hover .row-x, .row-x:focus-visible { opacity: 1; }
    .file-chip { display: inline-flex; align-items: center; gap: 4px; height: 21px; min-width: 0; max-width: 100%; padding: 0 3px 0 6px; border-radius: 4px; background: var(--raised); font: 12px var(--font-mono); }
    .file-chip .ic { color: var(--muted); }
    .file-chip button { width: 15px; height: 15px; display: grid; place-items: center; border-radius: 4px; color: var(--muted); }
    .file-chip button:hover { background: var(--hover); color: var(--ink); }
    .file-chip.bad { background: var(--bad-soft); color: var(--bad); }
    .file-chip.bad .ic { color: var(--bad); }
    .pick { height: 21px; display: inline-flex; align-items: center; gap: 4px; padding: 0 6px; border-radius: 4px; color: var(--accent); font: 500 12px var(--font-ui); }
    .pick:hover { background: var(--accent-soft); }
    @container (max-width: 560px) {
      .multi .kv-row { grid-template-columns: 32px minmax(0, 1fr) 80px minmax(0, 1.6fr); }
    }
  `,
})
export class MultipartTable {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  readonly fields = input.required<MultipartField[]>();
  readonly fieldsChange = output<MultipartField[]>();
  readonly pick = output<number>();

  protected readonly inside = isInsideCollection;

  protected patch(i: number, change: { name?: string; enabled?: boolean }) {
    this.fieldsChange.emit(this.fields().map((f, j) => (j === i ? { ...f, ...change } : f)));
  }

  protected setText(i: number, value: string) {
    this.fieldsChange.emit(this.fields().map((f, j) => (j === i && f.kind === 'text' ? { ...f, value } : f)));
  }

  protected setKind(i: number, kind: MultipartField['kind']) {
    this.fieldsChange.emit(
      this.fields().map((f, j) => {
        if (j !== i || f.kind === kind) return f;
        return kind === 'file' ? { ...f, kind, value: [] } : { ...f, kind, value: '' };
      }),
    );
  }

  protected removeFile(i: number, path: string) {
    this.fieldsChange.emit(
      this.fields().map((f, j) => (j === i && f.kind === 'file' ? { ...f, value: f.value.filter((p) => p !== path) } : f)),
    );
  }

  protected remove(i: number) {
    this.fieldsChange.emit(this.fields().filter((_, j) => j !== i));
  }

  protected add(event: Event) {
    const input = event.target as HTMLInputElement;
    const name = input.value;
    input.value = '';
    this.fieldsChange.emit([...this.fields(), { name, kind: 'text', value: '', enabled: true }]);
    setTimeout(() => {
      const rows = this.host.querySelectorAll('.kv-row:not(.ghost):not(.head)');
      const created = rows[rows.length - 1]?.querySelector<HTMLInputElement>('input[data-name]');
      created?.focus();
      created?.setSelectionRange(name.length, name.length);
    }, 30);
  }
}
