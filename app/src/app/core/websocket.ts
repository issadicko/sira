import type { RequestDoc, WsConnected, WsMessage, WsNotice, WsSent } from './model';

export type WsStatus = 'idle' | 'connecting' | 'open' | 'closed' | 'error';

/** Une ligne du journal : `in` reçu, `out` envoyé, `system` ce que dit l'application (connexion, fermeture, erreur). */
export interface WsLogEntry {
  n: number;
  at: string;
  direction: 'in' | 'out' | 'system';
  kind: 'text' | 'binary' | 'ping' | 'pong' | 'open' | 'close' | 'error';
  text: string;
  /** Octets du message ; 0 pour une ligne du système. */
  size: number;
}

export interface WsState {
  status: WsStatus;
  /** L'identifiant de la connexion en cours ou de la dernière. */
  connId?: string;
  info?: WsConnected;
  log: WsLogEntry[];
  /** Variables sans valeur rencontrées à la connexion ou dans un message envoyé. */
  unresolved: string[];
  next: number;
}

/** Le journal ne garde que les dernières lignes : une connexion qui pousse sans arrêt ne grossit pas sans fin. */
export const LOG_LIMIT = 2000;

export const emptyState = (): WsState => ({ status: 'idle', log: [], unresolved: [], next: 1 });

/** Les octets d'un texte en UTF-8, ce que le serveur reçoit. */
export const byteLength = (text: string): number => new TextEncoder().encode(text).length;

function push(state: WsState, entry: Omit<WsLogEntry, 'n'>): WsState {
  const log = [...state.log, { ...entry, n: state.next }];
  return { ...state, log: log.length > LOG_LIMIT ? log.slice(log.length - LOG_LIMIT) : log, next: state.next + 1 };
}

const merge = (known: string[], more: string[]): string[] => [...new Set([...known, ...more])];

/** Une nouvelle session : le journal des précédentes est effacé. */
export function connecting(connId: string): WsState {
  return { ...emptyState(), status: 'connecting', connId };
}

export function opened(state: WsState, info: WsConnected, at: string): WsState {
  const how = info.protocol ? `${info.status}, sous-protocole ${info.protocol}` : String(info.status);
  return push({ ...state, status: 'open', info, unresolved: merge(state.unresolved, info.unresolved) }, {
    at,
    direction: 'system',
    kind: 'open',
    text: `Connecté à ${info.url} (${how}) en ${Math.round(info.connectMs)} ms`,
    size: 0,
  });
}

/** La connexion n'a pas pu s'ouvrir. */
export function failed(state: WsState, message: string, at: string): WsState {
  return push({ ...state, status: 'error', info: undefined }, { at, direction: 'system', kind: 'error', text: message, size: 0 });
}

export function closeLabel(code: number | null, reason: string): string {
  const head = code === null ? 'Connexion fermée' : `Connexion fermée (${code})`;
  return reason ? `${head} : ${reason}` : head;
}

/** Applique ce que Rust relaie : un message reçu s'ajoute au journal, une fermeture ou une erreur termine la session. */
export function received(state: WsState, notice: WsNotice): WsState {
  const at = notice.at;
  switch (notice.kind) {
    case 'text':
      return push(state, { at, direction: 'in', kind: 'text', text: notice.data, size: byteLength(notice.data) });
    case 'binary':
      return push(state, { at, direction: 'in', kind: 'binary', text: notice.hex, size: notice.size });
    case 'ping':
      return push(state, { at, direction: 'in', kind: 'ping', text: '', size: notice.size });
    case 'pong':
      return push(state, { at, direction: 'in', kind: 'pong', text: '', size: notice.size });
    case 'close':
      return push({ ...state, status: 'closed' }, { at, direction: 'system', kind: 'close', text: closeLabel(notice.code, notice.reason), size: 0 });
    case 'error':
      return push({ ...state, status: 'error' }, { at, direction: 'system', kind: 'error', text: notice.message, size: 0 });
  }
}

/** Un message parti, tel que le serveur l'a reçu (variables résolues). */
export function sent(state: WsState, result: WsSent, at: string): WsState {
  return push({ ...state, unresolved: merge(state.unresolved, result.unresolved) }, {
    at,
    direction: 'out',
    kind: 'text',
    text: result.data,
    size: byteLength(result.data),
  });
}

/** Le message n'est pas parti : la connexion se ferme sous nos yeux ou l'envoi a échoué. */
export function sendFailed(state: WsState, message: string, at: string): WsState {
  return push(state, { at, direction: 'system', kind: 'error', text: `Envoi impossible : ${message}`, size: 0 });
}

/** Efface le journal sans toucher à la connexion. */
export const cleared = (state: WsState): WsState => ({ ...state, log: [] });

export const isLive = (state: WsState | undefined): boolean => state?.status === 'open' || state?.status === 'connecting';

// ---- les messages du fichier ----

export const messagesOf = (doc: RequestDoc): WsMessage[] => doc.wsMessages ?? [];

export const selectedOf = (doc: RequestDoc): WsMessage[] => messagesOf(doc).filter((m) => m.selected);

export const newMessage = (): WsMessage => ({ title: '', selected: true, kind: 'text', data: '' });

/** Le nom d'un message dans la liste : son titre, sinon le début de son contenu, sinon sa position. */
export function messageLabel(message: WsMessage, index: number): string {
  if (message.title.trim()) return message.title.trim();
  const first = message.data.trim().split('\n')[0] ?? '';
  if (first) return first.length > 40 ? `${first.slice(0, 40)}…` : first;
  return `Message ${index + 1}`;
}

export const withMessage = (doc: RequestDoc, index: number, patch: Partial<WsMessage>): RequestDoc => ({
  ...doc,
  wsMessages: messagesOf(doc).map((m, i) => (i === index ? { ...m, ...patch } : m)),
});

export const withAddedMessage = (doc: RequestDoc): RequestDoc => {
  const list = messagesOf(doc);
  if (list.length === 0) return { ...doc, wsMessages: [newMessage()] };
  // Un message sans titre ne se distingue pas dans une liste : le deuxième message donne son titre au premier.
  const named = list.length === 1 && !list[0]!.title ? [{ ...list[0]!, title: 'Message 1' }] : list;
  return { ...doc, wsMessages: [...named, { ...newMessage(), title: `Message ${named.length + 1}` }] };
};

export const withoutMessage = (doc: RequestDoc, index: number): RequestDoc => ({
  ...doc,
  wsMessages: messagesOf(doc).filter((_, i) => i !== index),
});

/** L'heure d'une ligne du journal, à la seconde près (et au millième pour les rafales). */
export function timeLabel(iso: string): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return '';
  const pad = (n: number, width = 2) => String(n).padStart(width, '0');
  return `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}.${pad(at.getMilliseconds(), 3)}`;
}

export function sizeLabel(size: number): string {
  if (size < 1024) return `${size} o`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1).replace('.', ',')} Ko`;
  return `${(size / 1024 / 1024).toFixed(1).replace('.', ',')} Mo`;
}
