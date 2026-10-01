import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { isMac, shortcutLabel } from '../core/commands';
import { dirname } from '../core/paths';
import { MenuState, TreeStore } from '../core/tree-store';
import { findItem } from '../core/tree-ops';
import { Workspace } from '../core/store';
import { Menu, MenuEntry } from './menu';

const DELETE_LABEL = isMac ? shortcutLabel('mod+backspace') : shortcutLabel('delete');

/** Menu de l'arbre : celui d'une ligne (clic droit, touche Menu, Maj+F10) ou celui du bouton + de l'en-tête. */
@Component({
  selector: 'app-tree-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Menu],
  host: { style: 'display: contents' },
  template: `
    @for (m of open(); track m) {
      <app-menu [items]="entries()" [x]="m.x" [y]="m.y" [label]="m.path ? 'Actions sur la ligne' : 'Nouvel élément'" [heading]="m.path ? '' : 'À la racine de la collection'" (closed)="tree.closeMenu()" />
    }
  `,
})
export class TreeMenu {
  protected readonly tree = inject(TreeStore);
  private readonly ws = inject(Workspace);

  protected readonly open = computed(() => {
    const menu = this.tree.menu();
    return menu ? [menu] : [];
  });
  protected readonly entries = computed(() => {
    const menu = this.tree.menu();
    return menu ? this.build(menu) : [];
  });

  private build({ path }: MenuState): MenuEntry[] {
    const item = path ? findItem(this.ws.collection()?.items ?? [], path) : null;
    const parent = !item ? '' : item.kind === 'folder' ? item.path : dirname(item.path);
    const create: MenuEntry[] = [
      { label: 'Nouvelle requête', icon: 'file', run: () => this.tree.beginCreate('request', parent) },
      { label: 'Nouveau dossier', icon: 'folder', run: () => this.tree.beginCreate('folder', parent) },
    ];
    if (!item) return create;
    return [
      ...create,
      'sep',
      { label: 'Renommer', icon: 'pencil', shortcut: shortcutLabel('f2'), run: () => this.tree.beginRename(item.path) },
      { label: 'Dupliquer', icon: 'copy', shortcut: shortcutLabel('mod+d'), run: () => this.tree.beginClone(item.path) },
      'sep',
      { label: 'Supprimer', icon: 'trash', shortcut: DELETE_LABEL, run: () => this.tree.requestDelete(item.path) },
    ];
  }
}
