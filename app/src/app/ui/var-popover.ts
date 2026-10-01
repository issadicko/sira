import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { Workspace } from '../core/store';

/** Survol d'une {{variable}} : valeur retenue et échelle de précédence complète. */
@Component({
  selector: 'app-var-popover',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (state(); as s) {
      <div class="pop open" role="tooltip" [style.left.px]="s.left" [style.top.px]="s.top">
        @if (s.kind === 'dynamic') {
          <div class="pop-head"><span class="pop-name">{{ '{{' + s.name + '}}' }}</span><span class="tag spec">dynamique</span></div>
          <p class="faint" style="margin: 0; font-size: 12px">Nouvelle valeur générée à chaque envoi.</p>
        } @else if (s.kind === 'env') {
          <div class="pop-head"><span class="pop-name">{{ '{{' + s.name + '}}' }}</span><span class="tag good">.env local</span></div>
          <p class="faint" style="margin: 0; font-size: 12px">Lue dans le fichier .env de la collection, ignoré par Git. Jamais affichée.</p>
        } @else if (s.info?.value != null) {
          <div class="pop-head"><span class="pop-name">{{ '{{' + s.name + '}}' }}</span>@if (s.info!.secret) {<span class="tag">secret</span>}</div>
          <div class="pop-val">{{ s.info!.secret ? '••••••••' : s.info!.value }}</div>
          <div class="ladder">
            @for (r of s.info!.rungs; track r.level; let first = $first) {
              <div class="rung" [class]="'rung ' + rungState(r.value, r.level === s.info!.level)">
                <span class="rung-dot"></span>
                <span class="rung-name"><span class="rung-label">{{ r.level }}</span>@if (r.level === s.info!.level) {<span class="tag new">utilisée</span>}<span class="rung-src">{{ r.source }}</span></span>
                @if (r.value != null) {
                  <span class="rung-val">{{ s.info!.secret ? '••••••••' : r.value }}</span>
                }
              </div>
            }
          </div>
          <div class="pop-foot"><span>Ordre de priorité de Bruno</span></div>
        } @else {
          <div class="pop-head"><span class="pop-name" style="color: var(--bad)">{{ '{{' + s.name + '}}' }}</span><span class="tag bad">non définie</span></div>
          <p style="margin: 0; font-size: 12.5px; color: var(--muted)">
            @if (s.info?.secret) {
              Variable secrète : sa valeur n'est jamais écrite dans le fichier. Saisie dans le trousseau prévue en V1 ; en attendant, utilise {{ '{{process.env.' + s.name + '}}' }}.
            } @else {
              Aucune portée ne la définit{{ ws.env() ? ' pour ' + ws.env() : '' }}. Elle partira telle quelle.
            }
          </p>
        }
      </div>
    }
  `,
})
export class VarPopover {
  protected readonly ws = inject(Workspace);

  protected readonly state = computed(() => {
    const h = this.ws.hover();
    if (!h) return null;
    const kind = h.name.startsWith('$') ? 'dynamic' : h.name.startsWith('process.env.') ? 'env' : 'var';
    const left = Math.min(Math.max(h.rect.left - 10, 8), window.innerWidth - 328);
    return { name: h.name, kind, info: this.ws.varMap().get(h.name), left, top: h.rect.bottom + 8 };
  });

  protected rungState(value: string | null | undefined, winner: boolean) {
    return value == null ? 'is-empty' : winner ? 'is-win' : 'is-shadowed';
  }
}
