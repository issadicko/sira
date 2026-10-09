import type { GrpcConnected, GrpcMessage, GrpcMethod, GrpcNotice, GrpcSent, RequestDoc } from './model';

export type GrpcStatus = 'idle' | 'connecting' | 'open' | 'done' | 'error';

/** Une ligne du journal : `in` reçu, `out` envoyé, `system` ce que dit l'application (ouverture, en-têtes, statut, erreur). */
export interface GrpcLogEntry {
  n: number;
  at: string;
  direction: 'in' | 'out' | 'system';
  kind: 'message' | 'headers' | 'open' | 'status' | 'error';
  text: string;
  size: number;
}

export interface GrpcState {
  status: GrpcStatus;
  /** L'identifiant de l'appel en cours ou du dernier. */
  callId?: string;
  info?: GrpcConnected;
  /** Faux tant qu'un flux client ou bidirectionnel attend d'autres messages ; vrai une fois l'envoi terminé. */
  sendFinished: boolean;
  log: GrpcLogEntry[];
  unresolved: string[];
  next: number;
}

export const LOG_LIMIT = 2000;

export const emptyState = (): GrpcState => ({ status: 'idle', sendFinished: true, log: [], unresolved: [], next: 1 });

export const byteLength = (text: string): number => new TextEncoder().encode(text).length;

function push(state: GrpcState, entry: Omit<GrpcLogEntry, 'n'>): GrpcState {
  const log = [...state.log, { ...entry, n: state.next }];
  return { ...state, log: log.length > LOG_LIMIT ? log.slice(log.length - LOG_LIMIT) : log, next: state.next + 1 };
}

const merge = (known: string[], more: string[]): string[] => [...new Set([...known, ...more])];

/** Un nouvel appel : le journal des précédents est effacé. */
export function connecting(callId: string): GrpcState {
  return { ...emptyState(), status: 'connecting', callId, sendFinished: false };
}

/** L'appel est ouvert et les messages du fichier sont partis : ils entrent au journal dans leur ordre d'envoi. */
export function opened(state: GrpcState, info: GrpcConnected, at: string): GrpcState {
  let next = push(
    { ...state, status: 'open', info, sendFinished: info.finished, unresolved: merge(state.unresolved, info.unresolved) },
    { at, direction: 'system', kind: 'open', text: `${info.method.fullName} sur ${info.url} (${methodKind(info.method)}, schéma : ${info.schemaSource}) en ${Math.round(info.connectMs)} ms`, size: 0 },
  );
  for (const message of info.sent) {
    next = push(next, { at, direction: 'out', kind: 'message', text: message.data, size: byteLength(message.data) });
  }
  return next;
}

export function failed(state: GrpcState, message: string, at: string): GrpcState {
  return push({ ...state, status: 'error', info: undefined }, { at, direction: 'system', kind: 'error', text: message, size: 0 });
}

export function statusLabel(code: number, name: string, message: string): string {
  if (code === 0) return 'Statut OK';
  return `Statut ${name} (${code})${message ? ` : ${message}` : ''}`;
}

/** Applique ce que Rust relaie : un message s'ajoute au journal, le statut ou une erreur termine l'appel. */
export function received(state: GrpcState, notice: GrpcNotice): GrpcState {
  const at = notice.at;
  switch (notice.kind) {
    case 'headers':
      return push(state, { at, direction: 'system', kind: 'headers', text: notice.headers.length ? `En-têtes : ${notice.headers.map(([k, v]) => `${k}: ${v}`).join(' · ')}` : 'En-têtes reçus', size: 0 });
    case 'message':
      return push(state, { at, direction: 'in', kind: 'message', text: notice.data, size: notice.size });
    case 'status': {
      const extra = notice.metadata.length ? ` · ${notice.metadata.map(([k, v]) => `${k}: ${v}`).join(' · ')}` : '';
      return push({ ...state, status: 'done', sendFinished: true }, { at, direction: 'system', kind: 'status', text: statusLabel(notice.code, notice.name, notice.message) + extra, size: notice.code });
    }
    case 'error':
      return push({ ...state, status: 'error', sendFinished: true }, { at, direction: 'system', kind: 'error', text: notice.message, size: 0 });
  }
}

export function sent(state: GrpcState, result: GrpcSent, at: string): GrpcState {
  return push({ ...state, unresolved: merge(state.unresolved, result.unresolved) }, { at, direction: 'out', kind: 'message', text: result.data, size: byteLength(result.data) });
}

export function sendFailed(state: GrpcState, message: string, at: string): GrpcState {
  return push(state, { at, direction: 'system', kind: 'error', text: `Envoi impossible : ${message}`, size: 0 });
}

export const finishedSending = (state: GrpcState): GrpcState => ({ ...state, sendFinished: true });

export const cleared = (state: GrpcState): GrpcState => ({ ...state, log: [] });

export const isLive = (state: GrpcState | undefined): boolean => state?.status === 'open' || state?.status === 'connecting';

/** Vrai quand l'appel accepte d'autres messages : un flux client ou bidirectionnel dont l'envoi n'est pas terminé. */
export const canSend = (state: GrpcState): boolean => state.status === 'open' && !state.sendFinished && !!state.info?.method.clientStreaming;

export function methodKind(method: Pick<GrpcMethod, 'clientStreaming' | 'serverStreaming'>): string {
  if (method.clientStreaming && method.serverStreaming) return 'bidi-streaming';
  if (method.clientStreaming) return 'client-streaming';
  if (method.serverStreaming) return 'server-streaming';
  return 'unary';
}

export const KIND_LABELS: Record<string, string> = {
  unary: 'Unaire',
  'server-streaming': 'Flux serveur',
  'client-streaming': 'Flux client',
  'bidi-streaming': 'Bidirectionnel',
};

// ---- les messages du fichier ----

export const messagesOf = (doc: RequestDoc): GrpcMessage[] => doc.grpcMessages ?? [];

export const newMessage = (message = '{}'): GrpcMessage => ({ description: '', message });

export function messageLabel(message: GrpcMessage, index: number): string {
  if (message.description.trim()) return message.description.trim();
  const first = message.message.trim().split('\n')[0] ?? '';
  if (first && first !== '{') return first.length > 40 ? `${first.slice(0, 40)}…` : first;
  return `Message ${index + 1}`;
}

export const withMessage = (doc: RequestDoc, index: number, patch: Partial<GrpcMessage>): RequestDoc => ({
  ...doc,
  grpcMessages: messagesOf(doc).map((m, i) => (i === index ? { ...m, ...patch } : m)),
});

export const withAddedMessage = (doc: RequestDoc): RequestDoc => {
  const list = messagesOf(doc);
  if (list.length === 0) return { ...doc, grpcMessages: [newMessage()] };
  const named = list.length === 1 && !list[0]!.description ? [{ ...list[0]!, description: 'Message 1' }] : list;
  return { ...doc, grpcMessages: [...named, { ...newMessage(), description: `Message ${named.length + 1}` }] };
};

export const withoutMessage = (doc: RequestDoc, index: number): RequestDoc => ({
  ...doc,
  grpcMessages: messagesOf(doc).filter((_, i) => i !== index),
});

/** Choisit `method` pour la requête : son nom, son type d'appel, et son message d'exemple si la requête n'en a pas encore. */
export function withMethod(doc: RequestDoc, method: GrpcMethod): RequestDoc {
  const hasMessage = messagesOf(doc).some((m) => m.message.trim() !== '' && m.message.trim() !== '{}');
  return {
    ...doc,
    method: method.fullName,
    grpcMethodType: methodKind(method),
    grpcMessages: hasMessage ? messagesOf(doc) : [newMessage(method.skeleton ?? '{}')],
  };
}
