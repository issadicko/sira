import type { Api } from './api';
import type { WsNotice } from './model';

type DemoWebSocket = Pick<Api, 'wsConnect' | 'wsSend' | 'wsClose' | 'onWsEvent'>;

/** Un serveur d'écho simulé pour le mode démo : il salue à la connexion, renvoie chaque texte et dit « bye » à la fermeture. */
export function createDemoWebSocket(): DemoWebSocket {
  const handlers = new Set<(notice: WsNotice) => void>();
  const open = new Set<string>();
  const emit = (notice: WsNotice) => handlers.forEach((handler) => handler(notice));
  const now = () => new Date().toISOString();
  const fill = (text: string) => text.replace(/\{\{\s*([^}]+?)\s*\}\}/g, (_, name: string) => name);
  return {
    wsConnect: async (id, _root, _path, doc) => {
      if (open.has(id)) throw 'cette connexion est déjà ouverte';
      const url = fill(doc.url).replace(/^https?:/, (scheme) => (scheme === 'https:' ? 'wss:' : 'ws:'));
      if (!/^wss?:\/\//.test(url)) throw `schéma non pris en charge : ${url.split(':')[0]}`;
      open.add(id);
      setTimeout(() => open.has(id) && emit({ id, at: now(), kind: 'text', data: 'Bienvenue sur le serveur de démonstration.' }), 120);
      return { status: 101, protocol: null, url, remoteAddr: '127.0.0.1:443', headers: [['upgrade', 'websocket'], ['connection', 'Upgrade']], connectMs: 34, unresolved: [] };
    },
    wsSend: async (id, _root, _path, _doc, _env, data) => {
      if (!open.has(id)) throw 'la connexion est fermée';
      const text = fill(data);
      setTimeout(() => open.has(id) && emit({ id, at: now(), kind: 'text', data: text }), 90);
      return { data: text, unresolved: [] };
    },
    wsClose: async (id) => {
      if (!open.delete(id)) return;
      setTimeout(() => emit({ id, at: now(), kind: 'close', code: 1000, reason: 'bye' }), 40);
    },
    onWsEvent: async (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
  };
}
