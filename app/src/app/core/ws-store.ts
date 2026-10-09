import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from './api';
import { Workspace } from './store';
import { WsNotice } from './model';
import { WsState, cleared, connecting, emptyState, failed, isLive, opened, received, selectedOf, sendFailed, sent } from './websocket';

/** Les connexions WebSocket des onglets : une session par requête (son journal, son état), pilotée depuis l'onglet actif. */
@Injectable({ providedIn: 'root' })
export class WsStore {
  private readonly ws = inject(Workspace);
  private readonly sessions = signal<ReadonlyMap<string, WsState>>(new Map());
  /** L'onglet de chaque connexion. */
  private readonly byConn = new Map<string, string>();
  /** Ce qui arrive avant que la connexion soit annoncée ouverte attend, pour que le journal garde l'ordre. */
  private readonly pending = new Map<string, WsNotice[]>();
  private listening: Promise<() => void> | null = null;

  /** La session de l'onglet actif. */
  readonly active = computed(() => this.sessions().get(this.ws.active()?.path ?? '') ?? emptyState());
  readonly live = computed(() => isLive(this.active()));

  constructor() {
    // Un onglet fermé, ou renommé (son chemin change), ne garde pas de connexion ouverte derrière lui.
    effect(() => {
      const open = new Set(this.ws.tabs().map((t) => t.path));
      untracked(() => {
        for (const [path, state] of this.sessions()) {
          if (open.has(path)) continue;
          if (isLive(state)) void this.close(state);
          this.forget(path);
        }
      });
    });
  }

  stateOf(path: string): WsState {
    return this.sessions().get(path) ?? emptyState();
  }

  private set(path: string, change: (state: WsState) => WsState) {
    this.sessions.update((all) => new Map(all).set(path, change(all.get(path) ?? emptyState())));
  }

  private forget(path: string) {
    this.sessions.update((all) => {
      const next = new Map(all);
      next.delete(path);
      return next;
    });
  }

  private ensureListening() {
    this.listening ??= api.onWsEvent((notice) => this.handle(notice));
    return this.listening;
  }

  private handle(notice: WsNotice) {
    const path = this.byConn.get(notice.id);
    if (!path) return;
    if (this.stateOf(path).status === 'connecting') {
      this.pending.set(notice.id, [...(this.pending.get(notice.id) ?? []), notice]);
      return;
    }
    this.apply(path, notice);
  }

  private apply(path: string, notice: WsNotice) {
    this.set(path, (state) => received(state, notice));
    if (notice.kind === 'close' || notice.kind === 'error') {
      this.byConn.delete(notice.id);
      this.pending.delete(notice.id);
    }
  }

  private close(state: WsState) {
    return state.connId ? api.wsClose(state.connId).catch(() => undefined) : Promise.resolve();
  }

  /** Ouvre la connexion de l'onglet actif avec son brouillon ; sans effet si elle est déjà ouverte. */
  async connect() {
    const collection = this.ws.collection();
    const tab = this.ws.active();
    if (!collection || !tab || tab.doc.requestType !== 'websocket' || isLive(this.stateOf(tab.path))) return;
    const id = crypto.randomUUID();
    const path = tab.path;
    this.byConn.set(id, path);
    this.set(path, () => connecting(id));
    try {
      await this.ensureListening();
      const info = await api.wsConnect(id, collection.root, path, tab.doc, this.ws.env());
      this.set(path, (state) => opened(state, info, new Date().toISOString()));
      for (const notice of this.pending.get(id) ?? []) this.apply(path, notice);
      this.pending.delete(id);
    } catch (e) {
      this.byConn.delete(id);
      this.pending.delete(id);
      this.set(path, (state) => failed(state, String(e), new Date().toISOString()));
    }
  }

  async disconnect() {
    const state = this.active();
    if (isLive(state)) await this.close(state);
  }

  /** Connecte ou déconnecte l'onglet actif, selon son état. */
  toggle() {
    return this.live() ? this.disconnect() : this.connect();
  }

  /** Envoie `data` sur la connexion de l'onglet actif ; le journal garde le texte parti, variables résolues. */
  async send(data: string) {
    const collection = this.ws.collection();
    const tab = this.ws.active();
    const state = this.active();
    if (!collection || !tab || state.status !== 'open' || !state.connId) return;
    try {
      const result = await api.wsSend(state.connId, collection.root, tab.path, tab.doc, this.ws.env(), data);
      this.set(tab.path, (current) => sent(current, result, new Date().toISOString()));
    } catch (e) {
      this.set(tab.path, (current) => sendFailed(current, String(e), new Date().toISOString()));
    }
  }

  /** Envoie les messages cochés du fichier, dans l'ordre. */
  async sendSelected() {
    const tab = this.ws.active();
    if (!tab) return;
    for (const message of selectedOf(tab.doc)) await this.send(message.data);
  }

  clear() {
    const path = this.ws.active()?.path;
    if (path) this.set(path, cleared);
  }
}
