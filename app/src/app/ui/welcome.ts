import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { Workspace } from '../core/store';
import { Icon } from './icon';

@Component({
  selector: 'app-welcome',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { class: 'island editor welcome' },
  template: `
    <div class="welcome-body">
      <span class="mark big" aria-hidden="true"><app-ic name="bolt" [size]="22" /></span>
      <h1>Ouvre une collection</h1>
      <p>Un dossier qui contient <span class="mono">opencollection.yml</span>. Les collections Bruno s'ouvrent telles quelles, sans import : même format, mêmes fichiers, même dépôt Git.</p>
      <button class="btn-primary lg" (click)="ws.pickAndOpen()" [disabled]="ws.loading()">
        <app-ic name="folder-open" [size]="16" />{{ ws.loading() ? 'Ouverture…' : 'Ouvrir un dossier' }}
      </button>
      @if (ws.error(); as e) {
        <div class="banner err"><app-ic name="alert" [size]="15" /><span>{{ e }}</span></div>
      }
      @if (ws.recent().length) {
        <div class="recent">
          <div class="group-head">Récentes</div>
          @for (r of ws.recent(); track r) {
            <button class="file-row" (click)="ws.open(r)" [title]="r"><app-ic name="box" [size]="14" /><span class="file-name">{{ last(r) }}</span><span class="file-dir mono">{{ r }}</span></button>
          }
        </div>
      }
      @if (ws.demo) {
        <p class="faint demo-note">Mode démo du navigateur : données synthétiques en mémoire. Lance l'application Tauri pour travailler sur tes fichiers.</p>
      }
      <p class="faint principles"><app-ic name="disk" [size]="13" />Tout reste sur ton disque. Aucun compte, aucun appel réseau tant que tu n'envoies pas de requête.</p>
    </div>
  `,
  styles: `
    :host.welcome { align-items: center; justify-content: center; }
    .welcome-body { width: min(520px, calc(100% - 48px)); display: flex; flex-direction: column; align-items: flex-start; gap: 14px; }
    .mark.big { width: 40px; height: 40px; border-radius: 10px; margin-bottom: 6px; }
    h1 { margin: 0; font-size: 20px; font-weight: 600; letter-spacing: -0.01em; }
    p { margin: 0; color: var(--muted); max-width: 60ch; }
    .recent { width: 100%; margin-top: 10px; border-top: 1px solid var(--line); padding-top: 8px; }
    .recent .file-row { width: 100%; margin: 0; }
    .err { background: var(--bad-soft); color: var(--bad); margin: 0; }
    .principles { display: flex; align-items: center; gap: 8px; font-size: 12px; margin-top: 8px; }
    .demo-note { font-size: 12px; }
  `,
})
export class Welcome {
  protected readonly ws = inject(Workspace);

  protected last(path: string) {
    return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  }
}
