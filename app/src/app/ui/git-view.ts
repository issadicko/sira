import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { GitStore } from '../core/git-store';
import { STATE_NAMES, collapse, diffRows, touchedLines } from '../core/git';
import { Workspace } from '../core/store';
import { Icon } from './icon';

/** Éditeur de la vue « Source Control » : la version du dernier commit face à celle du disque. */
@Component({
  selector: 'app-git-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  styles: `
    .col-body { font: var(--code-size)/1.6 var(--code-font); padding: 4px 0 16px; }
    .fold { color: var(--faint); font-style: italic; }
    .note { margin: 16px; }
  `,
  template: `
    @if (git.current(); as file) {
      <div class="tabs"><div class="tab preview" role="tab" aria-selected="true"><app-ic name="file-diff" [size]="14" /><span class="tab-name">{{ file.path.split('/').pop() }}</span><span class="tag">diff</span></div></div>
      <div class="view-head">
        <h1 class="view-title mono" style="font-size: 13.5px; font-weight: 500">{{ file.path }}</h1>
        <span class="tag" [class.good]="file.state === 'A'" [class.bad]="file.state === 'D' || file.state === 'U'">{{ names[file.state] }}</span>
        @if (file.from) {
          <span class="view-sub">depuis {{ file.from }}</span>
        } @else if (rows().length) {
          <span class="view-sub">{{ touched() }} ligne{{ touched() > 1 ? 's' : '' }} touchée{{ touched() > 1 ? 's' : '' }}</span>
        }
        <span class="grow"></span>
        @if (isRequest(file.path)) {
          <button class="btn" (click)="open(file.path)"><app-ic name="arrow-right" [size]="14" />Ouvrir la requête</button>
        }
      </div>
      @if (git.diff(); as diff) {
        @if (diff.unreadable) {
          <div class="note"><app-ic name="alert" [size]="14" /><span>{{ diff.unreadable }} : pas de comparaison ligne à ligne.</span></div>
        } @else {
          <div class="diff">
            <div class="diff-grid">
              <div class="diff-col">
                <div class="diff-col-head"><b>Dernier commit</b><span class="mono">{{ git.state()?.branch ?? 'HEAD' }}</span></div>
                <div class="col-body">
                  @for (s of shown(); track $index) {
                    @if (s.hidden) {
                      <div class="ln"><span class="ln-no"></span><span class="ln-g"></span><span class="ln-t fold">{{ s.hidden }} ligne{{ s.hidden > 1 ? 's' : '' }} identique{{ s.hidden > 1 ? 's' : '' }}</span></div>
                    } @else if (s.row?.l; as l) {
                      <div class="ln" [class.del]="s.row?.kind !== 'ctx'"><span class="ln-no">{{ l.no }}</span><span class="ln-g">{{ s.row?.kind === 'ctx' ? '' : '−' }}</span><span class="ln-t">{{ l.text }}</span></div>
                    } @else {
                      <div class="ln fill"><span class="ln-no"></span><span class="ln-g"></span><span class="ln-t"></span></div>
                    }
                  }
                </div>
              </div>
              <div class="diff-col">
                <div class="diff-col-head"><b>Copie de travail</b><span>sur le disque</span></div>
                <div class="col-body">
                  @for (s of shown(); track $index) {
                    @if (s.hidden) {
                      <div class="ln"><span class="ln-no"></span><span class="ln-g"></span><span class="ln-t fold">{{ s.hidden }} ligne{{ s.hidden > 1 ? 's' : '' }} identique{{ s.hidden > 1 ? 's' : '' }}</span></div>
                    } @else if (s.row?.r; as r) {
                      <div class="ln" [class.add]="s.row?.kind !== 'ctx'"><span class="ln-no">{{ r.no }}</span><span class="ln-g">{{ s.row?.kind === 'ctx' ? '' : '+' }}</span><span class="ln-t">{{ r.text }}</span></div>
                    } @else {
                      <div class="ln fill"><span class="ln-no"></span><span class="ln-g"></span><span class="ln-t"></span></div>
                    }
                  }
                </div>
              </div>
            </div>
          </div>
        }
      }
    } @else if (git.state()?.repo === false) {
      <div class="empty">
        <span class="empty-ic"><app-ic name="branch" [size]="18" /></span>
        <h2>Pas de dépôt Git</h2>
        <p>Initialise un dépôt dans la barre latérale pour suivre les changements de cette collection.</p>
      </div>
    } @else {
      <div class="empty">
        <span class="empty-ic"><app-ic name="check" [size]="18" /></span>
        <h2>Rien à valider</h2>
        <p>L'arbre de travail est propre. Tes prochaines modifications de requêtes apparaîtront ici, fichier par fichier.</p>
      </div>
    }
  `,
})
export class GitView {
  protected readonly git = inject(GitStore);
  private readonly ws = inject(Workspace);
  protected readonly names = STATE_NAMES;
  protected readonly rows = computed(() => {
    const diff = this.git.diff();
    return diff && !diff.unreadable ? diffRows(diff.head, diff.work) : [];
  });
  protected readonly shown = computed(() => collapse(this.rows()));
  protected readonly touched = computed(() => touchedLines(this.rows()));

  protected isRequest(path: string) {
    return path.endsWith('.yml') && !path.startsWith('environments/') && !path.startsWith('.oc-sync/') && !path.endsWith('folder.yml') && path !== 'opencollection.yml';
  }

  protected open(path: string) {
    this.ws.view.set('collections');
    void this.ws.openRequest(path, true);
  }
}
