import { ChangeDetectionStrategy, Component, ElementRef, afterNextRender, inject, input, viewChild } from '@angular/core';

import { TreeStore } from '../core/tree-store';
import { Icon } from './icon';
import { methodClass, shortMethod } from './method';

let count = 0;

/** Ligne de saisie sur place : création, duplication et renommage ; Entrée valide, Échap annule. */
@Component({
  selector: 'app-tree-edit',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
    <div class="row edit" [class.req]="kind() === 'request'" [style.--d]="depth()">
      @if (kind() === 'folder') {
        <span class="twist"></span><app-ic name="folder" [size]="15" />
      } @else {
        <span [class]="methodClass(method())">{{ shortMethod(method()) }}</span>
      }
      <input
        #field
        class="row-input"
        type="text"
        spellcheck="false"
        autocomplete="off"
        [class.is-bad]="!!tree.editError()"
        [attr.aria-label]="kind() === 'folder' ? 'Nom du dossier' : 'Nom de la requête'"
        [attr.aria-invalid]="!!tree.editError()"
        [attr.aria-describedby]="tree.editError() ? errorId : null"
        [placeholder]="kind() === 'folder' ? 'Nom du dossier' : 'Nom de la requête'"
        [readOnly]="tree.busy()"
        [value]="initial()"
        (input)="tree.editError.set(null)"
        (keydown)="onKey($event)"
        (blur)="onBlur()"
      />
    </div>
    @if (tree.editError(); as e) {
      <div class="row-error" role="alert" [id]="errorId" [style.--d]="depth()">{{ e }}</div>
    }
  `,
})
export class TreeEdit {
  protected readonly tree = inject(TreeStore);
  readonly kind = input.required<'request' | 'folder'>();
  readonly depth = input(0);
  readonly initial = input('');
  readonly method = input('GET');
  protected readonly errorId = `tree-edit-error-${count++}`;
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;
  private readonly field = viewChild.required<ElementRef<HTMLInputElement>>('field');

  constructor() {
    afterNextRender(() => {
      const el = this.field().nativeElement;
      el.focus();
      el.select();
      el.scrollIntoView({ block: 'nearest' });
    });
  }

  protected onKey(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === 'Enter') {
      event.preventDefault();
      event.stopPropagation();
      void this.tree.submit(this.field().nativeElement.value);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      this.tree.cancelEdit();
    }
  }

  protected onBlur() {
    if (document.hasFocus()) this.tree.cancelEdit();
  }
}
