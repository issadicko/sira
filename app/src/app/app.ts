import { DecimalPipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, ElementRef, HostListener, computed, inject, signal, viewChild } from '@angular/core';

import { isTauri } from './core/api';
import { COMMANDS, isMac, shortcutLabel } from './core/commands';
import { View, Workspace } from './core/store';
import { CurlDialog } from './ui/curl-dialog';
import { DiscardDialog } from './ui/discard-dialog';
import { Editor } from './ui/editor';
import { EnvView } from './ui/env-view';
import { Icon } from './ui/icon';
import { methodClass, shortMethod } from './ui/method';
import { OpenApiDialog } from './ui/openapi-dialog';
import { Palette } from './ui/palette';
import { Tree } from './ui/tree';
import { VarPopover } from './ui/var-popover';
import { Welcome } from './ui/welcome';

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DecimalPipe, Icon, Tree, Editor, EnvView, Welcome, VarPopover, Palette, CurlDialog, OpenApiDialog, DiscardDialog],
  templateUrl: './app.html',
  styleUrl: './app.css',
})
export class App {
  protected readonly ws = inject(Workspace);
  private readonly commands = inject(COMMANDS);
  protected readonly key = shortcutLabel;
  protected readonly nativeLights = isTauri && isMac;
  protected readonly envMenu = signal(false);
  protected readonly sidebarWidth = signal(264);
  protected readonly methodClass = methodClass;
  protected readonly shortMethod = shortMethod;
  private readonly filterInput = viewChild<ElementRef<HTMLInputElement>>('filter');

  protected readonly String = String;
  protected readonly lastResult = computed(() => this.ws.active()?.result ?? null);

  constructor() {
    const ws = this.ws;
    const opened = () => !!ws.collection();
    this.commands.register(
      { id: 'request.send', title: 'Envoyer la requête', group: 'Requête', icon: 'send', keys: 'mod+enter', when: () => !!ws.active(), run: () => ws.send() },
      { id: 'request.cancel', title: "Annuler l'envoi", group: 'Requête', icon: 'x-circle', keys: 'esc', when: () => !!ws.active()?.sendingId, run: () => ws.cancel() },
      { id: 'request.curl', title: 'Nouvelle requête depuis cURL…', group: 'Requête', icon: 'terminal', when: opened, run: () => ws.dialog.set('curl') },
      { id: 'request.save', title: 'Enregistrer la requête', group: 'Requête', icon: 'download', keys: 'mod+s', when: () => !!ws.active(), run: () => ws.save() },
      { id: 'tab.close', title: "Fermer l'onglet", group: 'Requête', icon: 'x', keys: 'mod+w', when: () => !!ws.activePath(), run: () => ws.closeTab(ws.activePath()!) },
      { id: 'collection.open', title: 'Ouvrir une collection…', group: 'Collection', icon: 'folder-open', keys: 'mod+o', run: () => ws.pickAndOpen() },
      { id: 'collection.openapi', title: 'Importer une spec OpenAPI…', group: 'Collection', icon: 'import', run: () => ws.dialog.set('openapi') },
      { id: 'collection.reload', title: 'Relire la collection sur le disque', group: 'Collection', icon: 'sync', when: opened, run: () => ws.reload() },
      { id: 'tree.filter', title: 'Filtrer les requêtes', group: 'Collection', icon: 'filter', keys: 'mod+shift+f', when: opened, run: () => this.focusFilter() },
      { id: 'view.env', title: 'Gérer les environnements', group: 'Collection', icon: 'variable', when: opened, run: () => this.show('env') },
      { id: 'view.sidebar', title: 'Afficher ou masquer la barre latérale', group: 'Affichage', icon: 'cols', keys: 'mod+b', when: opened, run: () => ws.sidebar.update((v) => !v) },
      { id: 'view.layout', title: 'Empiler ou juxtaposer requête et réponse', group: 'Affichage', icon: 'rows', keys: 'mod+\\', when: opened, run: () => ws.stacked.update((v) => !v) },
      { id: 'view.theme', title: 'Basculer le thème clair / sombre', group: 'Affichage', icon: 'sun', run: () => ws.toggleTheme() },
      { id: 'palette.search', title: 'Rechercher une requête ou une commande', group: 'Palette', icon: 'search', keys: 'mod+k', run: () => this.openPalette('') },
      { id: 'palette.commands', title: 'Afficher toutes les commandes', group: 'Palette', icon: 'settings', keys: 'mod+shift+p', run: () => this.openPalette('>') },
    );
  }

  protected toggleView(view: View) {
    if (this.ws.view() === view) this.ws.sidebar.update((v) => !v);
    else this.show(view);
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

  protected openPalette(query: string) {
    if (this.ws.modal()) return;
    this.envMenu.set(false);
    this.ws.hover.set(null);
    this.ws.palette.set(query);
  }

  protected focusFilter() {
    this.show('collections');
    setTimeout(() => this.filterInput()?.nativeElement.focus());
  }

  private show(view: View) {
    this.ws.view.set(view);
    this.ws.sidebar.set(true);
  }

  @HostListener('document:keydown', ['$event'])
  protected onKey(e: KeyboardEvent) {
    if (this.ws.palette() !== null) {
      if (e.key === 'Escape') this.ws.palette.set(null);
      return;
    }
    if (this.ws.modal()) return;
    if (e.key === 'Escape' && this.envMenu()) this.envMenu.set(false);
    else if (e.key === 'Escape' && this.ws.hover()) this.ws.hover.set(null);
    else this.commands.dispatch(e);
  }

  @HostListener('document:click', ['$event'])
  protected onClick(e: MouseEvent) {
    if (this.envMenu() && !(e.target as HTMLElement).closest('.env-wrap')) this.envMenu.set(false);
  }
}
