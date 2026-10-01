import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, HostListener, afterNextRender, inject, input, output } from '@angular/core';

import { matchesShortcut } from '../core/commands';
import { restoreFocus } from './focus';
import { Icon } from './icon';

const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';
const opened: Dialog[] = [];
let count = 0;

/**
 * Dialogue modal : Échap ferme, Tab reste dans le dialogue, ⌘↵ confirme, quel que soit l'élément focalisé.
 * Seul le dialogue ouvert en dernier réagit au clavier ; `busy` empêche de le fermer pendant une opération.
 */
@Component({
  selector: 'app-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: { style: 'display: contents' },
  template: `
    <div class="dialog-wrap">
      <div class="dialog" role="dialog" aria-modal="true" [attr.aria-labelledby]="titleId">
        <div class="dialog-head">
          <h2 [id]="titleId">{{ heading() }}</h2>
          <button class="icon-btn sm" (click)="dismiss()" [disabled]="busy()" aria-label="Fermer"><app-ic name="x" [size]="15" /></button>
        </div>
        <div class="dialog-body"><ng-content /></div>
        <div class="dialog-foot"><ng-content select="[dialog-foot]" /></div>
      </div>
    </div>
  `,
})
export class Dialog {
  readonly heading = input.required<string>();
  readonly busy = input(false);
  readonly closed = output<void>();
  readonly confirmed = output<void>();
  protected readonly titleId = `dialog-title-${count++}`;
  private readonly root = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private previous: HTMLElement | null = null;

  constructor() {
    opened.push(this);
    afterNextRender(() => {
      this.previous = document.activeElement as HTMLElement | null;
      (this.root.querySelector<HTMLElement>('[data-autofocus]') ?? this.focusable()[0])?.focus();
    });
    inject(DestroyRef).onDestroy(() => {
      opened.splice(opened.indexOf(this), 1);
      restoreFocus(this.previous);
    });
  }

  protected dismiss() {
    if (!this.busy()) this.closed.emit();
  }

  @HostListener('document:keydown', ['$event'])
  protected onKey(event: KeyboardEvent) {
    if (opened[opened.length - 1] !== this || event.isComposing) return;
    if (event.key === 'Escape') {
      this.dismiss();
    } else if (event.key === 'Tab') {
      this.keepFocus(event);
    } else if (matchesShortcut('mod+enter', event)) {
      event.preventDefault();
      this.confirmed.emit();
    }
  }

  private focusable(): HTMLElement[] {
    return [...this.root.querySelectorAll<HTMLElement>(FOCUSABLE)];
  }

  private keepFocus(event: KeyboardEvent) {
    const items = this.focusable();
    const [first, last] = [items[0], items[items.length - 1]];
    const edge = event.shiftKey ? first : last;
    if (document.activeElement === edge || !this.root.contains(document.activeElement)) {
      event.preventDefault();
      (event.shiftKey ? last : first)?.focus();
    }
  }
}
