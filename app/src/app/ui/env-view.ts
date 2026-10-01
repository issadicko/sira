import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { Workspace } from '../core/store';
import { Icon } from './icon';

@Component({
  selector: 'app-env-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
    <div class="tabs"><div class="tab" role="tab" aria-selected="true"><app-ic name="file" [size]="14" /><span class="tab-name">{{ ws.env() ? ws.env() + '.yml' : 'Environnements' }}</span></div></div>
    <div class="view-head">
      <h1 class="view-title">{{ ws.env() ? 'Environnement ' + ws.env() : 'Aucun environnement actif' }}</h1>
      <span class="view-sub">{{ ws.envVars().length }} variable(s) · lecture seule dans le MVP</span>
    </div>
    @if (ws.env()) {
      <div class="env-layout">
        <div class="env-main">
          <div class="vt" role="listbox" aria-label="Variables">
            <div class="vt-row head"><span class="vt-cell">Nom</span><span class="vt-cell">Valeur</span><span class="vt-cell">État</span><span class="vt-cell center">Secret</span></div>
            @for (v of ws.envVars(); track v.name) {
              <button class="vt-row" role="option" [attr.aria-selected]="selected() === v.name" (click)="selected.set(v.name)">
                <span class="vt-cell mono">{{ v.name }}</span>
                <span class="vt-cell mono">
                  @if (v.secret) {
                    <span class="secret"><app-ic name="key" [size]="13" />hors fichier</span>
                  } @else {
                    <span class="clip">{{ v.value ?? '' }}</span>
                  }
                </span>
                <span class="vt-cell vt-type">{{ v.enabled ? 'actif' : 'désactivé' }}</span>
                <span class="vt-cell center">
                  @if (v.secret) {
                    <span style="color: var(--accent)"><app-ic name="lock" [size]="14" /></span>
                  }
                </span>
              </button>
            }
          </div>
          <p class="faint" style="font-size: 12px; margin: 10px 2px">Les secrets ne sont jamais écrits dans le fichier, seul <span class="mono">secret: true</span> y figure.</p>
        </div>
        <aside class="resolve" aria-label="Résolution">
          @if (info(); as i) {
            <h2>Résolution de <span class="var">{{ '{{' + i.name + '}}' }}</span></h2>
            <p>{{ i.level ? 'Valeur utilisée : ' + i.level + '.' : 'Aucune portée ne la définit pour la requête active.' }}</p>
            <div class="ladder">
              @for (r of i.rungs; track r.level) {
                <div class="rung" [class]="'rung ' + (r.value == null ? 'is-empty' : r.level === i.level ? 'is-win' : 'is-shadowed')">
                  <span class="rung-dot"></span>
                  <span class="rung-name"><span class="rung-label">{{ r.level }}</span>@if (r.level === i.level) {<span class="tag new">utilisée</span>}<span class="rung-src">{{ r.source }}</span></span>
                  @if (r.value != null) {
                    <span class="rung-val">{{ i.secret ? '••••••••' : r.value }}</span>
                  }
                </div>
              }
            </div>
            <div class="resolve-foot">Calculée pour la requête active ({{ ws.activePath() }}). Ordre identique à Bruno.</div>
          } @else {
            <p>Ouvre une requête puis choisis une variable pour voir d'où vient sa valeur.</p>
          }
        </aside>
      </div>
    }
  `,
})
export class EnvView {
  protected readonly ws = inject(Workspace);
  protected readonly selected = signal<string | null>(null);
  protected readonly info = computed(() => {
    const name = this.selected() ?? this.ws.envVars()[0]?.name;
    return name ? this.ws.varMap().get(name) ?? null : null;
  });
}
