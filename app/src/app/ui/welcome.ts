import { ChangeDetectionStrategy, Component, inject } from '@angular/core';

import { shortcutLabel } from '../core/commands';
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
      <p>Un dossier qui contient <span class="mono">opencollection.yml</span>. Les collections Bruno s'ouvrent telles quelles, sans import : même format, mêmes fichiers, même dépôt Git. Dans un dossier vide, on te propose d'en créer une.</p>
      <div class="actions">
        <button class="btn-primary lg" (click)="ws.pickAndOpen()" [disabled]="ws.loading()">
          <app-ic name="folder-open" [size]="16" />{{ ws.loading() ? 'Ouverture…' : 'Ouvrir un dossier' }}
        </button>
        <button class="btn lg" (click)="ws.newCollection()" [disabled]="ws.loading()"><app-ic name="plus" [size]="16" />Nouvelle collection…</button>
        <button class="btn lg" (click)="ws.dialog.set('openapi')" [disabled]="ws.loading()"><app-ic name="import" [size]="16" />Importer une spec OpenAPI…</button>
      </div>
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
        <p class="faint demo-note">Mode démo du navigateur : données synthétiques en mémoire. Le sélecteur de dossier est simulé : « Ouvrir un dossier » propose tour à tour la collection de démo, un dossier vide, un dossier non vide et un dossier .bru. Lance l'application Tauri pour travailler sur tes fichiers.</p>
      }
      <p class="faint principles"><app-ic name="search" [size]="13" /><span>Ouvrir, importer, changer de thème : tout passe par la palette de commandes <kbd class="kbd">{{ key('mod+k') }}</kbd></span></p>
      <p class="faint principles"><app-ic name="settings" [size]="13" /><span>Thème, police et taille du texte : les réglages <kbd class="kbd">{{ key('mod+,') }}</kbd></span></p>
      <p class="faint principles"><app-ic name="disk" [size]="13" />Tout reste sur ton disque. Aucun compte, aucun appel réseau tant que tu n'envoies pas de requête.</p>
    </div>
  `,
  styles: `
    :host.welcome { align-items: center; justify-content: center; }
    .welcome-body { width: min(520px, calc(100% - 48px)); display: flex; flex-direction: column; align-items: flex-start; gap: 14px; }
    .mark.big { width: 40px; height: 40px; border-radius: 10px; margin-bottom: 6px; }
    h1 { margin: 0; font-size: calc(20 * var(--px)); font-weight: 600; letter-spacing: -0.01em; }
    p { margin: 0; color: var(--muted); max-width: 60ch; }
    .actions { display: flex; flex-wrap: wrap; gap: 8px; }
    .recent { width: 100%; margin-top: 10px; border-top: 1px solid var(--line); padding-top: 8px; }
    .recent .file-row { width: 100%; margin: 0; }
    .err { background: var(--bad-soft); color: var(--bad); margin: 0; }
    .principles { display: flex; align-items: flex-start; gap: 8px; font-size: calc(12 * var(--px)); margin-top: 8px; }
    .principles .ic { margin-top: 2px; }
    .demo-note { font-size: calc(12 * var(--px)); }
  `,
})
export class Welcome {
  protected readonly ws = inject(Workspace);
  protected readonly key = shortcutLabel;

  protected last(path: string) {
    return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  }
}
