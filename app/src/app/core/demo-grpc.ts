import type { Api } from './api';
import type { GrpcMethod, GrpcNotice } from './model';

type DemoGrpc = Pick<Api, 'grpcConnect' | 'grpcSend' | 'grpcFinish' | 'grpcCancel' | 'grpcMethods' | 'onGrpcEvent'>;

const METHODS: GrpcMethod[] = [
  { service: 'demo.Demo', name: 'Echo', fullName: 'demo.Demo/Echo', input: 'demo.EchoRequest', output: 'demo.EchoReply', clientStreaming: false, serverStreaming: false, skeleton: '{\n  "name": "",\n  "userId": 0\n}' },
  { service: 'demo.Demo', name: 'Watch', fullName: 'demo.Demo/Watch', input: 'demo.EchoRequest', output: 'demo.EchoReply', clientStreaming: false, serverStreaming: true, skeleton: '{\n  "name": "",\n  "userId": 0\n}' },
  { service: 'demo.Demo', name: 'Upload', fullName: 'demo.Demo/Upload', input: 'demo.EchoRequest', output: 'demo.EchoReply', clientStreaming: true, serverStreaming: false, skeleton: '{\n  "name": "",\n  "userId": 0\n}' },
  { service: 'demo.Demo', name: 'Chat', fullName: 'demo.Demo/Chat', input: 'demo.EchoRequest', output: 'demo.EchoReply', clientStreaming: true, serverStreaming: true, skeleton: '{\n  "name": "",\n  "userId": 0\n}' },
];

/** Un serveur gRPC simulé pour le mode démo : `Echo`, `Watch` (trois réponses), `Upload` (compte les messages) et `Chat`. */
export function createDemoGrpc(): DemoGrpc {
  const handlers = new Set<(notice: GrpcNotice) => void>();
  const calls = new Map<string, { method: GrpcMethod; count: number }>();
  const emit = (notice: GrpcNotice) => handlers.forEach((handler) => handler(notice));
  const now = () => new Date().toISOString();
  const fill = (text: string) => text.replace(/\{\{\s*([^}]+?)\s*\}\}/g, (_, name: string) => name);
  const reply = (text: string) => JSON.stringify({ message: text, userId: 0 }, null, 2);
  const status = (id: string) => {
    calls.delete(id);
    emit({ id, at: now(), kind: 'status', code: 0, name: 'OK', message: '', metadata: [] });
  };
  const nameOf = (json: string) => {
    try {
      return String((JSON.parse(json) as { name?: string }).name ?? '');
    } catch {
      return '';
    }
  };
  return {
    grpcMethods: async () => ({ source: 'proto/demo.proto', methods: METHODS }),
    grpcConnect: async (id, _root, _path, doc) => {
      const method = METHODS.find((m) => m.fullName === doc.method);
      if (!method) throw `la méthode ${doc.method || '(vide)'} n'existe pas dans proto/demo.proto`;
      if (calls.has(id)) throw 'cet appel est déjà ouvert';
      calls.set(id, { method, count: 0 });
      const messages = (doc.grpcMessages ?? []).map((m) => ({ description: m.description, data: fill(m.message) }));
      const sent = method.clientStreaming ? messages : (messages.length ? messages : [{ description: '', data: '{}' }]).slice(0, 1);
      const finished = !method.clientStreaming;
      setTimeout(() => {
        if (!calls.has(id)) return;
        emit({ id, at: now(), kind: 'headers', headers: [['content-type', 'application/grpc']] });
        if (method.name === 'Echo') {
          emit({ id, at: now(), kind: 'message', data: reply(`salut ${nameOf(sent[0]!.data)}`), size: 24 });
          status(id);
        } else if (method.name === 'Watch') {
          [1, 2, 3].forEach((n) => setTimeout(() => calls.has(id) && emit({ id, at: now(), kind: 'message', data: reply(`w${n}`), size: 8 }), n * 120));
          setTimeout(() => calls.has(id) && status(id), 480);
        } else if (method.name === 'Chat') {
          sent.forEach((m) => emit({ id, at: now(), kind: 'message', data: reply(`re:${nameOf(m.data)}`), size: 12 }));
        }
      }, 100);
      return { url: `grpc://${fill(doc.url)}`, remoteAddr: '127.0.0.1:50051', connectMs: 21, schemaSource: 'proto/demo.proto', method, sent, finished, unresolved: [] };
    },
    grpcSend: async (id, _root, _path, _doc, _env, data) => {
      const call = calls.get(id);
      if (!call) throw "l'appel est terminé";
      const text = fill(data);
      call.count += 1;
      if (call.method.name === 'Chat') setTimeout(() => calls.has(id) && emit({ id, at: now(), kind: 'message', data: reply(`re:${nameOf(text)}`), size: 12 }), 90);
      return { data: text, unresolved: [] };
    },
    grpcFinish: async (id) => {
      const call = calls.get(id);
      if (!call) throw "l'appel est terminé";
      setTimeout(() => {
        if (!calls.has(id)) return;
        if (call.method.name === 'Upload') emit({ id, at: now(), kind: 'message', data: reply(`${call.count} reçus`), size: 16 });
        status(id);
      }, 80);
    },
    grpcCancel: async (id) => {
      if (!calls.delete(id)) return;
      setTimeout(() => emit({ id, at: now(), kind: 'status', code: 1, name: 'CANCELLED', message: 'Cancelled', metadata: [] }), 40);
    },
    onGrpcEvent: async (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
  };
}
