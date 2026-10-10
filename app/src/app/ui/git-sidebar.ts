import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { GitStore } from '../core/git-store';
import { STATE_NAMES, branchSummary, fileName } from '../core/git';
import { isMac } from '../core/commands';
import { Icon } from './icon';

/** Barre latérale « Source Control » : branche, message de validation, fichiers modifiés. */
@Component({
  selector: 'app-git-sidebar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  styles: `
    .info { margin: 0 12px 10px; padding: 8px 10px; border-radius: 6px; font-size: calc(12 * var(--px)); line-height: 1.45; overflow-wrap: anywhere; white-space: pre-wrap; }
    .info.err { background: var(--bad-soft); color: var(--bad); }
    .info.ok { background: var(--good-soft); color: var(--good); }
    .hint { margin: 0 12px 8px; color: var(--faint); font-size: calc(11.5 * var(--px)); }
    .why { color: var(--faint); font-size: calc(11.5 * var(--px)); }
    .split { display: flex; gap: 6px; }
    .split .btn-primary { flex: 1; }
    .no-repo { padding: 16px 14px; display: flex; flex-direction: column; gap: 10px; color: var(--muted); font-size: calc(12.5 * var(--px)); }
    .no-repo p { margin: 0; }
    .no-repo .btn { align-self: flex-start; }
  `,
  template: `
    <div class="pane-head">
      <span class="pane-title">Source Control</span>
      <button class="icon-btn sm" (click)="git.pull()" [disabled]="!!git.busy() || !!git.pullBlocker()" [title]="git.pullBlocker() ?? 'Récupérer (pull, sans fusion automatique)'" aria-label="Récupérer"><app-ic name="arrow-down" [size]="15" /></button>
      <button class="icon-btn sm" (click)="git.push()" [disabled]="!!git.busy() || !git.state()?.repo || !!git.state()?.unborn" title="Pousser" aria-label="Pousser"><app-ic name="arrow-up" [size]="15" /></button>
      <button class="icon-btn sm" (click)="git.refresh()" title="Relire l'état Git" aria-label="Relire l'état Git"><app-ic name="sync" [size]="15" /></button>
    </div>
    @if (git.state(); as state) {
      @if (!state.repo) {
        <div class="no-repo">
          <p>Ce dossier n'est pas un dépôt Git. Crée-en un pour suivre les changements de la collection, les valider et les partager.</p>
          <button class="btn" (click)="git.init()" [disabled]="!!git.busy()"><app-ic name="branch" [size]="14" />Initialiser un dépôt</button>
        </div>
      } @else {
        <div class="branch-row"><app-ic name="branch" [size]="14" /><span class="mono">{{ state.branch ?? 'tête détachée' }}</span><span class="faint">{{ summary() }}</span></div>
        <div class="commit">
          <textarea placeholder="Message de commit ({{ shortcut }} pour valider)" aria-label="Message de commit" [value]="git.message()" (input)="git.message.set($any($event.target).value)" (keydown)="onKey($event)" [disabled]="!!git.busy()"></textarea>
          <div class="split">
            <button class="btn-primary" (click)="git.commit(false)" [disabled]="!!git.busy() || !!git.blocker()" [title]="git.blocker() ?? 'Valider tous les changements de la collection'"><app-ic name="check" [size]="14" />Valider</button>
            <button class="btn" (click)="git.commit(true)" [disabled]="!!git.busy() || !!git.blocker()" [title]="git.blocker() ?? 'Valider puis pousser'">et pousser</button>
          </div>
          @if (git.message().trim() === '' && git.count()) {
            <span class="why">{{ git.blocker() }}</span>
          }
        </div>
        @if (git.error(); as error) {
          <div class="info err" role="alert">{{ error }}</div>
        } @else if (git.notice(); as notice) {
          <div class="info ok" role="status">{{ notice }}</div>
        }
        @if (git.busy(); as busy) {
          <p class="hint">{{ busyLabel(busy) }}</p>
        }
        <div class="group-head"><app-ic name="chev-down" [size]="13" />Modifications<span class="n">{{ git.count() }}</span></div>
        <div class="sb-scroll" style="padding-top: 0">
          @for (f of git.files(); track f.path) {
            <button class="file-row" [class.is-active]="git.current()?.path === f.path" (click)="select(f.path)" [title]="f.path + ' · ' + names[f.state]">
              <app-ic name="file" [size]="14" /><span class="file-name">{{ name(f.path).name }}</span><span class="file-dir mono">{{ name(f.path).dir }}</span>
              <span class="git" [class]="'git git-' + f.state">{{ f.state }}</span>
            </button>
          } @empty {
            <p class="pal-empty">Aucune modification.</p>
          }
        </div>
      }
    } @else if (git.error(); as error) {
      <div class="info err" role="alert">{{ error }}</div>
    } @else {
      <p class="hint">Lecture de l'état Git…</p>
    }
  `,
})
export class GitSidebar {
  protected readonly git = inject(GitStore);
  protected readonly names = STATE_NAMES;
  protected readonly shortcut = isMac ? '⌘↵' : 'Ctrl+↵';
  protected readonly summary = computed(() => {
    const state = this.git.state();
    return state ? branchSummary(state) : '';
  });

  protected name = fileName;

  protected select(path: string) {
    this.git.select(path);
  }

  protected busyLabel(busy: string): string {
    return { commit: 'Validation en cours…', pull: 'Récupération en cours…', push: 'Envoi en cours…', init: 'Création du dépôt…' }[busy] ?? '';
  }

  protected onKey(event: KeyboardEvent) {
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void this.git.commit(false);
    }
  }
}
