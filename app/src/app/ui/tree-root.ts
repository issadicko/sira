import { Directive, computed, inject } from '@angular/core';

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
    const rows = (event.currentTarget as HTMLElement).querySelectorAll('.row');
    const last = rows[rows.length - 1];
    const below = !last || event.clientY > last.getBoundingClientRect().bottom;
    const accepted = below && this.tree.dragOverRoot();
    if (!below) this.tree.dragLeave();
    if (event.dataTransfer) event.dataTransfer.dropEffect = accepted ? 'move' : 'none';
    if (accepted) event.preventDefault();
  }

  protected onDragLeave(event: DragEvent) {
    if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null)) this.tree.dragLeave();
  }

  protected onDrop(event: DragEvent) {
    event.preventDefault();
    void this.tree.dropHere();
  }
}
