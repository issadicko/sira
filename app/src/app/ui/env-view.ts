import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { COMMANDS } from '../core/commands';
import { EnvStore } from '../core/env-store';
import { Workspace } from '../core/store';
import { EnvTable } from './env-table';
import { Icon } from './icon';

@Component({
  selector: 'app-env-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, EnvTable],
  host: { style: 'display: contents' },
  template: `
    @if (store.owner(); as env) {
      <div class="tabs">
        <div class="tab is-env" role="tab" aria-selected="true" [class.is-dirty]="store.dirty()" [class.is-stale]="store.stale() || store.missing()">
          <app-ic name="file" [size]="14" /><span class="tab-name">{{ env }}.yml</span>
          <span class="tab-x" aria-hidden="true"><span class="dirty"></span></span>
        </div>
      </div>
      <div class="view-head">
        <h1 class="view-title">Environnement {{ env }}</h1>
        @if (isDefault()) {
          <span class="tag good" title="La collection ouvre cet environnement au démarrage">par défaut</span>
        }
        <span class="view-sub">{{ store.draft().length }} variable(s)</span>
        <span class="crumbs-state head-state" [class.dirty]="store.dirty()" [class.stale]="store.stale() || store.missing()">
          @if (store.missing()) {
            <span class="stale-text" title="Le fichier n'existe plus sur le disque : ton brouillon est gardé, et l'enregistrer le recrée."><app-ic name="alert" [size]="12" />Fichier introuvable</span>
          } @else if (store.stale()) {
            <span class="stale-text" title="Le fichier a changé sur le disque : ton brouillon est gardé, et l'enregistrer demandera confirmation."><app-ic name="alert" [size]="12" />Modifié sur le disque</span>
            <button class="btn ghost sm" (click)="store.reloadFromDisk()">Recharger</button>
          } @else if (store.dirty()) {
            Non enregistré
          } @else {
            <app-ic name="check" [size]="12" />Enregistré sur le disque
          }
        </span>
        <span class="head-spacer"></span>
        @if (store.dirty() && !store.stale() && !store.missing()) {
          <button class="btn ghost sm" (click)="store.discard()">Annuler les modifications</button>
        }
        <button class="btn-primary" [class.is-sending]="store.saving()" [disabled]="!store.dirty() || !store.valid() || store.saving()" (click)="store.save()">
          @if (store.saving()) {
            <span class="spinner"></span>Enregistrement…
          } @else {
            {{ store.missing() ? 'Recréer le fichier' : 'Enregistrer' }} <kbd class="kbd">{{ label('env.save') }}</kbd>
          }
        </button>
        <button class="icon-btn" (click)="openMenu($event, env)" aria-haspopup="menu" title="Actions sur l'environnement" aria-label="Actions sur l'environnement"><app-ic name="more" [size]="16" /></button>
      </div>
      <div class="env-layout">
        <div class="env-main">
          @if (ws.envError(); as e) {
            @if (!store.missing()) {
              <div class="banner err" role="alert"><app-ic name="alert" [size]="15" /><span>Impossible de relire <span class="mono">{{ env }}.yml</span> : {{ e }}. Le tableau montre la dernière version lue.</span></div>
            }
          }
          <app-env-table [rows]="store.draft()" [problems]="store.problems()" [picked]="selected()" (rowsChange)="store.edit($event)" (pick)="selected.set($event)" />
          @for (p of problemList(); track p.row) {
            <p class="env-problem" role="alert"><app-ic name="alert" [size]="13" />Ligne {{ p.row }} : {{ p.text }}</p>
          }
          <p class="faint env-note">Un secret n'a jamais sa valeur dans le fichier : seul <span class="mono">secret: true</span> y figure. Sa saisie dans le trousseau est prévue en V1 ; en attendant, <span class="mono">{{ '{{process.env.NOM}}' }}</span> lit le fichier <span class="mono">.env</span> de la collection.</p>
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
            <div class="resolve-foot">Calculée pour la requête active ({{ ws.activePath() }}) d'après les fichiers enregistrés. Ordre identique à Bruno.</div>
          } @else if (selected()) {
            <p>« {{ selected() }} » n'est pas encore dans le fichier : enregistre pour voir d'où viendrait sa valeur.</p>
          } @else {
            <p>Ouvre une requête puis choisis une variable pour voir d'où vient sa valeur.</p>
          }
        </aside>
      </div>
    } @else {
      <div class="tabs"><div class="tab" role="tab" aria-selected="true"><app-ic name="file" [size]="14" /><span class="tab-name">Environnements</span></div></div>
      <div class="empty">
        <span class="empty-ic"><app-ic name="variable" [size]="20" /></span>
        <h2>{{ ws.collection()?.environments?.length ? 'Aucun environnement actif' : "Cette collection n'a pas d'environnement" }}</h2>
        <p>Un environnement regroupe des variables (<span class="mono">baseUrl</span>, un identifiant de test…) que les requêtes utilisent avec <span class="mono">{{ '{{nom}}' }}</span>. Chacun est un fichier de <span class="mono">environments/</span>.</p>
        <div class="actions">
          <button class="btn" (click)="store.beginNaming({ mode: 'create' })"><app-ic name="plus" [size]="14" />Nouvel environnement</button>
        </div>
      </div>
    }
  `,
  styles: `
    .head-state { margin-left: 4px; white-space: nowrap; flex-shrink: 0; }
    .view-title { flex-shrink: 0; }
    .head-spacer { flex: 1; }
    .env-problem { display: flex; align-items: center; gap: 6px; margin: 8px 2px 0; color: var(--bad); font-size: calc(12 * var(--px)); }
    .banner { margin-bottom: 10px; }
    .banner > span { min-width: 0; overflow-wrap: anywhere; }
    .env-note { font-size: calc(12 * var(--px)); margin: 10px 2px; }
  `,
})
export class EnvView {
  protected readonly ws = inject(Workspace);
  protected readonly store = inject(EnvStore);
  private readonly commands = inject(COMMANDS);
  protected readonly selected = signal<string | null>(null);
  protected readonly isDefault = computed(() => !!this.store.owner() && this.ws.collection()?.defaultEnvironment === this.store.owner());
  protected readonly info = computed(() => {
    const name = this.selected() ?? this.store.draft()[0]?.name;
    return name ? (this.ws.varMap().get(name) ?? null) : null;
  });
  protected readonly problemList = computed(() =>
    this.store
      .problems()
      .flatMap((text, i) => (text ? [{ row: i + 1, text }] : []))
      .slice(0, 3),
  );

  protected label(id: string) {
    return this.commands.label(id);
  }

  protected openMenu(event: MouseEvent, env: string) {
    if (this.store.menu()) {
      this.store.closeMenu();
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.store.openMenu(Math.max(8, rect.right - 200), rect.bottom + 4, env);
  }
}
