import { ChangeDetectionStrategy, Component, ElementRef, afterNextRender, inject, signal } from '@angular/core';

import { Workspace } from '../core/store';
import { AppearanceSettings } from './appearance-settings';
import { NetworkSettings } from './network-settings';
import { restoreFocus } from './focus';
import { Icon } from './icon';

type SectionId = 'appearance' | 'network';

const SECTIONS: { id: SectionId; label: string; icon: string }[] = [
  { id: 'appearance', label: 'Apparence', icon: 'eye' },
  { id: 'network', label: 'Réseau', icon: 'globe' },
];

/** Éditeur « Réglages » : les sections à gauche, la section choisie à droite. */
@Component({
  selector: 'app-settings-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, AppearanceSettings, NetworkSettings],
  host: { style: 'display: contents' },
  template: `
    <div class="tabs" role="tablist" aria-label="Réglages ouverts">
      <div class="tab" role="tab" aria-selected="true">
        <app-ic name="settings" [size]="14" /><span class="tab-name">Réglages</span>
        <button class="tab-x" (click)="close()" aria-label="Fermer les réglages"><span class="x"><app-ic name="x" [size]="13" /></span></button>
      </div>
    </div>
    <div class="settings">
      <nav class="settings-nav" aria-label="Sections des réglages">
        @for (s of sections; track s.id) {
          <button class="nav-item" [attr.aria-current]="section() === s.id" (click)="section.set(s.id)">
            <app-ic [name]="s.icon" [size]="15" />{{ s.label }}
          </button>
        }
      </nav>
      <div class="settings-body">
        @switch (section()) {
          @case ('appearance') {
            <app-appearance-settings />
          }
          @case ('network') {
            <app-network-settings />
          }
        }
      </div>
    </div>
  `,
  styles: `
    .settings { flex: 1; min-height: 0; display: grid; grid-template-columns: 220px minmax(0, 1fr); }
    .settings-nav { display: flex; flex-direction: column; gap: 2px; padding: 10px 8px; border-right: 1px solid var(--line); overflow: auto; }
    .nav-item { height: 28px; display: flex; align-items: center; gap: 8px; padding: 0 8px; border-radius: 6px; color: var(--muted); white-space: nowrap; transition: background 0.1s, color 0.1s; }
    .nav-item:hover { background: var(--hover); color: var(--ink); }
    .nav-item[aria-current='true'] { background: var(--accent-soft); color: var(--ink); font-weight: 500; }
    .settings-body { min-width: 0; overflow: auto; padding: 18px 24px 32px; }
  `,
})
export class SettingsView {
  protected readonly ws = inject(Workspace);
  protected readonly sections = SECTIONS;
  protected readonly section = signal<SectionId>('appearance');

  constructor() {
    const root = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    afterNextRender(() => root.querySelector<HTMLElement>('.nav-item[aria-current="true"]')?.focus());
  }

  protected close() {
    this.ws.closeSettings();
    setTimeout(() => restoreFocus(document.getElementById('act-settings')));
  }
}
