import { DecimalPipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, ElementRef, HostListener, computed, inject, signal, viewChild } from '@angular/core';

import { isTauri } from './core/api';
import { COMMANDS, isMac, shortcutLabel } from './core/commands';
import { EnvStore } from './core/env-store';
import { RunnerStore } from './core/runner-store';
import { View, Workspace } from './core/store';
import { SyncStore } from './core/sync-store';
import { TreeStore } from './core/tree-store';
import { CollectionDialog } from './ui/collection-dialog';
import { CurlDialog } from './ui/curl-dialog';
import { DeleteDialog } from './ui/delete-dialog';
import { DiscardDialog } from './ui/discard-dialog';
import { Editor } from './ui/editor';
import { EnvMenu } from './ui/env-menu';
import { EnvNameDialog } from './ui/env-name-dialog';
import { EnvSidebar } from './ui/env-sidebar';
import { EnvView } from './ui/env-view';
import { Icon } from './ui/icon';
import { methodClass, shortMethod } from './ui/method';
import { MoveDialog } from './ui/move-dialog';
import { OpenApiDialog } from './ui/openapi-dialog';
import { Palette } from './ui/palette';
import { RunnerSidebar } from './ui/runner-sidebar';
import { RunnerView } from './ui/runner-view';
import { SettingsView } from './ui/settings-view';
import { SyncSidebar } from './ui/sync-sidebar';
import { SyncView } from './ui/sync-view';
import { Tree } from './ui/tree';
import { TreeMenu } from './ui/tree-menu';
import { TreeRoot } from './ui/tree-root';
import { VarPopover } from './ui/var-popover';
import { Welcome } from './ui/welcome';

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    DecimalPipe,
    Icon,
    Tree,
    TreeMenu,
    TreeRoot,
    Editor,
    EnvView,
    EnvMenu,
    EnvNameDialog,
    EnvSidebar,
    SyncSidebar,
    SyncView,
    RunnerSidebar,
    RunnerView,
    SettingsView,
    Welcome,
    VarPopover,
    Palette,
    CurlDialog,
    OpenApiDialog,
    CollectionDialog,
    MoveDialog,
    DeleteDialog,
    DiscardDialog,
  ],
  templateUrl: './app.html',
  styleUrl: './app.css',
})
export class App {
  protected readonly ws = inject(Workspace);
  protected readonly sync = inject(SyncStore);
  protected readonly tree = inject(TreeStore);
  protected readonly envs = inject(EnvStore);
  protected readonly runner = inject(RunnerStore);
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
    const tree = this.tree;
    const envs = this.envs;
    const runner = this.runner;
    const hasTarget = () => opened() && !!tree.target();
    const inConflicts = () => ws.view() === 'sync' && this.sync.refs().length > 0;
    this.commands.register(
      { id: 'env.save', title: "Enregistrer l'environnement", group: 'Collection', icon: 'download', keys: 'mod+s', when: () => ws.view() === 'env', run: () => envs.save() },
      { id: 'request.send', title: 'Envoyer la requête', group: 'Requête', icon: 'send', keys: 'mod+enter', when: () => ws.view() === 'collections' && !!ws.active(), run: () => ws.send() },
      { id: 'request.cancel', title: "Annuler l'envoi", group: 'Requête', icon: 'x-circle', keys: 'esc', when: () => !!ws.active()?.sendingId, run: () => ws.cancel() },
      { id: 'item.new-request', title: 'Nouvelle requête', group: 'Requête', icon: 'file', when: opened, run: () => tree.beginCreate('request') },
      { id: 'request.curl', title: 'Nouvelle requête depuis cURL…', group: 'Requête', icon: 'terminal', when: opened, run: () => ws.dialog.set('curl') },
      { id: 'request.save', title: 'Enregistrer la requête', group: 'Requête', icon: 'download', keys: 'mod+s', when: () => ws.view() !== 'env' && !!ws.active(), run: () => ws.save() },
      { id: 'tab.close', title: "Fermer l'onglet", group: 'Requête', icon: 'x', keys: 'mod+w', when: () => !!ws.activePath(), run: () => ws.closeTab(ws.activePath()!) },
      { id: 'collection.open', title: 'Ouvrir une collection…', group: 'Collection', icon: 'folder-open', keys: 'mod+o', run: () => ws.pickAndOpen() },
      { id: 'collection.new', title: 'Nouvelle collection…', group: 'Collection', icon: 'plus', run: () => ws.newCollection() },
      { id: 'item.new-folder', title: 'Nouveau dossier', group: 'Collection', icon: 'folder', when: opened, run: () => tree.beginCreate('folder') },
      { id: 'item.rename', title: "Renommer l'élément actif", group: 'Collection', icon: 'pencil', when: hasTarget, run: () => tree.beginRename() },
      { id: 'item.clone', title: "Dupliquer l'élément actif", group: 'Collection', icon: 'copy', when: hasTarget, run: () => tree.beginClone() },
      { id: 'item.move', title: "Déplacer l'élément actif vers…", group: 'Collection', icon: 'arrow-right', when: hasTarget, run: () => tree.requestMove() },
      { id: 'item.delete', title: "Supprimer l'élément actif", group: 'Collection', icon: 'trash', when: hasTarget, run: () => tree.requestDelete() },
      { id: 'collection.openapi', title: 'Importer une spec OpenAPI…', group: 'Collection', icon: 'import', run: () => ws.dialog.set('openapi') },
      { id: 'collection.reload', title: 'Relire la collection sur le disque', group: 'Collection', icon: 'sync', when: opened, run: () => ws.reload() },
      { id: 'tree.filter', title: 'Filtrer les requêtes', group: 'Collection', icon: 'filter', keys: 'mod+shift+f', when: opened, run: () => this.focusFilter() },
      { id: 'env.new', title: 'Nouvel environnement…', group: 'Collection', icon: 'plus', when: opened, run: () => envs.beginNaming({ mode: 'create' }) },
      { id: 'env.rename', title: "Renommer l'environnement actif…", group: 'Collection', icon: 'pencil', when: () => opened() && !!ws.env(), run: () => envs.beginNaming({ mode: 'rename', env: ws.env()! }) },
      { id: 'env.clone', title: "Dupliquer l'environnement actif…", group: 'Collection', icon: 'copy', when: () => opened() && !!ws.env(), run: () => envs.beginNaming({ mode: 'clone', env: ws.env()! }) },
      { id: 'env.delete', title: "Supprimer l'environnement actif…", group: 'Collection', icon: 'trash', when: () => opened() && !!ws.env(), run: () => envs.remove(ws.env()!) },
      { id: 'env.default', title: "Ouvrir l'environnement actif par défaut", group: 'Collection', icon: 'check', when: () => opened() && !!ws.env() && ws.collection()?.defaultEnvironment !== ws.env(), run: () => envs.setDefault(ws.env()) },
      { id: 'view.env', title: 'Gérer les environnements', group: 'Collection', icon: 'variable', when: opened, run: () => this.show('env') },
      { id: 'runner.open', title: 'Ouvrir le runner', group: 'Runner', icon: 'play', when: opened, run: () => runner.openFor(runner.scope()) },
      { id: 'runner.run', title: 'Lancer le runner', group: 'Runner', icon: 'play', keys: 'mod+enter', when: () => opened() && ws.view() === 'runner' && !runner.running(), run: () => runner.run() },
      { id: 'runner.cancel', title: 'Annuler le run', group: 'Runner', icon: 'x-circle', keys: 'esc', when: () => ws.view() === 'runner' && runner.running(), run: () => runner.cancel() },
      { id: 'sync.run', title: 'Lancer la synchro OpenAPI', group: 'Synchro', icon: 'merge', when: opened, run: () => this.runSync() },
      { id: 'sync.connect', title: 'Connecter une spec OpenAPI…', group: 'Synchro', icon: 'import', when: opened, run: () => this.connectSync() },
      { id: 'sync.next', title: 'Conflit suivant', group: 'Synchro', icon: 'chev-down', keys: 'alt+arrowdown', when: inConflicts, run: () => this.sync.next(1) },
      { id: 'sync.previous', title: 'Conflit précédent', group: 'Synchro', icon: 'chev-up', keys: 'alt+arrowup', when: inConflicts, run: () => this.sync.next(-1) },
      { id: 'view.sidebar', title: 'Afficher ou masquer la barre latérale', group: 'Affichage', icon: 'cols', keys: 'mod+b', when: opened, run: () => ws.sidebar.update((v) => !v) },
      { id: 'view.layout', title: 'Empiler ou juxtaposer requête et réponse', group: 'Affichage', icon: 'rows', keys: 'mod+\\', when: opened, run: () => ws.stacked.update((v) => !v) },
      { id: 'view.theme', title: 'Basculer le thème clair / sombre', group: 'Affichage', icon: 'sun', run: () => ws.toggleTheme() },
      { id: 'view.settings', title: 'Ouvrir les réglages', group: 'Affichage', icon: 'settings', keys: 'mod+,', run: () => this.openSettings() },
      { id: 'palette.search', title: 'Rechercher une requête ou une commande', group: 'Palette', icon: 'search', keys: 'mod+k', run: () => this.openPalette('') },
      { id: 'palette.commands', title: 'Afficher toutes les commandes', group: 'Palette', icon: 'settings', keys: 'mod+shift+p', run: () => this.openPalette('>') },
    );
  }

  protected toggleView(view: View) {
    if (this.ws.view() === view) this.ws.sidebar.update((v) => !v);
    else this.show(view);
  }

  protected toggleSettings() {
    if (this.ws.view() === 'settings') this.ws.closeSettings();
    else this.openSettings();
  }

  protected togglePlusMenu(event: MouseEvent) {
    if (this.tree.menu()?.path === null) {
      this.tree.closeMenu();
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.tree.openMenu(rect.left, rect.bottom + 4, null);
  }

  protected openSync() {
    this.show('sync');
  }

  private runSync() {
    this.ws.view.set('sync');
    this.ws.sidebar.set(true);
    void this.sync.run();
  }

  private connectSync() {
    this.sync.beginConnect();
    this.show('sync');
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

  private openSettings() {
    this.envMenu.set(false);
    this.ws.hover.set(null);
    this.ws.openSettings();
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
    const reopened = this.ws.view() !== view;
    this.ws.view.set(view);
    this.ws.sidebar.set(true);
    if (view === 'sync') void this.sync.enter(reopened);
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

  @HostListener('window:focus')
  protected onFocus() {
    void this.ws.rescan();
  }

  /** Dans la fenêtre native, le menu contextuel de la webview n'apparaît que sur les champs de saisie. */
  @HostListener('document:contextmenu', ['$event'])
  protected onContextMenu(e: MouseEvent) {
    if (!isTauri || (e.target as HTMLElement).closest('input, textarea, [contenteditable="true"]')) return;
    e.preventDefault();
  }
}
