import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { shortcutLabel } from '../core/commands';
import { dirname, folderChoices } from '../core/paths';
import { Workspace } from '../core/store';
import { TreeStore } from '../core/tree-store';
import { Dialog } from './dialog';
import { Icon } from './icon';

/** « Déplacer vers… » : au clavier, choisit le dossier où mettre la requête ou le dossier visé ; il y arrive en fin de liste. */
@Component({
  selector: 'app-move-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  host: { style: 'display: contents' },
  template: `
    @if (tree.moving(); as m) {
      <app-dialog [heading]="m.kind === 'folder' ? 'Déplacer le dossier' : 'Déplacer la requête'" (closed)="tree.cancelMove()" (confirmed)="move()">
        <p class="move-text">Dossier où placer « {{ m.name }} » ; l'élément y arrive en dernière position.</p>
        <label class="fld">
          <span class="sec-head"><span class="sec-title">Dossier de destination</span></span>
          <span class="select-wrap">
            <select class="input" data-autofocus aria-label="Dossier de destination" (change)="folder.set($any($event.target).value)">
              @for (f of choices(); track f.path) {
                <option [value]="f.path" [selected]="folder() === f.path" [disabled]="f.current">{{ indent(f.depth) + f.label + (f.current ? ' (dossier actuel)' : '') }}</option>
              }
            </select>
            <app-ic name="chev-down" [size]="13" />
          </span>
        </label>
        @if (!ready()) {
          <div class="note"><app-ic name="alert" [size]="14" /><span>Aucun autre dossier : crée d'abord un dossier.</span></div>
        }
        <div dialog-foot class="foot-actions">
          <button class="btn ghost" (click)="tree.cancelMove()">Annuler</button>
          <button class="btn-primary" [disabled]="!ready()" (click)="move()">Déplacer <kbd class="kbd">{{ confirmKey }}</kbd></button>
        </div>
      </app-dialog>
    }
  `,
  styles: `
    .move-text { margin: 0; color: var(--muted); overflow-wrap: anywhere; }
  `,
})
export class MoveDialog {
  protected readonly tree = inject(TreeStore);
  private readonly ws = inject(Workspace);
  protected readonly confirmKey = shortcutLabel('mod+enter');
  protected readonly choices = computed(() => {
    const moving = this.tree.moving();
    if (!moving) return [];
    const here = dirname(moving.path);
    const folders = folderChoices(this.ws.collection()?.items ?? [], 0, moving.path);
    return [{ path: '', label: '(racine de la collection)', depth: 0 }, ...folders].map((f) => ({ ...f, current: f.path === here }));
  });
  protected readonly ready = computed(() => this.choices().some((f) => !f.current));
  protected readonly folder = signal(this.choices().find((f) => !f.current)?.path ?? '');

  protected indent(depth: number) {
    return '  '.repeat(depth);
  }

  protected move() {
    if (this.ready()) void this.tree.confirmMove(this.folder());
  }
}
