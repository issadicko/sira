import { DecimalPipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, ElementRef, HostListener, computed, inject, signal, viewChild } from '@angular/core';

import { isTauri } from './core/api';
import { Workspace } from './core/store';
import { Editor } from './ui/editor';
import { EnvView } from './ui/env-view';
import { Icon } from './ui/icon';
import { methodClass, shortMethod } from './ui/method';
import { Tree } from './ui/tree';
import { VarPopover } from './ui/var-popover';
import { Welcome } from './ui/welcome';

const isMac = navigator.userAgent.includes('Mac');

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DecimalPipe, Icon, Tree, Editor, EnvView, Welcome, VarPopover],
  templateUrl: './app.html',
  styleUrl: './app.css',
})
export class App {
  protected readonly ws = inject(Workspace);
  protected readonly mod = isMac ? '⌘' : 'Ctrl+';
  protected readonly nativeLights = isTauri && isMac;
  protected readonly envMenu = signal(false);
  protected readonly sidebarWidth = signal(264);
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;
  private readonly filterInput = viewChild<ElementRef<HTMLInputElement>>('filter');

  protected readonly String = String;
  protected readonly lastResult = computed(() => this.ws.active()?.result ?? null);

  protected toggleView(view: 'collections' | 'env') {
    if (this.ws.view() === view) this.ws.sidebar.update((v) => !v);
    else {
      this.ws.view.set(view);
      this.ws.sidebar.set(true);
    }
  }

  protected chooseEnv(env: string | null) {
    this.envMenu.set(false);
    void this.ws.setEnv(env);
  }

  protected dragSidebar(event: PointerEvent) {
    const handle = event.target as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    handle.classList.add('dragging');
    const move = (e: PointerEvent) => this.sidebarWidth.set(Math.min(460, Math.max(200, e.clientX - 44)));
    const up = () => {
      handle.classList.remove('dragging');
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', up);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up);
  }

  protected focusFilter() {
    this.ws.view.set('collections');
    this.ws.sidebar.set(true);
    setTimeout(() => this.filterInput()?.nativeElement.focus());
  }

  @HostListener('document:keydown', ['$event'])
  protected onKey(e: KeyboardEvent) {
    const mod = isMac ? e.metaKey : e.ctrlKey;
    const key = e.key.toLowerCase();
    if (mod && e.key === 'Enter') {
      e.preventDefault();
      void this.ws.send();
    } else if (mod && key === 's') {
      e.preventDefault();
      void this.ws.save();
    } else if (mod && key === 'k') {
      e.preventDefault();
      this.focusFilter();
    } else if (mod && key === 'b') {
      e.preventDefault();
      this.ws.sidebar.update((v) => !v);
    } else if (mod && e.key === '\\') {
      e.preventDefault();
      this.ws.stacked.update((v) => !v);
    } else if (mod && key === 'w' && this.ws.activePath()) {
      e.preventDefault();
      this.ws.closeTab(this.ws.activePath()!);
    } else if (mod && key === 'o') {
      e.preventDefault();
      void this.ws.pickAndOpen();
    } else if (e.key === 'Escape') {
      if (this.envMenu()) this.envMenu.set(false);
      else if (this.ws.hover()) this.ws.hover.set(null);
      else void this.ws.cancel();
    }
  }

  @HostListener('document:click', ['$event'])
  protected onClick(e: MouseEvent) {
    if (this.envMenu() && !(e.target as HTMLElement).closest('.env-wrap')) this.envMenu.set(false);
  }
}
