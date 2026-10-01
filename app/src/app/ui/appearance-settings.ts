import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';

import { SettingsStore } from '../core/settings-store';
import { FONT_GROUPS, FONT_NAME_MAX, FONT_NAME_RULE, FamilyChoice, FontGroup, FontGroupSpec, validFontName } from '../core/settings';
import { Workspace } from '../core/store';
import { Icon } from './icon';

const SAMPLES: Record<Exclude<FontGroup, 'ui'>, string> = {
  request: 'POST {{baseUrl}}/transactions\n{\n  "montant": 15000,\n  "devise": "XOF"\n}',
  response: '{\n  "id": "TX-2026-0042",\n  "statut": "CONFIRMEE",\n  "frais": 150\n}',
};

/** Section « Apparence » : thème, puis famille et taille de la police de l'interface, de l'éditeur de requête et du résultat. */
@Component({
  selector: 'app-appearance-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  template: `
    <h1 class="view-title">Apparence</h1>
    <p class="intro">Les changements s'appliquent tout de suite à toute l'application et restent sur cet ordinateur. Aucune police n'est téléchargée : Inter et JetBrains Mono sont embarquées, les autres viennent de ton système.</p>

    <section class="set-group" aria-labelledby="g-theme">
      <div class="set-head"><h2 class="sec-title" id="g-theme">Thème</h2></div>
      <div class="seg" role="group" aria-label="Thème">
        <button [attr.aria-pressed]="ws.theme() === 'dark'" (click)="ws.setTheme('dark')"><app-ic name="moon" [size]="13" />Sombre</button>
        <button [attr.aria-pressed]="ws.theme() === 'light'" (click)="ws.setTheme('light')"><app-ic name="sun" [size]="13" />Clair</button>
      </div>
    </section>

    @for (g of groups; track g.id) {
      @let font = store.settings()[g.id];
      <section class="set-group" [attr.aria-labelledby]="'g-' + g.id">
        <div class="set-head">
          <div class="set-title">
            <h2 class="sec-title" [id]="'g-' + g.id">{{ g.label }}</h2>
            <p class="hint">{{ g.hint }}</p>
          </div>
          <button class="btn ghost" (click)="reset(g)"><app-ic name="undo" [size]="14" />Rétablir les valeurs par défaut</button>
        </div>
        <div class="rows">
          <div class="set-row">
            <label class="lbl" [for]="'family-' + g.id">Famille</label>
            <div class="select-wrap">
              <select class="input" [id]="'family-' + g.id" (change)="setFamily(g.id, $any($event.target).value)">
                <option value="default" [selected]="font.family === 'default'">{{ g.presets.default.label }}</option>
                <option value="system" [selected]="font.family === 'system'">{{ g.presets.system.label }}</option>
                <option value="custom" [selected]="font.family === 'custom'">Personnalisée…</option>
              </select>
              <app-ic name="chev-down" [size]="14" />
            </div>
          </div>
          @if (font.family === 'custom') {
            <div class="set-row">
              <label class="lbl" [for]="'custom-' + g.id">Nom de la police</label>
              <div class="ctl">
                <input
                  class="input"
                  [class.is-bad]="invalid()[g.id]"
                  type="text"
                  [id]="'custom-' + g.id"
                  spellcheck="false"
                  autocomplete="off"
                  placeholder="Fira Code"
                  [attr.maxlength]="nameMax"
                  [attr.aria-invalid]="!!invalid()[g.id]"
                  [attr.aria-describedby]="'custom-hint-' + g.id"
                  [value]="font.custom"
                  (input)="setCustom(g.id, $event)"
                />
                @if (invalid()[g.id]) {
                  <span class="field-error" role="alert" [id]="'custom-hint-' + g.id">{{ rule }}</span>
                } @else {
                  <span class="hint" [id]="'custom-hint-' + g.id">Nom d'une police installée sur cet ordinateur. Si elle est introuvable, la police par défaut est utilisée.</span>
                }
              </div>
            </div>
          }
          <div class="set-row">
            <label class="lbl" [for]="'size-' + g.id">Taille</label>
            <div class="size">
              <input
                class="input"
                type="number"
                step="1"
                [id]="'size-' + g.id"
                [min]="g.size.min"
                [max]="g.size.max"
                [attr.aria-describedby]="'size-hint-' + g.id"
                [value]="font.size"
                (input)="setSize(g.id, $event, false)"
                (change)="setSize(g.id, $event, true)"
              />
              <span class="unit">px</span>
              <span class="hint" [id]="'size-hint-' + g.id">de {{ g.size.min }} à {{ g.size.max }} px, par défaut {{ decimal(g.size.default) }} px</span>
            </div>
          </div>
          <div class="set-row">
            <span class="lbl">Aperçu</span>
            @if (g.id === 'ui') {
              <div class="preview" aria-hidden="true">
                <strong>Collections, environnements, synchro</strong>
                <span>Le texte des menus, des listes et des boutons suit cette police et cette taille.</span>
              </div>
            } @else {
              <pre class="preview code-sample" aria-hidden="true" [style.--pv-font]="'var(' + g.variables.family + ')'" [style.--pv-size]="'var(' + g.variables.size + ')'">{{ samples[g.id] }}</pre>
            }
          </div>
        </div>
      </section>
    }
  `,
  styles: `
    :host { display: block; max-width: 640px; }
    .intro { margin: 4px 0 20px; color: var(--muted); max-width: 60ch; }
    .set-group { margin-bottom: 24px; }
    .set-head { display: flex; align-items: center; gap: 12px; margin-bottom: 8px; }
    .set-title { flex: 1; min-width: 0; }
    .set-title .hint { margin: 1px 0 0; }
    .sec-title { margin: 0; }
    .seg button { display: inline-flex; align-items: center; gap: 6px; }
    .rows { border: 1px solid var(--line); border-radius: 8px; }
    .set-row { display: grid; grid-template-columns: 130px minmax(0, 1fr); gap: 10px; align-items: start; padding: 10px 12px; border-top: 1px solid var(--line); }
    .set-row:first-child { border-top: 0; }
    .lbl { display: flex; align-items: center; min-height: 30px; color: var(--muted); }
    .select-wrap { max-width: 300px; }
    .ctl { display: flex; flex-direction: column; gap: 6px; max-width: 300px; }
    .size { display: flex; align-items: center; gap: 8px; min-height: 30px; }
    .size .input { width: 84px; }
    .unit { color: var(--muted); }
    .hint { color: var(--faint); font-size: calc(12 * var(--px)); }
    .field-error { color: var(--bad); font-size: calc(12 * var(--px)); }
    .preview { min-width: 0; margin: 0; padding: 10px 12px; border-radius: 6px; border: 1px solid var(--line); background: var(--sunken); display: flex; flex-direction: column; gap: 2px; overflow: hidden; }
    .preview span { color: var(--muted); }
    .code-sample { display: block; font: var(--pv-size) / 1.6 var(--pv-font); white-space: pre; }
  `,
})
export class AppearanceSettings {
  protected readonly ws = inject(Workspace);
  protected readonly store = inject(SettingsStore);
  protected readonly groups = FONT_GROUPS;
  protected readonly samples = SAMPLES;
  protected readonly rule = FONT_NAME_RULE;
  protected readonly nameMax = FONT_NAME_MAX;
  protected readonly decimal = (value: number) => String(value).replace('.', ',');
  protected readonly invalid = signal<Partial<Record<FontGroup, boolean>>>({});

  protected setFamily(group: FontGroup, family: FamilyChoice) {
    this.flag(group, false);
    this.store.change(group, { family });
  }

  protected setCustom(group: FontGroup, event: Event) {
    const name = (event.target as HTMLInputElement).value;
    const valid = name.trim() === '' || validFontName(name);
    this.flag(group, !valid);
    if (valid) this.store.change(group, { custom: name });
  }

  protected setSize(group: FontGroup, event: Event, commit: boolean) {
    const input = event.target as HTMLInputElement;
    const { min, max } = this.groups.find((g) => g.id === group)!.size;
    const value = input.valueAsNumber;
    if (!Number.isNaN(value) && (commit || (value >= min && value <= max))) this.store.change(group, { size: value });
    if (commit) input.value = String(this.store.settings()[group].size);
  }

  protected reset(group: FontGroupSpec) {
    this.flag(group.id, false);
    this.store.reset(group.id);
    this.ws.notify(`${group.label} : valeurs par défaut rétablies`);
  }

  private flag(group: FontGroup, invalid: boolean) {
    this.invalid.update((flags) => ({ ...flags, [group]: invalid }));
  }
}
