import { Directive, ElementRef, computed, inject } from '@angular/core';

import { insideRect } from '../core/tree-ops';
import { TreeStore } from '../core/tree-store';

/** Zone de défilement de l'arbre : la place libre sous les lignes reçoit les dépôts à la racine de la collection. */
@Directive({
  selector: '[appTreeRoot]',
  host: {
    '[class.drop-root]': 'over()',
    '(dragenter)': 'onDragEnter($event)',
    '(contextmenu)': 'onContext($event)',
    '(dragover)': 'onDragOver($event)',
    '(dragleave)': 'onDragLeave($event)',
    '(drop)': 'onDrop($event)',
  },
})
export class TreeRoot {
  private readonly tree = inject(TreeStore);
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  protected readonly over = computed(() => this.tree.drop()?.target === '');

  protected onContext(event: MouseEvent) {
    if ((event.target as HTMLElement).closest('.row')) return;
    event.preventDefault();
    this.tree.openMenu(event.clientX, event.clientY, null);
  }

  protected onDragEnter(event: DragEvent) {
    if (this.tree.dragging()) event.preventDefault();
  }

  protected onDragOver(event: DragEvent) {
    const below = this.belowRows(event);
    const accepted = below && this.tree.dragOverRoot();
    if (!below) this.tree.dragLeave();
    if (event.dataTransfer) event.dataTransfer.dropEffect = accepted ? 'move' : 'none';
    if (accepted) event.preventDefault();
  }

  /** WebKit renseigne `relatedTarget` à `null` en passant d'un enfant à l'autre : seule la géométrie dit si le pointeur a quitté la zone. */
  protected onDragLeave(event: DragEvent) {
    if (!insideRect(event.clientX, event.clientY, this.el.getBoundingClientRect())) this.tree.dragLeave();
  }

  protected onDrop(event: DragEvent) {
    event.preventDefault();
    if (this.belowRows(event)) void this.tree.dropRoot();
    else this.tree.endDrag();
  }

  private belowRows(event: DragEvent): boolean {
    const rows = this.el.querySelectorAll('.row');
    const last = rows[rows.length - 1];
    return !last || event.clientY > last.getBoundingClientRect().bottom;
  }
}
