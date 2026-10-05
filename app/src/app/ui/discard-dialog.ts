import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { Workspace } from '../core/store';
import { Dialog } from './dialog';

/** Demande de confirmer la perte de modifications non enregistrées (fermeture d'onglet, changement de collection). */
@Component({
  selector: 'app-discard-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Dialog],
  host: { style: 'display: contents' },
  template: `
    @if (ws.discard(); as d) {
      <app-dialog [heading]="d.heading ?? 'Modifications non enregistrées'" (closed)="answer(false)" (confirmed)="answer(true)">
        <p class="discard-text">{{ d.message }}</p>
        <div dialog-foot class="foot-actions">
          <button class="btn ghost" data-autofocus (click)="answer(false)">Annuler</button>
          <button class="btn-primary" (click)="answer(true)">{{ d.action }}</button>
        </div>
      </app-dialog>
    }
  `,
  styles: `
    .discard-text { margin: 0; color: var(--muted); }
  `,
})
export class DiscardDialog {
  protected readonly ws = inject(Workspace);

  protected answer(accepted: boolean) {
    this.ws.answerDiscard(accepted);
  }
}
