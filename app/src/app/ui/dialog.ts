import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, inject, input, output } from '@angular/core';

import { matchesShortcut } from '../core/commands';
import { Icon } from './icon';

const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';
let count = 0;

/** Dialogue modal : Échap ferme, Tab reste dans le dialogue, ⌘↵ confirme, les raccourcis globaux sont suspendus. */
@Component({
  selector: 'app-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
    <div class="dialog-wrap">
      <div class="dialog" role="dialog" aria-modal="true" [attr.aria-labelledby]="titleId" (keydown)="onKey($event)">
        <div class="dialog-head">
          <h2 [id]="titleId">{{ heading() }}</h2>
          <button class="icon-btn sm" (click)="closed.emit()" aria-label="Fermer"><app-ic name="x" [size]="15" /></button>
        </div>
        <div class="dialog-body"><ng-content /></div>
        <div class="dialog-foot"><ng-content select="[dialog-foot]" /></div>
      </div>
    </div>
  `,
})
export class Dialog {
  readonly heading = input.required<string>();
  readonly closed = output<void>();
  readonly confirmed = output<void>();
  protected readonly titleId = `dialog-title-${count++}`;
  private readonly root = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private previous: HTMLElement | null = null;

  constructor() {
    afterNextRender(() => {
      this.previous = document.activeElement as HTMLElement | null;
      (this.root.querySelector<HTMLElement>('[data-autofocus]') ?? this.focusable()[0])?.focus();
    });
    inject(DestroyRef).onDestroy(() => {
      if (this.previous?.isConnected) this.previous.focus();
    });
  }

  protected onKey(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      this.closed.emit();
    } else if (event.key === 'Tab') {
      this.keepFocus(event);
    } else if (matchesShortcut('mod+enter', event)) {
      event.preventDefault();
      this.confirmed.emit();
    }
    event.stopPropagation();
  }

  private focusable(): HTMLElement[] {
    return [...this.root.querySelectorAll<HTMLElement>(FOCUSABLE)];
  }

  private keepFocus(event: KeyboardEvent) {
    const items = this.focusable();
    const [first, last] = [items[0], items[items.length - 1]];
    const edge = event.shiftKey ? first : last;
    if (document.activeElement === edge) {
      event.preventDefault();
      (event.shiftKey ? last : first)?.focus();
    }
  }
}
