import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';

import { api } from './api';
import { GrpcMethods, GrpcNotice } from './model';
import { Workspace } from './store';
import { GrpcState, canSend, cleared, connecting, emptyState, failed, finishedSending, isLive, opened, received, sendFailed, sent } from './grpc';

export interface MethodsState {
  loading: boolean;
  error?: string;
  data?: GrpcMethods;
}

/** Les appels gRPC des onglets : une session par requête (son journal, son état), pilotée depuis l'onglet actif. */
@Injectable({ providedIn: 'root' })
export class GrpcStore {
  private readonly ws = inject(Workspace);
  private readonly sessions = signal<ReadonlyMap<string, GrpcState>>(new Map());
  private readonly methodLists = signal<ReadonlyMap<string, MethodsState>>(new Map());
  private readonly byCall = new Map<string, string>();
  /** Ce qui arrive avant que l'ouverture soit annoncée attend, pour que le journal garde l'ordre. */
  private readonly pending = new Map<string, GrpcNotice[]>();
  private listening: Promise<() => void> | null = null;

  readonly active = computed(() => this.sessions().get(this.ws.active()?.path ?? '') ?? emptyState());
  readonly live = computed(() => isLive(this.active()));
  readonly canSend = computed(() => canSend(this.active()));
  readonly methods = computed<MethodsState>(() => this.methodLists().get(this.ws.active()?.path ?? '') ?? { loading: false });

  constructor() {
    // Un onglet fermé ne garde pas d'appel ouvert derrière lui.
    effect(() => {
      const open = new Set(this.ws.tabs().map((t) => t.path));
      untracked(() => {
        for (const [path, state] of this.sessions()) {
          if (open.has(path)) continue;
          if (isLive(state) && state.callId) void api.grpcCancel(state.callId).catch(() => undefined);
          this.sessions.update((all) => {
            const next = new Map(all);
            next.delete(path);
            return next;
          });
        }
        this.methodLists.update((all) => new Map([...all].filter(([path]) => open.has(path))));
      });
    });
  }

  stateOf(path: string): GrpcState {
    return this.sessions().get(path) ?? emptyState();
  }

  private set(path: string, change: (state: GrpcState) => GrpcState) {
    this.sessions.update((all) => new Map(all).set(path, change(all.get(path) ?? emptyState())));
  }

  private ensureListening() {
    this.listening ??= api.onGrpcEvent((notice) => this.handle(notice));
    return this.listening;
  }

  private handle(notice: GrpcNotice) {
    const path = this.byCall.get(notice.id);
    if (!path) return;
    if (this.stateOf(path).status === 'connecting') {
      this.pending.set(notice.id, [...(this.pending.get(notice.id) ?? []), notice]);
      return;
    }
    this.apply(path, notice);
  }

  private apply(path: string, notice: GrpcNotice) {
    this.set(path, (state) => received(state, notice));
    if (notice.kind === 'status' || notice.kind === 'error') {
      this.byCall.delete(notice.id);
      this.pending.delete(notice.id);
    }
  }

  /** Lance l'appel de l'onglet actif avec son brouillon ; sans effet s'il est déjà en cours. */
  async connect() {
    const collection = this.ws.collection();
    const tab = this.ws.active();
    if (!collection || !tab || tab.doc.requestType !== 'grpc' || isLive(this.stateOf(tab.path))) return;
    const id = crypto.randomUUID();
    const path = tab.path;
    this.byCall.set(id, path);
    this.set(path, () => connecting(id));
    try {
      await this.ensureListening();
      const info = await api.grpcConnect(id, collection.root, path, tab.doc, this.ws.env());
      this.set(path, (state) => opened(state, info, new Date().toISOString()));
      for (const notice of this.pending.get(id) ?? []) this.apply(path, notice);
      this.pending.delete(id);
    } catch (e) {
      this.byCall.delete(id);
      this.pending.delete(id);
      this.set(path, (state) => failed(state, String(e), new Date().toISOString()));
    }
  }

  async cancel() {
    const state = this.active();
    if (isLive(state) && state.callId) await api.grpcCancel(state.callId).catch(() => undefined);
  }

  toggle() {
    return this.live() ? this.cancel() : this.connect();
  }

  /** Envoie le message JSON `data` sur l'appel de l'onglet actif (flux client ou bidirectionnel). */
  async send(data: string) {
    const collection = this.ws.collection();
    const tab = this.ws.active();
    const state = this.active();
    if (!collection || !tab || !canSend(state) || !state.callId) return;
    try {
      const result = await api.grpcSend(state.callId, collection.root, tab.path, tab.doc, this.ws.env(), data);
      this.set(tab.path, (current) => sent(current, result, new Date().toISOString()));
    } catch (e) {
      this.set(tab.path, (current) => sendFailed(current, String(e), new Date().toISOString()));
    }
  }

  /** Termine l'envoi : le serveur répond puis donne son statut. */
  async finish() {
    const tab = this.ws.active();
    const state = this.active();
    if (!tab || !canSend(state) || !state.callId) return;
    try {
      await api.grpcFinish(state.callId);
      this.set(tab.path, finishedSending);
    } catch (e) {
      this.set(tab.path, (current) => sendFailed(current, String(e), new Date().toISOString()));
    }
  }

  clear() {
    const path = this.ws.active()?.path;
    if (path) this.set(path, cleared);
  }

  /** Charge les méthodes que la requête peut appeler (fichiers `.proto`, sinon réflexion du serveur). */
  async loadMethods() {
    const collection = this.ws.collection();
    const tab = this.ws.active();
    if (!collection || !tab || tab.doc.requestType !== 'grpc') return;
    const path = tab.path;
    const put = (state: MethodsState) => this.methodLists.update((all) => new Map(all).set(path, state));
    put({ loading: true });
    try {
      put({ loading: false, data: await api.grpcMethods(collection.root, path, tab.doc, this.ws.env()) });
    } catch (e) {
      put({ loading: false, error: String(e) });
    }
  }
}
