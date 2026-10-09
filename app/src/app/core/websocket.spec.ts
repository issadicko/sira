import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { RequestDoc, WsConnected, WsNotice } from './model.ts';
import {
  LOG_LIMIT,
  byteLength,
  cleared,
  closeLabel,
  connecting,
  emptyState,
  failed,
  isLive,
  messageLabel,
  messagesOf,
  opened,
  received,
  selectedOf,
  sendFailed,
  sent,
  sizeLabel,
  timeLabel,
  withAddedMessage,
  withMessage,
  withoutMessage,
} from './websocket.ts';

const AT = '2026-10-06T12:30:15.042Z';
const info: WsConnected = { status: 101, protocol: 'chat', url: 'ws://demo.test/chat', remoteAddr: '10.0.0.1:80', headers: [], connectMs: 12.4, unresolved: ['jeton'] };
const notice = (extra: Record<string, unknown>): WsNotice => ({ id: 'c1', at: AT, ...extra }) as WsNotice;
const doc = (messages?: RequestDoc['wsMessages']): RequestDoc =>
  ({ name: 'Salon', requestType: 'websocket', method: 'GET', url: 'ws://x', params: [], headers: [], body: { type: 'none' }, auth: { type: 'none' }, assertions: [], variables: [], scripts: [], wsMessages: messages }) as RequestDoc;

test('ef_ws_01_une_session_commence_vide_puis_s_ouvre_en_le_disant', () => {
  const state = opened(connecting('c1'), info, AT);
  assert.equal(state.status, 'open');
  assert.equal(state.connId, 'c1');
  assert.deepEqual(state.unresolved, ['jeton']);
  assert.equal(state.log.length, 1);
  assert.equal(state.log[0]!.text, 'Connecté à ws://demo.test/chat (101, sous-protocole chat) en 12 ms');
  assert.equal(state.log[0]!.direction, 'system');
});

test('ef_ws_01_les_messages_recus_et_envoyes_s_ajoutent_dans_l_ordre_avec_leur_taille_en_octets', () => {
  let state = opened(connecting('c1'), { ...info, protocol: null }, AT);
  state = sent(state, { data: 'salut é', unresolved: ['x'] }, AT);
  state = received(state, notice({ kind: 'text', data: 'écho' }));
  state = received(state, notice({ kind: 'binary', size: 3, hex: '01 02 03' }));
  state = received(state, notice({ kind: 'ping', size: 0 }));
  const summary = state.log.slice(1).map((l) => [l.direction, l.kind, l.text, l.size]);
  assert.deepEqual(summary, [['out', 'text', 'salut é', 8], ['in', 'text', 'écho', 5], ['in', 'binary', '01 02 03', 3], ['in', 'ping', '', 0]]);
  assert.deepEqual(state.unresolved, ['jeton', 'x'], 'les variables sans valeur s\'accumulent sans doublon');
  assert.deepEqual(state.log.map((l) => l.n), [1, 2, 3, 4, 5]);
});

test('ef_ws_01_une_fermeture_ou_une_erreur_termine_la_session_et_reste_au_journal', () => {
  const closed = received(opened(connecting('c1'), info, AT), notice({ kind: 'close', code: 4000, reason: 'bye' }));
  assert.equal(closed.status, 'closed');
  assert.equal(closed.log.at(-1)!.text, 'Connexion fermée (4000) : bye');
  assert.equal(isLive(closed), false);

  const broken = received(opened(connecting('c1'), info, AT), notice({ kind: 'error', message: 'coupure' }));
  assert.equal(broken.status, 'error');
  assert.equal(broken.log.at(-1)!.kind, 'error');
  assert.equal(closeLabel(null, ''), 'Connexion fermée');
  assert.equal(closeLabel(1000, ''), 'Connexion fermée (1000)');
});

test('ef_ws_01_une_connexion_refusee_garde_la_raison_et_efface_la_session_precedente', () => {
  const refused = failed(connecting('c2'), 'mise à niveau refusée par le serveur (HTTP 401 Unauthorized)', AT);
  assert.equal(refused.status, 'error');
  assert.equal(refused.info, undefined);
  assert.equal(refused.log.length, 1);
  assert.equal(isLive(connecting('c3')), true);
  assert.equal(connecting('c4').log.length, 0, 'une nouvelle session repart d\'un journal vide');
});

test('ef_ws_01_un_envoi_qui_echoue_est_dit_sans_fermer_la_session', () => {
  const state = sendFailed(opened(connecting('c1'), info, AT), 'la connexion est fermée', AT);
  assert.equal(state.status, 'open');
  assert.equal(state.log.at(-1)!.text, 'Envoi impossible : la connexion est fermée');
});

test('ef_ws_01_le_journal_ne_garde_que_les_dernieres_lignes', () => {
  let state = opened(connecting('c1'), info, AT);
  for (let i = 0; i < LOG_LIMIT + 50; i++) state = received(state, notice({ kind: 'text', data: String(i) }));
  assert.equal(state.log.length, LOG_LIMIT);
  assert.equal(state.log.at(-1)!.text, String(LOG_LIMIT + 49));
  assert.equal(state.log[0]!.n, state.next - LOG_LIMIT);
  assert.equal(cleared(state).log.length, 0);
  assert.equal(cleared(state).status, 'open');
});

test('ef_ws_01_les_messages_du_fichier_se_choisissent_se_nomment_et_s_editent', () => {
  const list = doc([
    { title: 'Saluer', selected: true, kind: 'json', data: '{"a":1}' },
    { title: '', selected: false, kind: 'text', data: 'ping\nsuite' },
    { title: '', selected: true, kind: 'text', data: '' },
  ]);
  assert.deepEqual(selectedOf(list).map((m) => m.title), ['Saluer', '']);
  assert.deepEqual(messagesOf(list).map(messageLabel), ['Saluer', 'ping', 'Message 3']);
  assert.equal(messageLabel({ title: '', selected: true, kind: 'text', data: 'x'.repeat(60) }, 0), `${'x'.repeat(40)}…`);
  assert.deepEqual(messagesOf(doc()), []);

  const edited = withMessage(list, 1, { data: 'pong', selected: true });
  assert.equal(edited.wsMessages![1]!.data, 'pong');
  assert.equal(list.wsMessages![1]!.data, 'ping\nsuite', 'le document d\'origine n\'est pas modifié');
  assert.equal(withoutMessage(list, 0).wsMessages!.length, 2);
});

test('ef_ws_01_ajouter_un_message_a_un_message_seul_les_titre_tous_les_deux', () => {
  const single = doc([{ title: '', selected: true, kind: 'json', data: '{}' }]);
  const two = withAddedMessage(single);
  assert.deepEqual(two.wsMessages!.map((m) => [m.title, m.selected, m.kind]), [['Message 1', true, 'json'], ['Message 2', true, 'text']]);
  assert.equal(withAddedMessage(doc()).wsMessages![0]!.title, '', 'le premier message n\'a pas de titre : le fichier garde la forme courte');
  assert.equal(withAddedMessage(doc([])).wsMessages!.length, 1);
});

test('ef_ws_01_les_tailles_et_les_heures_se_lisent', () => {
  assert.equal(byteLength('é'), 2);
  assert.equal(sizeLabel(12), '12 o');
  assert.equal(sizeLabel(2048), '2,0 Ko');
  assert.equal(sizeLabel(3 * 1024 * 1024), '3,0 Mo');
  assert.match(timeLabel(AT), /^\d{2}:\d{2}:\d{2}\.042$/);
  assert.equal(timeLabel('pas une date'), '');
  assert.equal(emptyState().status, 'idle');
});
