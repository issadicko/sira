import { Injectable, computed, inject, signal } from '@angular/core';

import { api } from './api';
import { RequestDoc, StoredSchema } from './model';
import { Workspace } from './store';

export interface SchemaState {
  schema: StoredSchema | null;
  loading: boolean;
  error: string | null;
}

const NONE: SchemaState = { schema: null, loading: false, error: null };

/** Les schémas GraphQL des requêtes ouvertes : celui que le disque garde pour l'URL, puis celui que l'introspection rapporte. */
@Injectable({ providedIn: 'root' })
export class GraphqlSchemas {
  private readonly ws = inject(Workspace);
  private readonly byPath = signal<ReadonlyMap<string, SchemaState>>(new Map());
  private readonly restores = new Map<string, number>();

  readonly current = computed(() => this.byPath().get(this.ws.activePath() ?? '') ?? NONE);

  private set(path: string, state: SchemaState) {
    this.byPath.update((map) => new Map(map).set(path, state));
  }

  private get(path: string): SchemaState {
    return this.byPath().get(path) ?? NONE;
  }

  /** Relit le schéma gardé pour l'URL actuelle de la requête, sans réseau : aucun si l'URL n'en a pas. */
  async restore(path: string, doc: RequestDoc) {
    const root = this.ws.collection()?.root;
    if (!root) return;
    const turn = (this.restores.get(path) ?? 0) + 1;
    this.restores.set(path, turn);
    try {
      const schema = await api.graphqlSchema(root, path, doc, this.ws.env());
      if (this.restores.get(path) === turn) this.set(path, { ...this.get(path), schema, error: null });
    } catch {
      if (this.restores.get(path) === turn) this.set(path, { ...this.get(path), schema: null });
    }
  }

  /** Interroge le serveur de la requête (introspection) ; en cas d'échec, le schéma déjà connu reste en place. */
  async fetch(path: string, doc: RequestDoc) {
    const root = this.ws.collection()?.root;
    if (!root || this.get(path).loading) return;
    this.set(path, { ...this.get(path), loading: true, error: null });
    try {
      const schema = await api.graphqlFetchSchema(root, path, doc, this.ws.env());
      this.set(path, { schema, loading: false, error: null });
    } catch (e) {
      this.set(path, { ...this.get(path), loading: false, error: String(e) });
    }
  }
}
