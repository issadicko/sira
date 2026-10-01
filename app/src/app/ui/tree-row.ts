import { Directive, computed, inject, input } from '@angular/core';

import { isMac } from '../core/commands';
import { TreeItem } from '../core/model';
import { canDrop, treeKeyAction } from '../core/tree-ops';
import { TreeStore } from '../core/tree-store';

const KEYBOARD_MENU_MS = 400;

/** Comportement d'une ligne de l'arbre : sélection, raccourcis, menu contextuel et glisser-déposer. */
@Directive({
  selector: '[appTreeRow]',
  host: {
    role: 'treeitem',
    draggable: 'true',
    '[attr.data-path]': 'item().path',
    '[attr.aria-selected]': 'selected()',
    '[class.is-active]': 'selected()',
    '[class.drop-before]': 'dropAt() === "before"',
    '[class.drop-after]': 'dropAt() === "after"',
    '[class.drop-inside]': 'dropAt() === "inside"',
    '(keydown)': 'onKey($event)',
    '(contextmenu)': 'onContext($event)',
    '(dragstart)': 'onDragStart($event)',
    '(dragenter)': 'onDragEnter($event)',
    '(dragover)': 'onDragOver($event)',
    '(drop)': 'onDrop($event)',
    '(dragend)': 'tree.endDrag()',
  },
})
export class TreeRow {
  readonly item = input.required<TreeItem>({ alias: 'appTreeRow' });
  protected readonly tree = inject(TreeStore);
  protected readonly selected = computed(() => this.tree.selected() === this.item().path);
  protected readonly dropAt = computed(() => {
    const drop = this.tree.drop();
    return drop?.target === this.item().path ? drop.position : null;
  });
  private keyboardMenuAt = 0;

  protected onKey(event: KeyboardEvent) {
    const action = event.isComposing ? null : treeKeyAction(event, isMac);
    if (!action) return;
    event.preventDefault();
    const path = this.item().path;
    switch (action) {
      case 'rename':
        this.tree.beginRename(path);
        break;
      case 'clone':
        this.tree.beginClone(path);
        break;
      case 'delete':
        this.tree.requestDelete(path);
        break;
      case 'up':
        void this.tree.reorder(path, -1);
        break;
      case 'down':
        void this.tree.reorder(path, 1);
        break;
      case 'menu': {
        const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
        this.keyboardMenuAt = Date.now();
        this.tree.select(path);
        this.tree.openMenu(rect.left + 24, rect.bottom, path);
        break;
      }
    }
  }

  protected onContext(event: MouseEvent) {
    event.preventDefault();
    if (Date.now() - this.keyboardMenuAt < KEYBOARD_MENU_MS) return;
    (event.currentTarget as HTMLElement).focus();
    this.tree.select(this.item().path);
    this.tree.openMenu(event.clientX, event.clientY, this.item().path);
  }

  protected onDragStart(event: DragEvent) {
    if (this.tree.busy() || this.tree.edit()) {
      event.preventDefault();
      return;
    }
    event.dataTransfer?.setData('text/plain', this.item().path);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
    this.tree.startDrag(this.item().path);
  }

  protected onDragEnter(event: DragEvent) {
    if (this.tree.dragging() && canDrop(this.tree.dragging()!, this.item().path)) event.preventDefault();
    event.stopPropagation();
  }

  protected onDragOver(event: DragEvent) {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const item = this.item();
    const accepted = this.tree.dragOver(item.path, item.kind, rect.height ? (event.clientY - rect.top) / rect.height : 0.5);
    if (event.dataTransfer) event.dataTransfer.dropEffect = accepted ? 'move' : 'none';
    if (accepted) event.preventDefault();
    event.stopPropagation();
  }

  protected onDrop(event: DragEvent) {
    event.preventDefault();
    event.stopPropagation();
    void this.tree.dropHere();
  }
}
