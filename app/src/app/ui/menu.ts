import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, HostListener, afterNextRender, inject, input, output, viewChild } from '@angular/core';

import { restoreFocus } from './focus';
import { Icon } from './icon';

export interface MenuAction {
  label: string;
  icon?: string;
  /** Raccourci affiché à droite, déjà formaté. */
  shortcut?: string;
  disabled?: boolean;
  run: () => unknown;
}

export type MenuEntry = MenuAction | 'sep';

const MARGIN = 8;

/**
 * Menu flottant : s'ouvre en `x`, `y` sans sortir de la fenêtre, flèches / Début / Fin pour parcourir, Échap ou clic ailleurs pour fermer,
 * et rend le focus à l'élément qui l'avait avant l'ouverture. Un déclencheur `aria-haspopup="menu"` déjà `aria-expanded` gère lui-même son clic.
 */
@Component({
  selector: 'app-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
    <div
      #menu
      class="menu ctx-menu"
      role="menu"
      [attr.aria-label]="label()"
      [style.left.px]="x()"
      [style.top.px]="y()"
      (keydown)="onKey($event)"
    >
      @if (heading()) {
        <div class="menu-label">{{ heading() }}</div>
      }
      @for (entry of items(); track $index) {
        @if (entry === 'sep') {
          <div class="menu-sep" role="separator"></div>
        } @else {
          <button class="menu-item" role="menuitem" [disabled]="entry.disabled" (click)="choose(entry)">
            @if (entry.icon) {
              <app-ic [name]="entry.icon" [size]="14" />
            }
            <span>{{ entry.label }}</span>
            @if (entry.shortcut) {
              <span class="sub">{{ entry.shortcut }}</span>
            }
          </button>
        }
      }
    </div>
  `,
})
export class Menu {
  readonly items = input.required<MenuEntry[]>();
  readonly x = input.required<number>();
  readonly y = input.required<number>();
  readonly label = input('');
  readonly heading = input('');
  readonly closed = output<void>();

  private readonly menu = viewChild.required<ElementRef<HTMLElement>>('menu');
  private readonly previous = document.activeElement as HTMLElement | null;

  constructor() {
    afterNextRender(() => {
      const el = this.menu().nativeElement;
      el.style.left = `${Math.max(MARGIN, Math.min(this.x(), window.innerWidth - el.offsetWidth - MARGIN))}px`;
      el.style.top = `${Math.max(MARGIN, Math.min(this.y(), window.innerHeight - el.offsetHeight - MARGIN))}px`;
      this.enabled()[0]?.focus();
    });
    inject(DestroyRef).onDestroy(() => restoreFocus(this.previous));
  }

  protected choose(action: MenuAction) {
    this.closed.emit();
    void action.run();
  }

  protected onKey(event: KeyboardEvent) {
    const items = this.enabled();
    const index = items.indexOf(document.activeElement as HTMLElement);
    const last = items.length - 1;
    switch (event.key) {
      case 'ArrowDown':
        items[index < last ? index + 1 : 0]?.focus();
        break;
      case 'ArrowUp':
        items[index > 0 ? index - 1 : last]?.focus();
        break;
      case 'Home':
        items[0]?.focus();
        break;
      case 'End':
        items[last]?.focus();
        break;
      case 'Escape':
      case 'Tab':
        this.closed.emit();
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  }

  @HostListener('document:pointerdown', ['$event'])
  protected onOutside(event: PointerEvent) {
    const target = event.target as HTMLElement;
    if (!this.menu().nativeElement.contains(target) && !target.closest('[aria-haspopup="menu"][aria-expanded="true"]')) this.closed.emit();
  }

  @HostListener('document:keydown.escape')
  @HostListener('window:blur')
  @HostListener('window:resize')
  protected dismiss() {
    this.closed.emit();
  }

  private enabled(): HTMLElement[] {
    return [...this.menu().nativeElement.querySelectorAll<HTMLElement>('[role="menuitem"]:not(:disabled)')];
  }
}
