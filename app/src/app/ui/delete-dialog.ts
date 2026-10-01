import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { shortcutLabel } from '../core/commands';
import { Workspace } from '../core/store';
import { Deletion, TreeStore } from '../core/tree-store';
import { describeContents, unsavedMessage } from '../core/tree-ops';
import { Dialog } from './dialog';
import { Icon } from './icon';

/** Confirme la suppression d'une requête ou d'un dossier : l'élément part dans la corbeille du système. */
@Component({
  selector: 'app-delete-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog, Icon],
  host: { style: 'display: contents' },
  template: `
    @if (tree.deletion(); as d) {
      <app-dialog [heading]="d.kind === 'folder' ? 'Supprimer le dossier' : 'Supprimer la requête'" [busy]="tree.busy()" (closed)="tree.cancelDelete()" (confirmed)="tree.confirmDelete()">
        <p class="del-text">{{ message(d) }}</p>
        @if (d.unsaved.length) {
          <div class="note"><app-ic name="alert" [size]="14" /><span>{{ unsaved(d) }}</span></div>
        }
        @if (ws.demo) {
          <p class="del-text faint">Mode démo : l'élément disparaît de la collection en mémoire, rien n'est envoyé à la corbeille de ton disque, et le suivi OpenAPI (<span class="mono">.oc-sync</span>) n'est pas simulé.</p>
        }
        @if (tree.deleteError(); as e) {
          <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
        }
        <div dialog-foot class="foot-actions">
          <button class="btn ghost" data-autofocus [disabled]="tree.busy()" (click)="tree.cancelDelete()">Annuler</button>
          <button class="btn-primary" [class.is-sending]="tree.busy()" [disabled]="tree.busy()" (click)="tree.confirmDelete()">
            @if (tree.busy()) {
              <span class="spinner"></span>Suppression…
            } @else {
              <app-ic name="trash" [size]="15" />Mettre à la corbeille <kbd class="kbd">{{ confirmKey }}</kbd>
            }
          </button>
        </div>
      </app-dialog>
    }
  `,
  styles: `
    .del-text { margin: 0; color: var(--muted); overflow-wrap: anywhere; }
    .del-text b { color: var(--ink); font-weight: 600; }
  `,
})
export class DeleteDialog {
  protected readonly ws = inject(Workspace);
  protected readonly tree = inject(TreeStore);
  protected readonly confirmKey = shortcutLabel('mod+enter');

  protected message(d: Deletion): string {
    if (d.kind === 'request') return `« ${d.name} » va dans la corbeille du système. Tu pourras la récupérer de là.`;
    const contents = describeContents(d);
    return contents
      ? `Le dossier « ${d.name} » va dans la corbeille du système, avec ce qu'il contient : ${contents}.`
      : `Le dossier « ${d.name} », vide, va dans la corbeille du système.`;
  }

  protected unsaved(d: Deletion): string {
    return unsavedMessage(d.unsaved);
  }
}
