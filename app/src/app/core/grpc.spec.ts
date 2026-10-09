import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { GrpcConnected, GrpcMethod, GrpcNotice, RequestDoc } from './model.ts';
import {
  LOG_LIMIT,
  canSend,
  cleared,
  connecting,
  emptyState,
  failed,
  finishedSending,
  isLive,
  messageLabel,
  messagesOf,
  methodKind,
  opened,
  received,
  sendFailed,
  sent,
  statusLabel,
  withAddedMessage,
  withMessage,
  withMethod,
  withoutMessage,
} from './grpc.ts';

const AT = '2026-10-06T12:30:15.042Z';
const method = (extra: Partial<GrpcMethod> = {}): GrpcMethod => ({ service: 'demo.Demo', name: 'Chat', fullName: 'demo.Demo/Chat', input: 'demo.In', output: 'demo.Out', clientStreaming: true, serverStreaming: true, skeleton: '{\n  "name": ""\n}', ...extra });
const info = (extra: Partial<GrpcConnected> = {}): GrpcConnected => ({
  url: 'grpc://localhost:50051',
  remoteAddr: '127.0.0.1:50051',
  connectMs: 12.4,
  schemaSource: 'proto/demo.proto',
  method: method(),
  sent: [{ description: 'Premier', data: '{"name":"un"}' }],
  finished: false,
  unresolved: ['jeton'],
  ...extra,
});
const notice = (extra: Record<string, unknown>): GrpcNotice => ({ id: 'g1', at: AT, ...extra }) as GrpcNotice;
const doc = (messages?: RequestDoc['grpcMessages']): RequestDoc =>
  ({ name: 'Écho', requestType: 'grpc', method: '', url: 'localhost:50051', params: [], headers: [], body: { type: 'none' }, auth: { type: 'none' }, assertions: [], variables: [], scripts: [], grpcMessages: messages }) as RequestDoc;

test('ef_grpc_01_un_appel_ouvert_dit_la_methode_le_schema_et_journalise_les_messages_partis', () => {
  const state = opened(connecting('g1'), info(), AT);
  assert.equal(state.status, 'open');
  assert.equal(state.callId, 'g1');
  assert.deepEqual(state.unresolved, ['jeton']);
  assert.equal(state.log[0]!.text, 'demo.Demo/Chat sur grpc://localhost:50051 (bidi-streaming, schéma : proto/demo.proto) en 12 ms');
  assert.deepEqual(state.log.slice(1).map((l) => [l.direction, l.text]), [['out', '{"name":"un"}']]);
});

test('ef_grpc_01_un_flux_client_accepte_des_messages_tant_que_l_envoi_n_est_pas_termine', () => {
  const state = opened(connecting('g1'), info(), AT);
  assert.equal(canSend(state), true);
  assert.equal(canSend(finishedSending(state)), false);
  const unary = opened(connecting('g2'), info({ method: method({ clientStreaming: false, serverStreaming: false }), finished: true }), AT);
  assert.equal(canSend(unary), false);
  assert.equal(canSend(emptyState()), false);
});

test('ef_grpc_01_les_reponses_s_ajoutent_au_journal_et_le_statut_termine_l_appel', () => {
  let state = opened(connecting('g1'), info(), AT);
  state = received(state, notice({ kind: 'headers', headers: [['content-type', 'application/grpc']] }));
  state = received(state, notice({ kind: 'message', data: '{"message":"ok"}', size: 16 }));
  state = received(state, notice({ kind: 'status', code: 0, name: 'OK', message: '', metadata: [] }));
  assert.equal(state.status, 'done');
  assert.equal(canSend(state), false);
  assert.deepEqual(state.log.slice(2).map((l) => [l.direction, l.kind, l.text]), [
    ['system', 'headers', 'En-têtes : content-type: application/grpc'],
    ['in', 'message', '{"message":"ok"}'],
    ['system', 'status', 'Statut OK'],
  ]);
  assert.equal(isLive(state), false);
});

test('ef_grpc_01_un_statut_d_erreur_dit_son_nom_son_code_et_son_message', () => {
  assert.equal(statusLabel(5, 'NOT_FOUND', 'utilisateur introuvable'), 'Statut NOT_FOUND (5) : utilisateur introuvable');
  assert.equal(statusLabel(14, 'UNAVAILABLE', ''), 'Statut UNAVAILABLE (14)');
  const state = received(opened(connecting('g1'), info(), AT), notice({ kind: 'status', code: 5, name: 'NOT_FOUND', message: 'non', metadata: [['x-a', '1']] }));
  assert.equal(state.log.at(-1)!.text, 'Statut NOT_FOUND (5) : non · x-a: 1');
  assert.equal(state.log.at(-1)!.size, 5);
});

test('ef_grpc_01_une_erreur_de_transport_ou_d_ouverture_termine_la_session', () => {
  assert.equal(received(opened(connecting('g1'), info(), AT), notice({ kind: 'error', message: 'cassé' })).status, 'error');
  const state = failed(connecting('g1'), 'appel impossible', AT);
  assert.equal(state.status, 'error');
  assert.equal(state.info, undefined);
  assert.equal(state.log[0]!.text, 'appel impossible');
});

test('ef_grpc_01_un_message_envoye_garde_le_texte_parti_et_les_variables_manquantes', () => {
  const state = sent(opened(connecting('g1'), info(), AT), { data: '{"a":1}', unresolved: ['x'] }, AT);
  assert.deepEqual(state.unresolved, ['jeton', 'x']);
  assert.equal(state.log.at(-1)!.text, '{"a":1}');
  assert.equal(sendFailed(state, 'fermé', AT).log.at(-1)!.text, 'Envoi impossible : fermé');
});

test('ef_grpc_01_le_journal_est_borne_et_s_efface_sans_toucher_a_l_appel', () => {
  let state = opened(connecting('g1'), info({ sent: [] }), AT);
  for (let i = 0; i < LOG_LIMIT + 10; i++) state = received(state, notice({ kind: 'message', data: String(i), size: 1 }));
  assert.equal(state.log.length, LOG_LIMIT);
  assert.equal(state.log.at(-1)!.text, String(LOG_LIMIT + 9));
  const emptied = cleared(state);
  assert.equal(emptied.log.length, 0);
  assert.equal(emptied.status, 'open');
});

test('ef_grpc_01_le_type_d_appel_se_deduit_du_sens_des_flux', () => {
  assert.equal(methodKind({ clientStreaming: false, serverStreaming: false }), 'unary');
  assert.equal(methodKind({ clientStreaming: false, serverStreaming: true }), 'server-streaming');
  assert.equal(methodKind({ clientStreaming: true, serverStreaming: false }), 'client-streaming');
  assert.equal(methodKind({ clientStreaming: true, serverStreaming: true }), 'bidi-streaming');
});

test('ef_grpc_01_ajouter_un_deuxieme_message_nomme_le_premier_puis_retirer_revient_a_un_seul', () => {
  const one = withAddedMessage(doc());
  assert.deepEqual(messagesOf(one), [{ description: '', message: '{}' }]);
  const two = withAddedMessage(one);
  assert.deepEqual(messagesOf(two).map((m) => m.description), ['Message 1', 'Message 2']);
  const back = withoutMessage(two, 1);
  assert.equal(messagesOf(back).length, 1);
  assert.equal(messagesOf(withMessage(back, 0, { message: '{"a":1}' }))[0]!.message, '{"a":1}');
});

test('ef_grpc_01_un_message_se_nomme_par_sa_description_puis_son_contenu_puis_sa_position', () => {
  assert.equal(messageLabel({ description: ' Salut ', message: '{}' }, 0), 'Salut');
  assert.equal(messageLabel({ description: '', message: '{\n  "a": 1\n}' }, 0), 'Message 1');
  assert.equal(messageLabel({ description: '', message: '"texte"' }, 2), '"texte"');
});

test('ef_grpc_01_choisir_une_methode_fixe_son_nom_son_type_et_donne_un_exemple_si_aucun_message_n_est_ecrit', () => {
  const chosen = withMethod(doc(), method());
  assert.equal(chosen.method, 'demo.Demo/Chat');
  assert.equal(chosen.grpcMethodType, 'bidi-streaming');
  assert.deepEqual(messagesOf(chosen), [{ description: '', message: '{\n  "name": ""\n}' }]);
  const written = withMethod(doc([{ description: '', message: '{"name":"Ada"}' }]), method({ clientStreaming: false, serverStreaming: false }));
  assert.equal(written.grpcMethodType, 'unary');
  assert.equal(messagesOf(written)[0]!.message, '{"name":"Ada"}');
});
