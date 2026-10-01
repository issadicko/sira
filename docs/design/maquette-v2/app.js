'use strict';

const isMac = /Mac|iP(hone|ad|od)/.test(navigator.platform || navigator.userAgent);
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const ic = (name, size = 16, cls = '') => `<svg class="ic ${cls}" width="${size}" height="${size}" aria-hidden="true"><use href="#i-${name}"/></svg>`;
const norm = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

function kbd(combo) {
  const map = { mod: isMac ? '⌘' : 'Ctrl', shift: isMac ? '⇧' : 'Maj', alt: isMac ? '⌥' : 'Alt', enter: '↵', esc: 'Échap' };
  return `<kbd class="kbd">${combo.split('+').map((p) => map[p] ?? p).join(isMac ? '' : '+')}</kbd>`;
}

const store = {
  get(k) { try { return localStorage.getItem(k); } catch { return null; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* stockage indisponible */ } },
};

/* ================= données de démonstration (synthétiques) ================= */

const STATUS_TEXT = { 200: 'OK', 201: 'Created', 404: 'Not Found', 409: 'Conflict', 410: 'Gone' };
const short = (m) => ({ DELETE: 'DEL', OPTIONS: 'OPT' }[m] ?? m);
const mcls = (m) => `m m-${m.toLowerCase()}`;

const VARS = {
  baseUrl: { type: 'texte', levels: { Collection: 'https://api.paiements.test/v1' } },
  txId: { type: 'texte', levels: {} },
  canal: { type: 'texte', levels: { Dossier: 'USSD' } },
  token: { type: 'secret', levels: { Runtime: 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9…' }, runtimeSrc: 'Connexion · post-réponse' },
  timeoutMs: { type: 'nombre', levels: { Collection: '10000' } },
  notifier: { type: 'booléen', levels: {} },
  marchand: { type: 'objet', levels: { Globale: '{"id":"M-001"}' } },
};

const ENVS = {
  dev: { baseUrl: 'https://api.dev.local/v1', txId: 'TX-2026-0042', canal: 'MOBILE', token: '•', timeoutMs: '5000', notifier: 'true', marchand: '{"id":"M-118","pays":"BF"}' },
  recette: { baseUrl: 'https://api.recette.paiements.test/v1', txId: 'TX-2026-0007', canal: 'MOBILE', token: '•', timeoutMs: '8000', notifier: 'false', marchand: '{"id":"M-020","pays":"BF"}' },
  prod: { baseUrl: 'https://api.paiements.example/v1', txId: '', canal: 'MOBILE', token: '•', timeoutMs: '5000', notifier: 'true', marchand: '{"id":"M-118","pays":"BF"}' },
};
const DOTENV = ['PSP_PASSWORD', 'WEBHOOK_SECRET'];

const LEVELS = [
  { key: 'Runtime', label: () => 'Runtime', src: () => 'bru.setVar()' },
  { key: 'Requête', label: () => 'Requête', src: () => 'fichier de la requête' },
  { key: 'Dossier', label: () => 'Dossier Transactions', src: () => 'transactions/folder.yml' },
  { key: 'Environnement', label: (env) => (env ? `Environnement ${env}` : 'Environnement (aucun)'), src: (env) => (env ? `environments/${env}.yml` : '—') },
  { key: 'Collection', label: () => 'Collection', src: () => 'opencollection.yml' },
  { key: 'Globale', label: () => 'Globale', src: () => 'variables globales' },
];

function resolve(name, env = S.env) {
  const v = VARS[name];
  if (!v) return null;
  const rungs = LEVELS.map((l) => {
    let val = l.key === 'Environnement' ? (env && ENVS[env] ? ENVS[env][name] : undefined) : v.levels[l.key];
    if (l.key === 'Environnement' && name === 'token' && val) val = '•••••••• trousseau';
    return { key: l.key, label: l.label(env), src: l.key === 'Runtime' && v.runtimeSrc && val !== undefined ? v.runtimeSrc : l.src(env), val: val || undefined };
  });
  const win = rungs.find((r) => r.val !== undefined) || null;
  rungs.forEach((r) => { r.state = r.val === undefined ? 'is-empty' : r === win ? 'is-win' : 'is-shadowed'; });
  return { name, type: v.type, rungs, win };
}

const TX = {
  id: 'TX-2026-0042', reference: 'PAI-7F3K-2291', montant: 15000, frais: 150, devise: 'XOF', statut: 'CONFIRMEE', canal: 'USSD',
  client: { id: 'CL-00318', nom: 'Aminata Ouédraogo', telephone: '+226 70 00 00 00', kycNiveau: 2 },
  marchand: { id: 'M-118', nom: 'Boutique Wend-Panga', pays: 'BF' },
  historique: [
    { statut: 'INITIEE', le: '2026-09-29T10:42:11Z' },
    { statut: 'AUTORISEE', le: '2026-09-29T10:42:14Z' },
    { statut: 'CONFIRMEE', le: '2026-09-29T10:42:18Z' },
  ],
  remboursable: true, remboursements: [], creeLe: '2026-09-29T10:42:11Z', misAJourLe: '2026-09-29T10:42:18Z',
};

const ACCEPT = () => ({ k: 'Accept', v: 'application/json', on: true, spec: true });
const ST200 = (v = '200') => ({ expr: 'res.status', op: 'eq', val: v, on: true });

const REQ = {
  connexion: {
    method: 'POST', name: 'Connexion', folder: 'Auth', file: 'auth/connexion.yml', op: 'login', url: '{{baseUrl}}/auth/connexion',
    headers: [ACCEPT(), { k: 'Content-Type', v: 'application/json', on: true, spec: true }], auth: 'none',
    body: '{\n  "identifiant": "caisse-ouaga-01",\n  "motDePasse": "{{process.env.PSP_PASSWORD}}"\n}',
    post: 'bru.setVar("token", res.body.token);',
    data: { token: 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9…', type: 'Bearer', expireDans: 3600, caisse: { id: 'CA-01', agence: 'Ouagadougou Centre' } },
  },
  liste: {
    method: 'GET', name: 'Liste des transactions', folder: 'Transactions', file: 'transactions/liste-transactions.yml', op: 'listTransactions', url: '{{baseUrl}}/transactions',
    query: [
      { k: 'page', v: '1', on: true, spec: true, d: 'Numéro de page' },
      { k: 'parPage', v: '20', on: true, spec: true, d: '100 au plus' },
      { k: 'statut', v: 'CONFIRMEE', on: true, spec: true, d: 'Filtre sur le statut' },
    ],
    data: { donnees: [{ id: 'TX-2026-0042', montant: 15000, statut: 'CONFIRMEE' }, { id: 'TX-2026-0041', montant: 2500, statut: 'CONFIRMEE' }, { id: 'TX-2026-0039', montant: 48000, statut: 'CONFIRMEE' }], page: 1, parPage: 20, total: 148 },
  },
  creer: {
    method: 'POST', name: 'Créer une transaction', folder: 'Transactions', file: 'transactions/creer-transaction.yml', op: 'createTransaction', url: '{{baseUrl}}/transactions',
    headers: [ACCEPT(), { k: 'Content-Type', v: 'application/json', on: true, spec: true }, { k: 'Idempotency-Key', v: '{{$uuid}}', on: true }],
    body: '{\n  "montant": 15000,\n  "devise": "XOF",\n  "canal": "{{canal}}",\n  "client": { "id": "CL-00318" },\n  "marchand": { "id": "M-118" }\n}',
    post: 'bru.setVar("txId", res.body.id);', status: 201, assertions: [ST200('201')],
    data: { id: 'TX-2026-0057', statut: 'INITIEE', montant: 15000, devise: 'XOF', creeLe: '2026-09-29T10:51:02Z' },
  },
  detail: {
    method: 'GET', name: "Détail d'une transaction", folder: 'Transactions', file: 'transactions/detail-transaction.yml', op: 'getTransaction', url: '{{baseUrl}}/transactions/{{txId}}',
    query: [
      { k: 'expand', v: 'client', on: true, spec: true, d: "Inclure l'objet client" },
      { k: 'locale', v: 'fr', on: false, d: 'Langue des libellés' },
    ],
    params: [{ k: 'id', v: '{{txId}}', spec: true, d: 'Identifiant de la transaction' }],
    headers: [ACCEPT(), { k: 'X-Canal', v: '{{canal}}', on: true, spec: true }, { k: 'X-Request-Id', v: '{{$uuid}}', on: true }],
    assertions: [ST200(), { expr: 'res.body.devise', op: 'eq', val: '"XOF"', on: true }],
    js: [{ name: 'montant est un nombre', fn: (d) => typeof d.montant === 'number', code: 'test("montant est un nombre", () => {\n  expect(res.body.montant).to.be.a("number");\n});' }],
    conflict: true, data: TX,
    docs: "Retourne une transaction par son identifiant. Avec `expand=client`, l'objet client complet est inclus.",
  },
  annuler: {
    method: 'PATCH', name: 'Annuler une transaction', folder: 'Transactions', file: 'transactions/annuler-transaction.yml', op: 'cancelTransaction', url: '{{baseUrl}}/transactions/{{txId}}/annuler',
    params: [{ k: 'id', v: '{{txId}}', spec: true, d: 'Identifiant de la transaction' }],
    headers: [ACCEPT(), { k: 'Content-Type', v: 'application/json', on: true, spec: true }],
    body: '{\n  "motif": "Erreur de saisie",\n  "canal": "USSD"\n}',
    assertions: [ST200(), { expr: 'res.body.statut', op: 'eq', val: 'ANNULEE', on: true }],
    conflict: true, data: { id: 'TX-2026-0042', statut: 'ANNULEE', motif: 'Erreur de saisie', annuleeLe: '2026-09-29T11:03:40Z' },
  },
  ancienne: {
    method: 'DELETE', name: 'Ancienne annulation', folder: 'Transactions', file: 'transactions/ancienne-annulation.yml', url: '{{baseUrl}}/transactions/{{txId}}',
    params: [{ k: 'id', v: '{{txId}}', d: 'Identifiant de la transaction' }], deprecated: true, status: 410,
    data: { erreur: 'ROUTE_RETIREE', message: 'Route retirée en v2.4.0. Utilise PATCH /transactions/{id}/annuler.' },
  },
  'cl-liste': { method: 'GET', name: 'Liste des clients', folder: 'Clients', file: 'clients/liste-clients.yml', url: '{{baseUrl}}/clients', query: [{ k: 'page', v: '1', on: true, spec: true, d: 'Numéro de page' }], data: { donnees: [{ id: 'CL-00318', nom: 'Aminata Ouédraogo' }, { id: 'CL-00317', nom: 'Issouf Sawadogo' }], total: 312 } },
  'cl-detail': { method: 'GET', name: "Détail d'un client", folder: 'Clients', file: 'clients/detail-client.yml', url: '{{baseUrl}}/clients/{{clientId}}', params: [{ k: 'id', v: '{{clientId}}', spec: true, d: 'Identifiant du client' }], status: 404, data: { erreur: 'CLIENT_INCONNU', message: 'Aucun client pour cet identifiant.' } },
  'cl-creer': { method: 'POST', name: 'Créer un client', folder: 'Clients', file: 'clients/creer-client.yml', url: '{{baseUrl}}/clients', body: '{\n  "nom": "Issouf Sawadogo",\n  "telephone": "+226 70 00 00 01"\n}', status: 201, assertions: [ST200('201')], data: { id: 'CL-00319', kycNiveau: 0 } },
  'cl-modifier': { method: 'PUT', name: 'Modifier un client', folder: 'Clients', file: 'clients/modifier-client.yml', url: '{{baseUrl}}/clients/{{clientId}}', params: [{ k: 'id', v: '{{clientId}}', spec: true, d: 'Identifiant du client' }], body: '{\n  "telephone": "+226 70 00 00 02"\n}', data: { id: 'CL-00318', misAJourLe: '2026-09-29T11:12:09Z' } },
  'cl-bloquer': { method: 'POST', name: 'Bloquer un client', folder: 'Clients', file: 'clients/bloquer-client.yml', url: '{{baseUrl}}/clients/{{clientId}}/bloquer', params: [{ k: 'id', v: '{{clientId}}', d: 'Identifiant du client' }], body: '{\n  "raison": "FRAUDE_SUSPECTEE"\n}', data: { id: 'CL-00318', bloque: true } },
  'kyc-soumettre': { method: 'POST', name: 'Soumettre un dossier', folder: 'Dossiers', file: 'dossiers/soumettre-dossier.yml', url: '{{baseUrl}}/kyc/dossiers', body: '{\n  "clientId": "CL-00318",\n  "niveau": 2\n}', status: 201, assertions: [ST200('201')], data: { id: 'KYC-0091', statut: 'EN_REVUE' } },
  'kyc-statut': { method: 'GET', name: "Statut d'un dossier", folder: 'Dossiers', file: 'dossiers/statut-dossier.yml', url: '{{baseUrl}}/kyc/dossiers/{{dossierId}}', data: { id: 'KYC-0091', statut: 'EN_REVUE' } },
  'remb-creer': { method: 'POST', name: 'Créer un remboursement', folder: 'Remboursements', file: 'remboursements/creer-remboursement.yml', url: '{{baseUrl}}/remboursements', body: '{\n  "transactionId": "{{txId}}",\n  "montant": 15000\n}', isNew: true, status: 201, assertions: [ST200('201')], data: { id: 'RB-0004', statut: 'EN_COURS' } },
  'remb-detail': { method: 'GET', name: "Détail d'un remboursement", folder: 'Remboursements', file: 'remboursements/detail-remboursement.yml', url: '{{baseUrl}}/remboursements/{{remboursementId}}', isNew: true, data: { id: 'RB-0004', statut: 'EN_COURS' } },
  'cl-plafonds': { method: 'GET', name: "Plafonds d'un client", folder: 'Clients', file: 'clients/plafonds-client.yml', url: '{{baseUrl}}/clients/{{clientId}}/plafonds', isNew: true, data: { journalier: 500000, mensuel: 2000000, devise: 'XOF' } },
};

Object.entries(REQ).forEach(([id, r]) => {
  r.id = id;
  r.query ??= [];
  r.params ??= [];
  r.headers ??= [ACCEPT()];
  r.body ??= null;
  r.auth ??= 'inherit';
  r.assertions ??= [ST200(String(r.deprecated ? 200 : r.status ?? 200))];
  r.js ??= [];
  r.pre ??= 'req.setHeader("X-Horodatage", Date.now());';
  r.post ??= 'if (res.status >= 400) {\n  console.warn(res.body.message);\n}';
});

const TREE = [
  { id: 'api', type: 'collection', name: 'API Paiements', open: true, children: [
    { id: 'f-auth', type: 'folder', name: 'Auth', open: true, children: ['connexion'] },
    { id: 'f-tx', type: 'folder', name: 'Transactions', open: true, children: ['liste', 'creer', 'detail', 'annuler', 'ancienne'] },
    { id: 'f-cl', type: 'folder', name: 'Clients', open: false, children: ['cl-liste', 'cl-detail', 'cl-creer', 'cl-modifier', 'cl-bloquer'] },
  ] },
  { id: 'kyc', type: 'collection', name: 'Back-office KYC', open: false, children: [
    { id: 'f-kyc', type: 'folder', name: 'Dossiers', open: true, children: ['kyc-soumettre', 'kyc-statut'] },
  ] },
];

function yamlOf(r, seq = 1) {
  const out = ['info:', `  name: ${r.name}`, '  type: http', `  seq: ${seq}`, 'http:', `  method: ${r.method}`, `  url: "${r.url}"`];
  if (r.query.length) {
    out.push('  params:');
    r.query.forEach((q) => { out.push(`    - name: ${q.k}`, `      value: "${q.v}"`, '      type: query'); if (!q.on) out.push('      disabled: true'); });
  }
  const headers = r.headers.filter((h) => h.k !== 'Accept');
  if (headers.length) {
    out.push('  headers:');
    headers.forEach((h) => out.push(`    - name: ${h.k}`, `      value: ${/[{"]/.test(h.v) ? `"${h.v.replace(/"/g, '\\"')}"` : h.v}`));
  }
  if (r.body) out.push('  body:', '    type: json', '    data: |-', ...r.body.split('\n').map((l) => `      ${l}`));
  out.push(`  auth: ${r.auth === 'none' ? 'none' : 'inherit'}`);
  const asserts = r.assertions.filter((a) => !(a.expr === 'res.status' && r.assertions.length === 1 && !r.js.length && r.id !== 'annuler'));
  if (asserts.length) {
    out.push('runtime:', '  assertions:');
    asserts.forEach((a) => out.push(`    - expression: ${a.expr}`, `      operator: ${a.op}`, `      value: ${/^"/.test(a.val) || /^\d+$/.test(a.val) ? `"${a.val.replace(/"/g, '')}"` : a.val}`));
  }
  out.push('settings:', '  encodeUrl: true');
  return out.join('\n');
}

const HEADS = {};
Object.values(REQ).forEach((r, i) => { HEADS[r.id] = yamlOf(r, i + 1); });
HEADS.detail = yamlOf({ ...REQ.detail, query: REQ.detail.query.filter((q) => q.k !== 'locale') }, 3);
HEADS.annuler = yamlOf({ ...REQ.annuler, body: REQ.annuler.body.replace('Erreur de saisie', 'Doublon'), assertions: [ST200()] }, 4);

const DEV_HEAD = 'name: dev\nvariables:\n  - name: baseUrl\n    value: https://api.dev.local/v1\n  - name: txId\n    value: TX-2026-0042\n  - name: canal\n    value: MOBILE\n  - name: timeoutMs\n    value: "10000"\n  - name: marchand\n    value: \'{"id":"M-118","pays":"BF"}\'\n  - name: token\n    secret: true';
const DEV_WORK = DEV_HEAD.replace('"10000"', '"5000"').replace('  - name: marchand', '  - name: notifier\n    value: "true"\n  - name: marchand');
const SOURCE_HEAD = 'source: ./spec/openapi.yml\nversion: 2.3.0\nsha256: 9f2c0d71e4b8a6f31c5e02d9b7a84f1d41ab\noperations: 14';
const SOURCE_WORK = 'source: ./spec/openapi.yml\nversion: 2.4.0\nsha256: 4be17a90c3f2e85d16b0a7c9e4d23f8bc07d\noperations: 16';

const CONFLICTS = [
  {
    id: 'c1', req: 'annuler', method: 'PATCH', path: '/transactions/{id}/annuler', field: 'corps › exemple', file: 'transactions/annuler-transaction.yml', start: 5,
    before: ['http:', '  method: PATCH', '  url: "{{baseUrl}}/transactions/{{txId}}/annuler"', '  body:', '    type: json', '    data: |-', '      {'],
    after: ['      }', '  auth: inherit'],
    base: [{ t: '        "motif": "ERREUR",' }, { t: '        "canal": "USSD"' }],
    ours: [{ t: '        "motif": "Erreur de saisie",', c: 1 }, { t: '        "canal": "USSD"' }],
    theirs: [{ t: '        "motif": "ERREUR_SAISIE",', c: 1 }, { t: '        "canal": "USSD",', c: 1 }, { t: '        "notifier": true', c: 1 }],
    both: [{ t: '        "motif": "Erreur de saisie",', o: 'o' }, { t: '        "canal": "USSD",', o: 'o' }, { t: '        "notifier": true', o: 't' }],
    why: "L'équipe a réécrit le motif, la spec en fait une énumération et ajoute « notifier ».",
  },
  {
    id: 'c2', req: 'detail', method: 'GET', path: '/transactions/{id}', field: 'en-têtes › X-Canal', file: 'transactions/detail-transaction.yml', start: 12,
    before: ['  headers:'],
    after: ['    - name: X-Request-Id', '      value: "{{$uuid}}"', '  auth: inherit'],
    base: [{ t: '    - name: X-Canal' }, { t: '      value: WEB' }],
    ours: [{ t: '    - name: X-Canal' }, { t: '      value: "{{canal}}"', c: 1 }],
    theirs: [{ t: '    - name: X-Canal' }, { t: '      value: MOBILE', c: 1 }, { t: '    - name: X-Version', c: 1 }, { t: '      value: "2"', c: 1 }],
    both: [{ t: '    - name: X-Canal', o: 'o' }, { t: '      value: "{{canal}}"', o: 'o' }, { t: '    - name: X-Version', o: 't' }, { t: '      value: "2"', o: 't' }],
    why: "L'équipe a rendu le canal variable, la spec le fixe à MOBILE et ajoute X-Version.",
  },
];

const AUTO = [
  { method: 'GET', path: '/transactions', state: 'spec appliquée', cls: 'good' },
  { method: 'POST', path: '/transactions', state: 'équipe gardée', cls: '' },
  { method: 'GET', path: '/clients', state: 'équipe gardée', cls: '' },
  { method: 'GET', path: '/clients/{id}', state: 'spec appliquée', cls: 'good' },
  { method: 'PUT', path: '/clients/{id}', state: 'spec appliquée', cls: 'good' },
];
const NEW_OPS = [
  { method: 'POST', path: '/remboursements', state: '→ Remboursements' },
  { method: 'GET', path: '/remboursements/{id}', state: '→ Remboursements' },
  { method: 'GET', path: '/clients/{id}/plafonds', state: '→ Clients' },
];
const CHOICE_LABEL = { ours: "Garder l'équipe", theirs: 'Prendre la spec', both: 'Combiner les deux', edit: 'Éditer à la main' };

/* ================= état ================= */

const S = {
  view: 'collections', sidebar: true, env: 'dev', envSel: 'dev', envVar: 'baseUrl', layout: 'h', reqPct: 46, reqhPct: 44,
  tabs: ['detail', 'connexion', 'annuler'], active: 'detail', preview: 'annuler', dirty: new Set(['detail']),
  reqTab: 'params', resTab: 'body', open: {}, filter: '', histOpen: true,
  responses: {}, sending: null, warm: false, jsonFilter: '', folds: new Set(), wrap: false, raw: false,
  choices: {}, edits: {}, conflict: 'c1', showBase: false, synced: false,
  scm: [], scmSel: 'annuler', commitMsg: '', commitError: false, ahead: 1, lang: 'FR',
  history: [
    { id: 'detail', t: '10:42', code: 200 },
    { id: 'connexion', t: '10:41', code: 200 },
    { id: 'creer', t: '10:38', code: 201 },
    { id: 'annuler', t: '10:35', code: 409 },
    { id: 'liste', t: '10:31', code: 200 },
  ],
};

S.scm = [
  { key: 'annuler', req: 'annuler', file: 'transactions/annuler-transaction.yml', st: 'M', head: () => HEADS.annuler, work: () => yamlOf(REQ.annuler, 4) },
  { key: 'dev', file: 'environments/dev.yml', st: 'M', head: () => DEV_HEAD, work: () => DEV_WORK },
  { key: 'cl-bloquer', req: 'cl-bloquer', file: 'clients/bloquer-client.yml', st: 'A', head: () => '', work: () => yamlOf(REQ['cl-bloquer'], 5) },
];

/* ================= réponses ================= */

function phasesOf(ms, cold) {
  const w = cold ? [0.03, 0.08, 0.2, 0.61] : [0, 0, 0, 0.88];
  const p = w.map((x) => Math.round(ms * x));
  return { dns: p[0], tcp: p[1], tls: p[2], wait: p[3], dl: ms - p.reduce((a, b) => a + b, 0) };
}

function buildResponse(id, { ms, cold = true } = {}) {
  const r = REQ[id];
  let data = r.data ?? (r.method === 'GET' ? { donnees: [], total: 0 } : { ok: true });
  if (id === 'detail' && !r.query.find((q) => q.k === 'expand')?.on) data = { ...data, client: { id: data.client.id } };
  const text = JSON.stringify(data, null, 2);
  const total = ms ?? (cold ? 120 : 70) + Math.round(Math.random() * 60);
  return { status: r.status ?? 200, data, bytes: new Blob([text]).size, ms: total, phases: phasesOf(total, cold), reused: !cold };
}

S.responses.detail = buildResponse('detail', { ms: 142 });
S.responses.connexion = buildResponse('connexion', { ms: 96 });
S.responses.liste = buildResponse('liste', { ms: 118 });

const fmtSize = (b) => (b < 1024 ? `${b} o` : `${(b / 1024).toLocaleString('fr-FR', { maximumFractionDigits: 1 })} Ko`);
const hhmm = () => new Date().toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });

function testResults(r, res) {
  const get = (expr) => {
    if (expr === 'res.status') return res.status;
    return expr.replace(/^res\.body\.?/, '').split('.').filter(Boolean).reduce((o, k) => (o == null ? undefined : o[k]), res.data);
  };
  const asserts = r.assertions.filter((a) => a.on).map((a) => {
    const actual = get(a.expr);
    const ok = a.op === 'eq' ? String(actual) === a.val.replace(/"/g, '') : a.op === 'isNumber' ? typeof actual === 'number' : false;
    const label = a.expr === 'res.status' ? `statut ${a.val}` : `${a.expr.replace('res.body.', '')} = ${a.val.replace(/"/g, '')}`;
    return { name: label, src: `${a.expr} ${a.op} ${a.val}`, ok, actual };
  });
  const js = r.js.map((t) => ({ name: t.name, src: 'test()', ok: t.fn(res.data) }));
  return [...asserts, ...js];
}

/* ================= coloration ================= */

const varsInline = (escaped) => escaped.replace(/\{\{([^}]+)\}\}/g, (_, n) => `<span class="t-var" data-var="${n}">{{${n}}}</span>`);

function hlJson(line) {
  return esc(line).replace(/(&quot;(?:[^&]|&(?!quot;))*?&quot;)(\s*:)?|\b(true|false|null)\b|(-?\b\d+(?:\.\d+)?\b)/g, (m, str, colon, kw, num) => {
    if (str) return colon ? `<span class="t-key">${str}</span>${colon}` : `<span class="t-str">${varsInline(str)}</span>`;
    if (kw) return `<span class="t-kw">${kw}</span>`;
    return `<span class="t-num">${num}</span>`;
  });
}

function hlScalar(raw) {
  const lead = raw.match(/^\s*/)[0];
  const v = raw.slice(lead.length);
  if (!v) return esc(raw);
  let cls = '';
  if (/^(".*"|'.*')$/.test(v)) cls = 't-str';
  else if (/^(true|false|null|\|-)$/.test(v)) cls = 't-kw';
  else if (/^-?\d+(\.\d+)?$/.test(v)) cls = 't-num';
  const body = varsInline(esc(v));
  return esc(lead) + (cls ? `<span class="${cls}">${body}</span>` : body);
}

function hlYaml(line) {
  if (/^\s*["{}[\]]/.test(line)) return hlJson(line);
  const m = line.match(/^(\s*(?:- )?)([\w.$-]+)(:)(.*)$/);
  if (!m) return hlScalar(line);
  return `${esc(m[1])}<span class="t-key">${esc(m[2])}</span>${m[3]}${hlScalar(m[4])}`;
}

function hlJs(line) {
  return esc(line).replace(/(\/\/.*$)|(&quot;.*?&quot;|`[^`]*`)|\b(const|let|if|else|return|await|async|function|true|false|null|new)\b|\b(bru|req|res|expect|test|console)\b|(\b\d+\b)/g,
    (all, com, str, kw, glob, num) => (com ? `<span class="t-com">${com}</span>` : str ? `<span class="t-str">${str}</span>` : kw ? `<span class="t-kw">${kw}</span>` : glob ? `<span class="t-fn">${glob}</span>` : `<span class="t-num">${num}</span>`));
}

function codeBlock(lines, { hl = esc, start = 1, cls = '' } = {}) {
  return `<div class="code ${cls}">${lines.map((l, i) => {
    const o = typeof l === 'string' ? { t: l } : l;
    return `<div class="ln ${o.cls ?? ''}"><span class="ln-no">${o.no ?? start + i}</span><span class="ln-g">${o.g ?? ''}</span><span class="ln-t">${o.html ?? hl(o.t)}</span></div>`;
  }).join('')}</div>`;
}

function jsonModel(value) {
  const lines = JSON.stringify(value, null, 2).split('\n');
  const pairs = {};
  const stack = [];
  lines.forEach((l, i) => {
    const t = l.trim();
    if (/^[}\]]/.test(t)) { const o = stack.pop(); if (o !== undefined && i - o > 1) pairs[o] = i; }
    if (/[{[]$/.test(t)) stack.push(i);
  });
  return { lines, pairs };
}

function jsonCode(value, foldKey) {
  const { lines, pairs } = jsonModel(value);
  let out = '';
  let i = 0;
  while (i < lines.length) {
    const fid = `${foldKey}:${i}`;
    const canFold = pairs[i] !== undefined;
    const folded = canFold && S.folds.has(fid);
    const g = canFold ? `<button data-act="fold" data-fold="${fid}" class="${folded ? 'folded' : ''}" aria-label="${folded ? 'Déplier' : 'Replier'}">${ic(folded ? 'chev-right' : 'chev-down', 12)}</button>` : '';
    let text = hlJson(lines[i]);
    if (folded) text += `<span class="fold-pill" data-act="fold" data-fold="${fid}">${pairs[i] - i - 1} lignes</span>${esc(lines[pairs[i]].trim())}`;
    out += `<div class="ln"><span class="ln-no">${i + 1}</span><span class="ln-g">${g}</span><span class="ln-t">${text}</span></div>`;
    i = folded ? pairs[i] + 1 : i + 1;
  }
  return `<div class="code ${S.wrap ? 'wrap' : ''}">${out}</div>`;
}

function jsonPath(obj, path) {
  const p = path.trim();
  if (!p || p === '$') return { ok: true, value: obj };
  if (!/^\$(\.[\w$À-ÿ-]+|\[\d+\])+$/.test(p)) return { ok: false, error: 'Expression invalide' };
  let cur = obj;
  for (const [, key, idx] of p.slice(1).matchAll(/\.([\w$À-ÿ-]+)|\[(\d+)\]/g)) {
    cur = cur == null ? undefined : idx !== undefined ? cur[Number(idx)] : cur[key];
    if (cur === undefined) return { ok: false, error: `Rien à ${p}` };
  }
  return { ok: true, value: cur };
}

function diffLines(a, b) {
  const n = a.length;
  const m = b.length;
  const dp = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const ops = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) { ops.push({ k: 'ctx', l: a[i++], r: b[j++] }); }
    else if (dp[i + 1][j] >= dp[i][j + 1]) ops.push({ k: 'del', l: a[i++] });
    else ops.push({ k: 'add', r: b[j++] });
  }
  while (i < n) ops.push({ k: 'del', l: a[i++] });
  while (j < m) ops.push({ k: 'add', r: b[j++] });
  const rows = [];
  for (let x = 0; x < ops.length;) {
    if (ops[x].k === 'ctx') { rows.push(ops[x++]); continue; }
    const dels = [];
    const adds = [];
    while (x < ops.length && ops[x].k !== 'ctx') { (ops[x].k === 'del' ? dels : adds).push(ops[x]); x++; }
    for (let y = 0; y < Math.max(dels.length, adds.length); y++) rows.push({ k: 'chg', l: dels[y]?.l ?? null, r: adds[y]?.r ?? null });
  }
  return rows;
}

function markWords(a, b, cls) {
  let p = 0;
  while (p < a.length && p < b.length && a[p] === b[p]) p++;
  let s = 0;
  while (s < a.length - p && s < b.length - p && a[a.length - 1 - s] === b[b.length - 1 - s]) s++;
  const mid = a.slice(p, a.length - s);
  return hlYaml(a.slice(0, p)) + (mid ? `<span class="${cls}">${esc(mid)}</span>` : '') + esc(a.slice(a.length - s));
}

/* ================= rendu : barre de titre, activité, statut ================= */

function renderTitlebar() {
  $('#ccKbd').innerHTML = kbd('mod+K');
  const n = CONFLICTS.filter((c) => !S.choices[c.id]).length;
  const chip = $('#specChip');
  chip.hidden = S.synced;
  chip.innerHTML = `${ic('alert', 14)}<span class="lbl-long">Spec OpenAPI :</span><span>${n ? `${n} conflit${n > 1 ? 's' : ''}` : 'prête à appliquer'}</span>`;
  chip.title = 'Ouvrir la synchro OpenAPI';
  const env = $('#envBtn');
  env.classList.toggle('is-prod', S.env === 'prod');
  env.innerHTML = `<span class="env-dot ${S.env ?? 'none'}"></span><span>${S.env ?? 'Aucun environnement'}</span>${ic('chev-down', 14)}`;
  env.setAttribute('aria-label', `Environnement actif : ${S.env ?? 'aucun'}`);
  const lb = $('#layoutBtn');
  lb.innerHTML = ic(S.layout === 'h' ? 'cols' : 'rows', 16);
  lb.title = `${S.layout === 'h' ? 'Empiler requête et réponse' : 'Requête et réponse côte à côte'} (${isMac ? '⌘\\' : 'Ctrl+\\'})`;
  lb.setAttribute('aria-label', lb.title);
  const tb = $('#themeBtn');
  const dark = document.documentElement.dataset.theme !== 'light';
  tb.innerHTML = ic(dark ? 'sun' : 'moon', 16);
  tb.title = dark ? 'Passer au thème clair' : 'Passer au thème sombre';
  tb.setAttribute('aria-label', tb.title);
}

function renderActivity() {
  const conflicts = CONFLICTS.filter((c) => !S.choices[c.id]).length;
  const items = [
    { view: 'collections', icon: 'layers', label: 'Collections' },
    { view: 'env', icon: 'variable', label: 'Environnements et variables' },
    { view: 'scm', icon: 'branch', label: 'Source Control', badge: S.scm.length || '' },
    { view: 'sync', icon: 'merge', label: 'Synchro OpenAPI', badge: S.synced ? '' : conflicts || '', warn: true },
  ];
  $('#activity').innerHTML = items.map((a) => `
    <button class="act" data-act="go" data-view="${a.view}" aria-current="${S.view === a.view}" title="${a.label}" aria-label="${a.label}${a.badge ? ` (${a.badge})` : ''}">
      ${ic(a.icon, 20)}${a.badge ? `<span class="act-badge ${a.warn ? 'warn' : ''}">${a.badge}</span>` : ''}
    </button>`).join('') + `
    <span class="act-spacer"></span>
    <button class="act" data-act="palette" data-prefix=">" title="Commandes et réglages" aria-label="Commandes et réglages">${ic('settings', 19)}</button>`;
}

function renderStatus() {
  const n = CONFLICTS.filter((c) => !S.choices[c.id]).length;
  const res = S.responses[S.active];
  const spec = S.synced
    ? `<button class="sb-item good" data-act="go" data-view="sync">${ic('check', 13)}Spec à jour · v2.4.0</button>`
    : `<button class="sb-item" data-act="go" data-view="sync"><span class="dot warn"></span>Spec v2.4.0 : ${n ? `${n} conflit${n > 1 ? 's' : ''}` : 'prête'}</button>`;
  $('#statusbar').innerHTML = `
    <button class="sb-item" data-act="go" data-view="scm">${ic('branch', 13)}main${S.ahead ? `<span class="faint">↑${S.ahead}</span>` : ''}</button>
    <button class="sb-item" data-act="go" data-view="scm">${S.scm.length ? `${S.scm.length} modification${S.scm.length > 1 ? 's' : ''}` : 'Arbre propre'}</button>
    ${spec}
    <span class="sb-item">${ic('disk', 13)}Disque surveillé</span>
    <span class="sb-spacer"></span>
    ${S.view === 'collections' && res && !res.cancelled ? `<button class="sb-item" data-act="res-tab" data-tab="timeline">${res.status} · ${res.ms} ms</button>` : ''}
    <button class="sb-item" data-act="env-menu" aria-haspopup="menu">${ic('variable', 13)}${S.env ?? 'aucun'}</button>
    <span class="sb-item">${ic('shield', 13)}Sandbox QuickJS</span>
    <span class="sb-item">OpenCollection YAML</span>
    <button class="sb-item" data-act="lang">${ic('lang', 13)}${S.lang}</button>`;
}

/* ================= barre latérale ================= */

function countReqs(n) { return n.children.reduce((s, c) => s + (typeof c === 'string' ? 1 : countReqs(c)), 0); }
const gitOf = (id) => S.scm.find((f) => f.req === id)?.st;

function highlight(text, q) {
  if (!q) return esc(text);
  const i = norm(text).indexOf(norm(q));
  if (i < 0) return esc(text);
  return `${esc(text.slice(0, i))}<mark>${esc(text.slice(i, i + q.length))}</mark>${esc(text.slice(i + q.length))}`;
}

function reqRow(id, depth) {
  const r = REQ[id];
  const active = S.active === id && S.view === 'collections';
  const git = gitOf(id);
  return `<button class="row req ${active ? 'is-active' : ''} ${r.deprecated ? 'is-deprecated' : ''}" data-act="open" data-id="${id}" style="--d:${depth}" title="${esc(r.file)}">
    <span class="${mcls(r.method)}">${short(r.method)}</span>
    <span class="row-name">${highlight(r.name, S.filter)}</span>
    ${r.deprecated ? '<span class="tag">dépréciée</span>' : ''}
    ${r.isNew ? '<span class="tag new">nouvelle</span>' : ''}
    ${r.conflict && !S.synced ? `<span class="tree-conflict" title="Conflit avec la spec OpenAPI">${ic('alert', 13)}</span>` : ''}
    ${git ? `<span class="git git-${git}" title="${git === 'A' ? 'Ajouté' : 'Modifié'} (Git)">${git}</span>` : ''}
  </button>`;
}

function treeHTML(nodes, depth) {
  const q = norm(S.filter);
  return nodes.map((n) => {
    if (typeof n === 'string') {
      const r = REQ[n];
      return !q || norm(`${r.name} ${r.url}`).includes(q) ? reqRow(n, depth) : '';
    }
    const inner = treeHTML(n.children, depth + 1);
    if (q && !inner) return '';
    const open = q ? true : S.open[n.id] ?? n.open;
    const icon = n.type === 'collection' ? 'box' : open ? 'folder-open' : 'folder';
    return `<button class="row ${n.type}" data-act="toggle" data-id="${n.id}" aria-expanded="${open}" style="--d:${depth}">
      <span class="twist">${ic(open ? 'chev-down' : 'chev-right', 13)}</span>${ic(icon, 15)}
      <span class="row-name">${esc(n.name)}</span>
      ${n.isNew ? '<span class="tag new">nouveau</span>' : ''}
      <span class="row-meta">${countReqs(n)}</span>
    </button>${open ? `<div class="group" style="--d:${depth}">${inner}</div>` : ''}`;
  }).join('');
}

function sidebarCollections() {
  return `
    <div class="pane-head">
      <span class="pane-title">Collections</span>
      <button class="icon-btn sm" data-act="toast" data-msg="Nouvelle requête : écran non maquetté." title="Nouvelle requête (${isMac ? '⌘N' : 'Ctrl+N'})" aria-label="Nouvelle requête">${ic('plus', 15)}</button>
      <button class="icon-btn sm" data-act="palette" data-prefix=">Importer" title="Importer cURL, Postman, Insomnia, OpenAPI, Bruno" aria-label="Importer">${ic('import', 15)}</button>
      <button class="icon-btn sm" data-act="collapse-all" title="Tout replier" aria-label="Tout replier">${ic('more', 15)}</button>
    </div>
    <label class="sb-filter">${ic('filter', 13)}<input id="treeFilter" type="text" placeholder="Filtrer par nom ou URL" value="${esc(S.filter)}" aria-label="Filtrer les requêtes" spellcheck="false"></label>
    <div class="sb-scroll" id="tree">${treeHTML(TREE, 0) || '<p class="pal-empty">Aucune requête ne correspond.</p>'}</div>
    <div class="sb-section">
      <button class="sb-section-head" data-act="hist" aria-expanded="${S.histOpen}">${ic(S.histOpen ? 'chev-down' : 'chev-right', 13)}Historique<span class="row-meta" style="margin-left:auto">${S.history.length}</span></button>
      ${S.histOpen ? `<div class="hist">${S.history.slice(0, 5).map((h) => {
        const r = REQ[h.id];
        return `<button class="hist-row" data-act="open" data-id="${h.id}" data-pin="1" title="${esc(r.name)}">
          <span class="hist-time">${h.t}</span><span class="${mcls(r.method)}">${short(r.method)}</span>
          <span class="hist-path">${esc(r.url.replace('{{baseUrl}}', ''))}</span><span class="hist-code c-${String(h.code)[0]}">${h.code}</span>
        </button>`;
      }).join('')}</div>` : ''}
    </div>`;
}

function sidebarEnv() {
  const envRow = (id, label, meta, icon, extra = '') => `<button class="row env-row ${S.envSel === id ? 'is-active' : ''}" data-act="env-sel" data-env="${id}" style="--d:0">
      ${icon}<span class="row-name">${label}</span>${extra}<span class="row-meta">${meta}</span></button>`;
  return `
    <div class="pane-head"><span class="pane-title">Environnements</span>
      <button class="icon-btn sm" data-act="toast" data-msg="Nouvel environnement : écran non maquetté." aria-label="Nouvel environnement" title="Nouvel environnement">${ic('plus', 15)}</button></div>
    <div class="sb-scroll">
      ${envRow('globales', 'Variables globales', 1, ic('globe', 15))}
      ${Object.keys(ENVS).map((e) => envRow(e, e, Object.keys(ENVS[e]).length, `<span class="env-dot ${e}" style="margin:0 4px"></span>`, `${S.env === e ? '<span class="tag good">actif</span>' : ''}${e === 'prod' ? '<span class="tag bad">production</span>' : ''}`)).join('')}
      <div class="group-head" style="margin-top:12px">${ic('lock', 13)}.env local<span class="n">ignoré par Git</span></div>
      ${DOTENV.map((k) => `<div class="row" style="--d:0;cursor:default"><span class="mono row-name" style="font-size:12px">process.env.${k}</span><span class="row-meta">masqué</span></div>`).join('')}
    </div>`;
}

function sidebarScm() {
  return `
    <div class="pane-head"><span class="pane-title">Source Control</span>
      <button class="icon-btn sm" data-act="toast" data-msg="Pull effectué : déjà à jour avec origin/main." title="Pull" aria-label="Pull">${ic('arrow-down', 15)}</button></div>
    <div class="branch-row">${ic('branch', 14)}<span class="mono">main</span><span class="faint">${S.ahead ? `↑${S.ahead} à pousser` : 'à jour avec origin'}</span></div>
    <div class="commit">
      <textarea id="commitMsg" placeholder="Message de commit (${isMac ? '⌘↵' : 'Ctrl+↵'} pour valider)" aria-label="Message de commit" aria-invalid="${S.commitError}">${esc(S.commitMsg)}</textarea>
      ${S.commitError ? '<span style="color:var(--bad);font-size:12px">Écris un message avant de valider.</span>' : ''}
      <button class="btn-primary" data-act="commit" ${S.scm.length ? '' : 'disabled'}>${ic('check', 14)}Valider et pousser</button>
    </div>
    <div class="group-head">${ic('chev-down', 13)}Modifications<span class="n">${S.scm.length}</span></div>
    <div class="sb-scroll" style="padding-top:0">
      ${S.scm.map((f) => {
        const name = f.file.split('/').pop();
        const dir = f.file.split('/').slice(0, -1).join('/');
        return `<button class="file-row ${S.scmSel === f.key ? 'is-active' : ''}" data-act="scm-sel" data-key="${f.key}" title="${esc(f.file)}">
          ${ic('file', 14)}<span class="file-name">${esc(name)}</span><span class="file-dir mono">${esc(dir)}</span><span class="git git-${f.st}">${f.st}</span></button>`;
      }).join('') || '<p class="pal-empty">Aucune modification.</p>'}
    </div>`;
}

function sidebarSync() {
  const left = CONFLICTS.filter((c) => !S.choices[c.id]).length;
  const opRow = (o, extra = '') => `<div class="op-row ${extra}"><span class="${mcls(o.method)}">${short(o.method)}</span><span class="op-path">${esc(o.path)}</span><span class="op-state ${o.cls ?? ''}">${esc(o.state)}</span></div>`;
  return `
    <div class="pane-head"><span class="pane-title">Synchro OpenAPI</span>
      <button class="icon-btn sm" data-act="toast" data-msg="Comparaison relancée : aucun nouveau changement dans la spec." title="Relancer la comparaison" aria-label="Relancer la comparaison">${ic('sync', 15)}</button></div>
    <div class="sync-src">
      <span class="mono">${ic('file', 13)}./spec/openapi.yml</span>
      <span class="muted">${S.synced ? 'v2.4.0 · base à jour' : 'v2.3.0 → v2.4.0 · base du 12 sept.'}</span>
    </div>
    <div class="sync-counts">
      <span><i class="dot ${left ? 'warn' : 'good'}"></i><b>${left}</b> conflit${left > 1 ? 's' : ''}</span>
      <span><b>5</b> fusions auto</span><span><b>3</b> nouvelles</span><span><b>1</b> dépréciée</span>
    </div>
    <div class="sb-scroll" style="padding-top:0">
      <div class="group-head">À arbitrer<span class="n">${CONFLICTS.length}</span></div>
      ${CONFLICTS.map((c) => {
        const ch = S.choices[c.id];
        return `<button class="op-row ${S.conflict === c.id && !S.synced ? 'is-active' : ''}" data-act="conflict" data-id="${c.id}">
          <span class="${mcls(c.method)}">${short(c.method)}</span><span class="op-path">${esc(c.path)}</span>
          <span class="op-state ${ch ? 'good' : 'warn'}">${ch ? `${ic('check', 12)}` : 'à arbitrer'}</span>
          <span class="op-field">${esc(c.field)}${ch ? ` · ${CHOICE_LABEL[ch].toLowerCase()}` : ''}</span></button>`;
      }).join('')}
      <div class="group-head" style="margin-top:8px">Fusion automatique<span class="n">${AUTO.length}</span></div>
      ${AUTO.map((o) => opRow(o)).join('')}
      <div class="group-head" style="margin-top:8px">Nouvelles<span class="n">${NEW_OPS.length}</span></div>
      ${NEW_OPS.map((o) => opRow(o)).join('')}
      <div class="group-head" style="margin-top:8px">Dépréciées<span class="n">1</span></div>
      ${opRow({ method: 'DELETE', path: '/transactions/{id}', state: 'conservée' }, 'is-deprecated')}
    </div>`;
}

function renderSidebar() {
  const fn = { collections: sidebarCollections, env: sidebarEnv, scm: sidebarScm, sync: sidebarSync }[S.view];
  $('#sidebar').innerHTML = fn();
  $('#sidebar').setAttribute('aria-label', { collections: 'Collections', env: 'Environnements', scm: 'Source Control', sync: 'Synchro OpenAPI' }[S.view]);
}

/* ================= éditeur : collections ================= */

function chip(name) {
  const kind = name.startsWith('$') ? 'dyn' : name.startsWith('process.env.') ? 'env' : '';
  const bad = !kind && !resolve(name)?.win;
  return `<span class="var ${kind} ${bad ? 'bad' : ''}" data-var="${esc(name)}" tabindex="0">{{${esc(name)}}}</span>`;
}
const withChips = (s) => esc(s).replace(/\{\{([^}]+)\}\}/g, (_, n) => chip(n));

function editorCollections() {
  if (!S.tabs.length) {
    return `<div class="empty">
      <span class="empty-ic">${ic('layers', 20)}</span>
      <h2>Aucune requête ouverte</h2>
      <p>Choisis une requête dans la collection, ou passe par le clavier.</p>
      <div class="keys">
        <span>Rechercher une requête</span>${kbd('mod+K')}
        <span>Toutes les commandes</span>${kbd('mod+shift+P')}
        <span>Nouvelle requête</span>${kbd('mod+N')}
        <span>Coller une commande cURL</span>${kbd('mod+V')}
      </div></div>`;
  }
  return `
    <div class="tabs" id="tabs" role="tablist" aria-label="Requêtes ouvertes"></div>
    <div class="crumbs" id="crumbs"></div>
    <div class="urlbar" id="urlbar"></div>
    <div class="split ${S.layout === 'v' ? 'vertical' : ''}" id="split" style="--req:${S.reqPct}%;--reqh:${S.reqhPct}%">
      <section class="pane" id="reqPane" aria-label="Requête"></section>
      <div class="split-handle" data-drag="split" role="separator" aria-label="Redimensionner requête et réponse"></div>
      <section class="pane" id="resPane" aria-label="Réponse"></section>
    </div>`;
}

function paintTabs() {
  $('#tabs').innerHTML = S.tabs.map((id) => {
    const r = REQ[id];
    return `<div class="tab ${S.preview === id ? 'preview' : ''} ${S.dirty.has(id) ? 'is-dirty' : ''}" role="tab" tabindex="0" aria-selected="${S.active === id}" data-act="tab" data-id="${id}" title="${esc(r.file)}">
      <span class="${mcls(r.method)}" style="width:auto">${short(r.method)}</span><span class="tab-name">${esc(r.name)}</span>
      <button class="tab-x" data-act="close-tab" data-id="${id}" aria-label="Fermer ${esc(r.name)}"><span class="dirty"></span>${ic('x', 13, 'x')}</button>
    </div>`;
  }).join('') + `<span class="tabs-end"><button class="icon-btn sm" data-act="palette" title="Ouvrir une requête" aria-label="Ouvrir une requête">${ic('plus', 15)}</button></span>`;
}

function paintCrumbs() {
  const r = REQ[S.active];
  const parts = ['api-paiements', ...r.file.split('/')];
  const dirty = S.dirty.has(S.active);
  $('#crumbs').innerHTML = `${ic('file', 13)}${parts.map((p, i) => (i === parts.length - 1 ? `<b>${esc(p)}</b>` : `<span>${esc(p)}</span>${ic('chev-right', 12)}`)).join('')}
    <span class="crumbs-state ${dirty ? 'dirty' : ''}">${dirty ? `Non enregistré ${kbd('mod+S')}` : `${ic('check', 12)}Enregistré sur le disque`}</span>`;
}

function urlHTML(r) {
  const q = r.query.filter((x) => x.on && x.k);
  const qs = q.length ? `<span class="url-q">?${q.map((x) => `<b>${esc(x.k)}</b>=<b>${withChips(x.v)}</b>`).join('&amp;')}</span>` : '';
  return withChips(r.url) + qs;
}

function paintUrl() {
  const r = REQ[S.active];
  const sending = S.sending?.id === S.active;
  $('#urlbar').innerHTML = `
    <div class="url">
      <button class="method-select ${mcls(r.method)}" style="width:auto" data-act="method-menu" aria-haspopup="menu" aria-label="Méthode ${r.method}">${r.method}${ic('chev-down', 12)}</button>
      <div class="url-text" id="urlText">${urlHTML(r)}</div>
    </div>
    <button class="btn-primary lg ${sending ? 'is-sending' : ''}" data-act="send" aria-label="${sending ? 'Annuler la requête' : 'Envoyer la requête'}">
      ${sending ? `<span class="spinner"></span>Annuler ${kbd('esc')}` : `${ic('send', 15)}Envoyer ${kbd('mod+enter')}`}
    </button>
    <button class="btn lg" data-act="code-menu" aria-haspopup="menu" title="Générer du code" aria-label="Générer du code">${ic('code', 16)}</button>`;
}

const REQ_TABS = [
  ['params', 'Paramètres', (r) => r.query.filter((q) => q.on).length + r.params.length],
  ['body', 'Corps', (r) => (r.body ? 'JSON' : '')],
  ['headers', 'En-têtes', (r) => r.headers.filter((h) => h.on).length + (r.auth === 'inherit' ? 1 : 0)],
  ['auth', 'Auth', (r) => (r.auth === 'inherit' ? 'hérité' : '')],
  ['scripts', 'Scripts', () => ''],
  ['tests', 'Tests', (r) => r.assertions.length + r.js.length],
  ['docs', 'Docs', () => ''],
];

function subtabs(list, current, act) {
  return list.map(([id, label, n]) => `<button class="subtab" role="tab" aria-selected="${current === id}" data-act="${act}" data-tab="${id}">${label}${n !== '' && n !== undefined ? `<span class="n">${n}</span>` : ''}</button>`).join('');
}

function paintReq() {
  const r = REQ[S.active];
  const tabs = REQ_TABS.map(([id, label, f]) => [id, label, f(r) || '']);
  const body = { params: paramsHTML, body: bodyHTML, headers: headersHTML, auth: authHTML, scripts: scriptsHTML, tests: testsHTML, docs: docsHTML }[S.reqTab](r);
  $('#reqPane').innerHTML = `<div class="subtabs" role="tablist" aria-label="Sections de la requête">${subtabs(tabs, S.reqTab, 'req-tab')}</div><div class="pane-body">${body}</div>`;
}

function deprecatedBanner(r) {
  return r.deprecated ? `<div class="banner">${ic('alert', 15)}<span><b>Requête dépréciée.</b> Retirée de la spec en v2.4.0, elle reste ici tant que tu ne la supprimes pas toi-même.</span></div>` : '';
}

function paramsHTML(r) {
  const on = r.query.filter((q) => q.on).length;
  return `${deprecatedBanner(r)}
    <section class="sec">
      <div class="sec-head"><span class="sec-title">Paramètres de requête</span><span class="sec-meta">${on} actif${on > 1 ? 's' : ''}</span>
        <button class="sec-act" data-act="toast" data-msg="Édition en masse : une ligne clé=valeur par paramètre.">Édition en masse</button></div>
      <div class="kv">
        <div class="kv-row head"><span class="kv-cell"></span><span class="kv-cell">Clé</span><span class="kv-cell">Valeur</span><span class="kv-cell desc">Description</span></div>
        ${r.query.map((q, i) => `<div class="kv-row ${q.on ? '' : 'off'}">
          <span class="kv-cell"><input type="checkbox" class="cb" data-q="${i}" ${q.on ? 'checked' : ''} aria-label="Activer ${esc(q.k)}"></span>
          <span class="kv-cell m-cell"><input type="text" value="${esc(q.k)}" data-qk="${i}" aria-label="Clé" spellcheck="false">${q.spec ? '<span class="tag spec" title="Déclaré dans la spec OpenAPI">spec</span>' : ''}</span>
          <span class="kv-cell m-cell"><input type="text" value="${esc(q.v)}" data-qv="${i}" aria-label="Valeur de ${esc(q.k)}" spellcheck="false"></span>
          <span class="kv-cell desc"><span class="clip-text">${esc(q.d ?? '')}</span></span></div>`).join('')}
        <div class="kv-row ghost"><span class="kv-cell">${ic('plus', 13)}</span>
          <span class="kv-cell m-cell"><input type="text" placeholder="Ajouter un paramètre puis ↵" data-qnew aria-label="Nouveau paramètre" spellcheck="false"></span>
          <span class="kv-cell"></span><span class="kv-cell desc"></span></div>
      </div>
    </section>
    ${r.params.length ? `<section class="sec">
      <div class="sec-head"><span class="sec-title">Paramètres de chemin</span><span class="sec-meta">déduits de l'URL</span></div>
      <div class="kv cols-3">
        <div class="kv-row head"><span class="kv-cell"></span><span class="kv-cell">Clé</span><span class="kv-cell">Valeur</span></div>
        ${r.params.map((p) => `<div class="kv-row"><span class="kv-cell">${ic('lock', 12, 'faint')}</span>
          <span class="kv-cell m-cell">${esc(p.k)}${p.spec ? '<span class="tag spec">spec</span>' : ''}</span>
          <span class="kv-cell m-cell">${withChips(p.v)}<span class="pdesc">${esc(p.d)}</span></span></div>`).join('')}
      </div></section>` : ''}
    ${r.auth === 'inherit' ? `<div class="note">${ic('shield', 14)}<span>Auth héritée de la collection : Bearer ${chip('token')}</span><a data-act="req-tab" data-tab="auth" style="margin-left:auto">Voir</a></div>` : ''}`;
}

function bodyHTML(r) {
  const types = ['Aucun', 'JSON', 'XML', 'Texte', 'Formulaire', 'Multipart', 'Binaire'];
  const cur = r.body ? r.bodyType ?? 'JSON' : 'Aucun';
  const seg = `<div class="seg" role="group" aria-label="Type de corps">${types.map((t) => `<button aria-pressed="${t === cur}" data-act="body-type" data-type="${t}">${t}</button>`).join('')}</div>`;
  if (!r.body) {
    return `${seg}<div class="empty" style="min-height:200px"><span class="empty-ic">${ic('file', 18)}</span>
      <h2>Pas de corps</h2><p>Une requête ${r.method} part généralement sans corps. Choisis un type au-dessus pour en ajouter un.</p></div>`;
  }
  return `${seg}<div style="height:10px"></div>${codeBlock(r.body.split('\n'), { hl: hlJson, cls: 'boxed' })}
    <p class="faint" style="font-size:12px;margin:8px 2px 0">Les variables <span class="mono">{{…}}</span> sont résolues à l'envoi. Survole-les pour voir leur valeur.</p>`;
}

function headersHTML(r) {
  return `<section class="sec"><div class="sec-head"><span class="sec-title">En-têtes</span><span class="sec-meta">${r.headers.filter((h) => h.on).length} actifs</span></div>
    <div class="kv cols-3">
      <div class="kv-row head"><span class="kv-cell"></span><span class="kv-cell">Nom</span><span class="kv-cell">Valeur</span></div>
      ${r.headers.map((h, i) => `<div class="kv-row ${h.on ? '' : 'off'}"><span class="kv-cell"><input type="checkbox" class="cb" data-h="${i}" ${h.on ? 'checked' : ''} aria-label="Activer ${esc(h.k)}"></span>
        <span class="kv-cell m-cell">${esc(h.k)}${h.spec ? '<span class="tag spec">spec</span>' : ''}</span><span class="kv-cell m-cell">${withChips(h.v)}</span></div>`).join('')}
      ${r.auth === 'inherit' ? `<div class="kv-row locked"><span class="kv-cell">${ic('lock', 12)}</span><span class="kv-cell m-cell">Authorization</span><span class="kv-cell m-cell">Bearer ${chip('token')}<span class="tag" style="margin-left:auto">hérité</span></span></div>` : ''}
    </div></section>
    <p class="faint" style="font-size:12px;margin:0 2px">Les en-têtes marqués <span class="tag spec">spec</span> viennent de la spec OpenAPI. Ceux que tu ajoutes ne sont jamais touchés par la synchro.</p>`;
}

function authHTML(r) {
  if (r.auth === 'none') {
    return `<section class="sec"><div class="sec-head"><span class="sec-title">Type</span></div><span class="select">Aucune authentification${ic('chev-down', 13)}</span></section>
      <div class="note">${ic('key', 14)}<span>Cette requête obtient le jeton. Son script post-réponse le range dans ${chip('token')} pour toute la collection.</span></div>`;
  }
  return `<section class="sec"><div class="sec-head"><span class="sec-title">Type</span></div><span class="select">Hériter du parent${ic('chev-down', 13)}</span></section>
    <section class="sec"><div class="sec-head"><span class="sec-title">Bearer Token</span><span class="sec-meta">défini dans opencollection.yml</span></div>
      <div class="kv cols-ro">
        <div class="kv-row"><span class="kv-cell">Jeton</span><span class="kv-cell m-cell">${chip('token')}</span></div>
        <div class="kv-row"><span class="kv-cell">Préfixe</span><span class="kv-cell m-cell">Bearer</span></div>
        <div class="kv-row"><span class="kv-cell">Stockage</span><span class="kv-cell" style="font-size:12.5px">Trousseau du système, jamais écrit dans un fichier</span></div>
      </div></section>
    <div class="note">${ic('arrow-right', 14)}<span>Le jeton est rafraîchi par le script post-réponse de <a data-act="open" data-id="connexion" data-pin="1">Connexion</a>.</span></div>`;
}

function scriptsHTML(r) {
  return `<section class="sec"><div class="sec-head"><span class="sec-title">Avant la requête</span><span class="sec-meta">après ceux de la collection et du dossier</span></div>
      ${codeBlock(r.pre.split('\n'), { hl: hlJs, cls: 'boxed' })}</section>
    <section class="sec"><div class="sec-head"><span class="sec-title">Après la réponse</span></div>
      ${codeBlock(r.post.split('\n'), { hl: hlJs, cls: 'boxed' })}</section>
    <div class="note">${ic('shield', 14)}<span>Sandbox QuickJS : pas d'accès disque ni réseau, hors <span class="mono">bru.sendRequest</span>. API compatible Bruno.</span></div>`;
}

function testsHTML(r) {
  return `<section class="sec"><div class="sec-head"><span class="sec-title">Assertions</span><span class="sec-meta">sans code</span></div>
      <div class="kv">
        <div class="kv-row head"><span class="kv-cell"></span><span class="kv-cell">Expression</span><span class="kv-cell">Opérateur</span><span class="kv-cell">Valeur attendue</span></div>
        ${r.assertions.map((a, i) => `<div class="kv-row ${a.on ? '' : 'off'}"><span class="kv-cell"><input type="checkbox" class="cb" data-a="${i}" ${a.on ? 'checked' : ''} aria-label="Activer l'assertion"></span>
          <span class="kv-cell m-cell">${esc(a.expr)}</span><span class="kv-cell m-cell">${esc(a.op)}</span><span class="kv-cell m-cell">${esc(a.val)}</span></div>`).join('')}
      </div></section>
    <section class="sec"><div class="sec-head"><span class="sec-title">Tests JavaScript</span><span class="sec-meta">style Chai</span></div>
      ${r.js.length ? codeBlock(r.js.map((t) => t.code).join('\n\n').split('\n'), { hl: hlJs, cls: 'boxed' }) : codeBlock(['// test("nom", () => { expect(res.status).to.equal(200); });'], { hl: hlJs, cls: 'boxed' })}</section>`;
}

function docsHTML(r) {
  const md = (s) => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>');
  return `<div class="md"><h3>${esc(r.name)}</h3>
    <p>${md(r.docs ?? `Requête ${r.method} sur ${r.url.replace('{{baseUrl}}', '')}.`)}</p>
    <ul><li>${r.status && r.status >= 400 ? `<code>${r.status}</code> : réponse attendue de démonstration` : '<code>200</code> : succès'}</li><li><code>401</code> : jeton absent ou expiré</li></ul>
    ${r.op ? `<p>Source : <code>openapi.yml</code> · operationId <code>${r.op}</code></p>` : '<p>Requête créée par l\'équipe, absente de la spec.</p>'}
    <p class="faint" style="font-size:12px">Docs en Markdown, rangées dans le fichier de la requête.</p></div>`;
}

function resolvedUrl(r) {
  const rep = (s) => s.replace(/\{\{([^}]+)\}\}/g, (all, n) => resolve(n)?.win?.val ?? all);
  const q = r.query.filter((x) => x.on && x.k);
  return rep(r.url) + (q.length ? `?${q.map((x) => `${x.k}=${rep(x.v)}`).join('&')}` : '');
}

function paintRes() {
  const id = S.active;
  const r = REQ[id];
  const res = S.responses[id];
  const sending = S.sending?.id === id;
  const pane = $('#resPane');
  const live = res && !res.cancelled;
  const tests = live ? testResults(r, res) : [];
  const passed = tests.filter((t) => t.ok).length;
  const tabs = [
    ['body', 'Corps', ''],
    ['headers', 'En-têtes', live ? 9 : ''],
    ['cookies', 'Cookies', live ? 1 : ''],
    ['timeline', 'Timeline', ''],
    ['tests', 'Tests', live ? `<span class="${passed === tests.length ? 'ok' : 'ko'}">${passed}/${tests.length}</span>` : ''],
  ];
  const meta = live ? `<span class="status s-${String(res.status)[0]}">${res.status}<span class="status-text"> ${STATUS_TEXT[res.status] ?? ''}</span></span>
    <button class="metric time" data-pop="timing" data-act="res-tab" data-tab="timeline" aria-label="Durée ${res.ms} ms, voir la timeline">${ic('clock', 12)}${res.ms} ms</button>
    <span class="metric size">${fmtSize(res.bytes)}</span>` : '';
  let content;
  if (!res && !sending) {
    content = `<div class="empty"><span class="empty-ic">${ic('send', 18)}</span><h2>Pas encore de réponse</h2>
      <p>Envoie la requête pour voir le corps, les en-têtes, la timeline réseau et les tests.</p>
      <button class="btn-primary" data-act="send" style="margin-top:6px">${ic('send', 14)}Envoyer ${kbd('mod+enter')}</button></div>`;
  } else if (!res && sending) {
    content = `<div class="empty"><span class="spinner"></span><h2>Envoi en cours…</h2><p class="mono" style="font-size:12px">${esc(resolvedUrl(r))}</p></div>`;
  } else if (res.cancelled) {
    content = `<div class="empty"><span class="empty-ic">${ic('x-circle', 18)}</span><h2>Requête annulée</h2>
      <p>Annulée après ${res.after} ms, avant la réponse du serveur. Rien n'a été enregistré.</p>
      <button class="btn" data-act="send" style="margin-top:6px">${ic('sync', 14)}Renvoyer</button></div>`;
  } else {
    content = resContent(r, res, tests);
  }
  pane.innerHTML = `<div class="res-bar"><div class="subtabs" role="tablist" aria-label="Sections de la réponse">${subtabs(tabs, S.resTab, 'res-tab')}</div><div class="res-meta">${meta}</div></div>
    ${live && S.resTab === 'body' ? bodyTools() : ''}
    <div class="res-body ${sending ? 'is-sending' : ''}" id="resBody">${sending ? '<div class="progress"></div>' : ''}${content}</div>`;
}

function bodyTools() {
  return `<div class="res-tools">
    <div class="seg" role="group" aria-label="Format"><button aria-pressed="${!S.raw}" data-act="raw" data-raw="0">JSON</button><button aria-pressed="${S.raw}" data-act="raw" data-raw="1">Brut</button></div>
    <label class="jp" id="jpWrap">${ic('filter', 13)}<input id="jpInput" type="text" value="${esc(S.jsonFilter)}" placeholder="$.client.nom" aria-label="Filtre JSONPath" spellcheck="false"></label>
    <span class="grow"></span>
    <button class="icon-btn sm" data-act="wrap" aria-pressed="${S.wrap}" title="Retour à la ligne" aria-label="Retour à la ligne">${ic('wrap', 15)}</button>
    <button class="icon-btn sm" data-act="copy-body" title="Copier le corps" aria-label="Copier le corps">${ic('copy', 15)}</button>
    <button class="icon-btn sm" data-act="toast" data-msg="Corps enregistré dans ~/Téléchargements/reponse.json" title="Enregistrer dans un fichier" aria-label="Enregistrer dans un fichier">${ic('download', 15)}</button>
  </div>`;
}

function bodyCode(res) {
  if (S.raw) return `<div class="code wrap"><div class="ln"><span class="ln-no">1</span><span class="ln-g"></span><span class="ln-t">${hlJson(JSON.stringify(res.data))}</span></div></div>`;
  const q = jsonPath(res.data, S.jsonFilter);
  if (!q.ok) return `<div class="empty" style="min-height:160px"><h2>${esc(q.error)}</h2><p>Exemples : <span class="mono">$.client</span>, <span class="mono">$.historique[0].statut</span></p></div>`;
  return jsonCode(q.value, `${S.active}${S.jsonFilter}`);
}

function timelineRows(res, compact = false) {
  const ph = res.phases;
  const rows = [['DNS', ph.dns, 'dns'], ['Connexion TCP', ph.tcp, 'tcp'], ['Négociation TLS', ph.tls, 'tls'], ['Attente (TTFB)', ph.wait, 'wait'], ['Téléchargement', ph.dl, 'dl']];
  let off = 0;
  return `<div class="timeline" ${compact ? 'style="grid-template-columns:104px minmax(0,1fr) 46px;gap:7px 10px;margin:0"' : ''}>${rows.map(([label, ms, k]) => {
    const left = (off / res.ms) * 100;
    off += ms;
    return `<span class="tl-label ${ms ? '' : 'faint'}">${label}</span><span class="tl-track">${ms ? `<span class="tl-bar ph-${k}" style="left:${left}%;width:${(ms / res.ms) * 100}%"></span>` : ''}</span><span class="tl-ms ${ms ? '' : 'faint'}">${ms ? `${ms} ms` : '—'}</span>`;
  }).join('')}${compact ? '' : `<span class="tl-label tl-total">Total</span><span class="tl-total"></span><span class="tl-ms tl-total">${res.ms} ms</span>`}</div>`;
}

function resContent(r, res, tests) {
  if (S.resTab === 'body') return `<div id="resCode">${bodyCode(res)}</div>`;
  if (S.resTab === 'headers') {
    const hs = [['content-type', 'application/json; charset=utf-8'], ['content-length', String(res.bytes)], ['date', 'Tue, 29 Sep 2026 10:42:18 GMT'], ['x-request-id', '7c1e0a52-4d3b-4f7e-9a61-2b8d4f0c9e13'], ['x-ratelimit-remaining', '118'], ['cache-control', 'no-store'], ['strict-transport-security', 'max-age=31536000'], ['vary', 'Accept-Encoding'], ['set-cookie', 'session=…; HttpOnly; Secure']];
    return `<div class="pane-body"><div class="kv cols-ro">${hs.map(([k, v]) => `<div class="kv-row"><span class="kv-cell m-cell" style="color:var(--muted)">${k}</span><span class="kv-cell m-cell">${esc(v)}</span></div>`).join('')}</div></div>`;
  }
  if (S.resTab === 'cookies') {
    const cols = 'grid-template-columns:minmax(0,.8fr) minmax(0,1.2fr) minmax(0,1fr) 60px minmax(0,1fr)';
    return `<div class="pane-body"><div class="kv">
      <div class="kv-row head" style="${cols}"><span class="kv-cell" style="justify-content:flex-start;padding:0 10px">Nom</span><span class="kv-cell">Valeur</span><span class="kv-cell">Domaine</span><span class="kv-cell">Chemin</span><span class="kv-cell">Attributs</span></div>
      <div class="kv-row" style="${cols}"><span class="kv-cell m-cell" style="justify-content:flex-start;padding:0 10px">session</span><span class="kv-cell m-cell">s%3A9f1c…e02</span><span class="kv-cell m-cell">api.dev.local</span><span class="kv-cell m-cell">/</span><span class="kv-cell"><span class="tag">HttpOnly</span><span class="tag">Secure</span></span></div>
    </div><p class="faint" style="font-size:12px;margin:10px 2px">Les cookies restent dans le gestionnaire local, par domaine.</p></div>`;
  }
  if (S.resTab === 'timeline') {
    return `<div class="pane-body">
      <div class="sec-head"><span class="sec-title">Timeline réseau</span><span class="sec-meta">${res.reused ? 'connexion réutilisée, pas de DNS ni de TLS' : 'nouvelle connexion'}</span></div>
      <div style="height:6px"></div>${timelineRows(res)}
      <dl class="dl">
        <dt>URL résolue</dt><dd>${esc(resolvedUrl(r))}</dd>
        <dt>Protocole</dt><dd>HTTP/2</dd>
        <dt>Adresse distante</dt><dd>10.0.4.12:443</dd>
        <dt>TLS</dt><dd>TLS 1.3 · TLS_AES_128_GCM_SHA256</dd>
        <dt>Certificat</dt><dd>*.dev.local · valide jusqu'au 14 mars 2027</dd>
        <dt>Reçu</dt><dd>${fmtSize(res.bytes)} (corps) · 412 o (en-têtes)</dd>
      </dl></div>`;
  }
  const passed = tests.filter((t) => t.ok).length;
  const allOk = passed === tests.length;
  return `<div class="pane-body">
    <div class="tests-sum"><span class="badge-ic ${allOk ? '' : 'ko'}">${ic(allOk ? 'check' : 'x', 15)}</span>
      <span style="font-weight:600">${passed} test${passed > 1 ? 's' : ''} sur ${tests.length} ${passed > 1 ? 'passent' : 'passe'}</span>
      <span class="muted">${allOk ? 'Tout est vert, beau travail.' : 'Regarde la valeur reçue à droite de chaque échec.'}</span></div>
    ${tests.map((t) => `<div class="test-row">${ic(t.ok ? 'check-circle' : 'x-circle', 15, t.ok ? 'ok' : 'ko')}<span>${esc(t.name)}</span>
      <span class="src">${t.ok ? esc(t.src) : `reçu ${esc(String(t.actual))}`}</span></div>`).join('')}</div>`;
}

/* ================= éditeur : environnements ================= */

function editorEnv() {
  const sel = S.envSel;
  const isGlobal = sel === 'globales';
  const names = isGlobal ? Object.keys(VARS).filter((n) => VARS[n].levels.Globale) : Object.keys(ENVS[sel]);
  if (!names.includes(S.envVar)) S.envVar = names[0];
  const file = isGlobal ? 'variables globales' : `${sel}.yml`;
  const valOf = (n) => (isGlobal ? VARS[n].levels.Globale : ENVS[sel][n]);
  const res = resolve(S.envVar, isGlobal ? S.env : sel);
  return `
    <div class="tabs"><div class="tab" role="tab" aria-selected="true">${ic(isGlobal ? 'globe' : 'file', 14)}<span class="tab-name">${esc(file)}</span></div></div>
    <div class="crumbs">${ic('file', 13)}<span>api-paiements</span>${ic('chev-right', 12)}${isGlobal ? '<b>variables globales</b>' : `<span>environments</span>${ic('chev-right', 12)}<b>${esc(file)}</b>`}</div>
    <div class="view-head">
      <h1 class="view-title">${isGlobal ? 'Variables globales' : `Environnement ${esc(sel)}`}</h1>
      ${!isGlobal && S.env === sel ? '<span class="tag good">actif</span>' : ''}${sel === 'prod' ? '<span class="tag bad">production</span>' : ''}
      <span class="view-sub">${names.length} variable${names.length > 1 ? 's' : ''}</span>
      <span class="grow"></span>
      ${isGlobal ? '' : `<button class="btn" data-act="env-activate" ${S.env === sel ? 'disabled' : ''}>${S.env === sel ? `${ic('check', 14)}Environnement actif` : 'Utiliser cet environnement'}</button>`}
    </div>
    <div class="env-layout">
      <div class="env-main">
        ${sel === 'prod' ? `<div class="banner">${ic('alert', 15)}<span><b>Production.</b> Les requêtes envoyées ici touchent des données réelles. La barre de titre passe en rouge quand cet environnement est actif.</span></div>` : ''}
        <div class="vt" role="listbox" aria-label="Variables">
          <div class="vt-row head"><span class="vt-cell">Nom</span><span class="vt-cell">Valeur</span><span class="vt-cell">Type</span><span class="vt-cell center">Secret</span></div>
          ${names.map((n) => {
            const v = valOf(n);
            const secret = VARS[n].type === 'secret';
            return `<button class="vt-row" role="option" aria-selected="${S.envVar === n}" data-act="env-var" data-var="${n}">
              <span class="vt-cell mono">${esc(n)}</span>
              <span class="vt-cell mono">${secret ? `<span class="secret">${ic('key', 13)}trousseau du système</span>` : v ? `<span class="clip">${esc(v)}</span>` : '<span class="faint">vide</span>'}</span>
              <span class="vt-cell vt-type">${secret ? 'secret' : VARS[n].type}</span>
              <span class="vt-cell center">${secret ? `<span style="color:var(--accent)">${ic('lock', 14)}</span>` : ''}</span></button>`;
          }).join('')}
        </div>
        <p class="faint" style="font-size:12px;margin:10px 2px">Les secrets sont rangés dans le trousseau du système : le fichier ne contient que <span class="mono">secret: true</span>. Ce fichier se versionne sans risque.</p>
      </div>
      <aside class="resolve" aria-label="Résolution de la variable">
        <h2>Résolution de ${chip(S.envVar)}</h2>
        <p>${res?.win ? `Valeur utilisée : niveau ${esc(res.win.label)}.` : 'Aucune portée ne la définit : elle part telle quelle.'}</p>
        ${res ? ladderHTML(res) : ''}
        <div class="resolve-foot">Du plus prioritaire au moins prioritaire, dans le même ordre que Bruno. Les valeurs du <span class="mono">.env</span> local se lisent avec <code>{{process.env.NOM}}</code>.</div>
      </aside>
    </div>`;
}

function ladderHTML(res) {
  return `<div class="ladder">${res.rungs.map((r) => `<div class="rung ${r.state}">
    <span class="rung-dot"></span>
    <span class="rung-name"><span class="rung-label">${esc(r.label)}</span>${r.state === 'is-win' ? '<span class="tag new">utilisée</span>' : ''}<span class="rung-src">${esc(r.src)}</span></span>
    ${r.val !== undefined ? `<span class="rung-val">${esc(r.val)}</span>` : ''}</div>`).join('')}</div>`;
}

/* ================= éditeur : source control ================= */

function editorScm() {
  const f = S.scm.find((x) => x.key === S.scmSel) ?? S.scm[0];
  if (!f) {
    return `<div class="empty"><span class="empty-ic">${ic('check', 18)}</span><h2>Rien à valider</h2>
      <p>L'arbre de travail est propre. Tes prochaines modifications de requêtes apparaîtront ici, fichier par fichier.</p></div>`;
  }
  S.scmSel = f.key;
  const name = f.file.split('/').pop();
  const rows = diffLines(f.head() ? f.head().split('\n') : [], f.work().split('\n'));
  let ln = 0;
  let rn = 0;
  const left = [];
  const right = [];
  rows.forEach((row) => {
    if (row.k === 'ctx') {
      left.push({ no: ++ln, t: row.l });
      right.push({ no: ++rn, t: row.r });
      return;
    }
    const both = row.l !== null && row.r !== null;
    left.push(row.l === null ? { no: '', t: '', cls: 'fill' } : { no: ++ln, cls: 'del', g: '−', html: both ? markWords(row.l, row.r, 'w-del') : hlYaml(row.l) });
    right.push(row.r === null ? { no: '', t: '', cls: 'fill' } : { no: ++rn, cls: 'add', g: '+', html: both ? markWords(row.r, row.l, 'w-add') : hlYaml(row.r) });
  });
  const changed = rows.filter((r) => r.k === 'chg').length;
  return `
    <div class="tabs"><div class="tab preview" role="tab" aria-selected="true">${ic('file-diff', 14)}<span class="tab-name">${esc(name)}</span><span class="tag">diff</span></div></div>
    <div class="crumbs">${ic('file', 13)}${['api-paiements', ...f.file.split('/')].map((p, i, a) => (i === a.length - 1 ? `<b>${esc(p)}</b>` : `<span>${esc(p)}</span>${ic('chev-right', 12)}`)).join('')}</div>
    <div class="view-head">
      <h1 class="view-title mono" style="font-size:13.5px;font-weight:500">${esc(f.file)}</h1>
      <span class="tag ${f.st === 'A' ? 'good' : ''}">${f.st === 'A' ? 'ajouté' : 'modifié'}</span>
      <span class="view-sub">${changed} ligne${changed > 1 ? 's' : ''} touchée${changed > 1 ? 's' : ''} · aucun champ volatil</span>
      <span class="grow"></span>
      ${f.req ? `<button class="btn" data-act="open" data-id="${f.req}" data-pin="1">${ic('arrow-right', 14)}Ouvrir la requête</button>` : ''}
    </div>
    <div class="diff"><div class="diff-grid">
      <div class="diff-col"><div class="diff-col-head"><b>HEAD</b><span class="mono">main</span></div>${codeBlock(left, { hl: hlYaml })}</div>
      <div class="diff-col"><div class="diff-col-head"><b>Copie de travail</b><span>sur le disque</span></div>${codeBlock(right, { hl: hlYaml })}</div>
    </div></div>`;
}

/* ================= éditeur : synchro OpenAPI ================= */

function hiddenRow(c) {
  return `<div class="ln fold-row"><span class="ln-no"></span><span class="ln-g">${ic('chev-right', 12)}</span><span class="ln-t">lignes 1 à ${c.start - 1} masquées, identiques des deux côtés</span></div>`;
}

function conflictLines(c, side) {
  const cls = { ours: 'h-o', theirs: 'h-t', base: 'h-b' }[side];
  return [...c.before.map((t) => ({ t })), ...c[side].map((l) => ({ t: l.t, cls: l.c || side === 'base' ? cls : '' })), ...c.after.map((t) => ({ t }))];
}

function resultLines(c) {
  const ch = S.choices[c.id];
  let hunk;
  if (!ch) hunk = c.ours.map((l) => ({ t: l.t, cls: 'r-u' }));
  else if (ch === 'ours') hunk = c.ours.map((l) => ({ t: l.t, cls: 'r-o' }));
  else if (ch === 'theirs') hunk = c.theirs.map((l) => ({ t: l.t, cls: 'r-t' }));
  else if (ch === 'both') hunk = c.both.map((l) => ({ t: l.t, cls: l.o === 't' ? 'r-t' : 'r-o' }));
  else hunk = (S.edits[c.id] ?? c.ours.map((l) => l.t)).map((t, i) => ({ t, cls: 'r-e', edit: i }));
  return hunk;
}

function resultCode(c) {
  const hunk = resultLines(c);
  const ch = S.choices[c.id];
  let no = c.start;
  const line = (o) => {
    const text = o.edit !== undefined
      ? `<span class="ln-t" contenteditable="true" spellcheck="false" data-edit="${c.id}:${o.edit}">${esc(o.t)}</span>`
      : `<span class="ln-t">${hlYaml(o.t)}</span>`;
    return `<div class="ln ${o.cls ?? ''}"><span class="ln-no">${no++}</span><span class="ln-g"></span>${text}</div>`;
  };
  const lens = `<div class="lens" role="group" aria-label="Arbitrer ce conflit">${['ours', 'theirs', 'both', 'edit'].map((k, i) => `${i ? '<i>·</i>' : ''}<button data-act="choose" data-id="${c.id}" data-choice="${k}" aria-pressed="${ch === k}">${CHOICE_LABEL[k]}</button>`).join('')}</div>`;
  return `<div class="code">${hiddenRow(c)}${c.before.map((t) => line({ t })).join('')}${lens}${hunk.map(line).join('')}${c.after.map((t) => line({ t })).join('')}</div>`;
}

function editorSync() {
  if (S.synced) {
    return `<div class="tabs"><div class="tab" role="tab" aria-selected="true">${ic('merge', 14)}<span class="tab-name">Fusion · openapi.yml</span></div></div>
      <div class="empty"><span class="empty-ic" style="background:var(--good-soft);color:var(--good)">${ic('check', 20)}</span>
        <h2>Spec synchronisée</h2>
        <p>11 changements écrits, base <span class="mono">.oc-sync</span> mise à jour en dernier. Rien de ce que l'équipe avait saisi n'a été écrasé.</p>
        <div style="display:flex;gap:8px;margin-top:8px"><button class="btn" data-act="go" data-view="scm">${ic('branch', 14)}Voir dans Source Control</button>
        <button class="btn" data-act="go" data-view="collections">${ic('layers', 14)}Retour aux requêtes</button></div></div>`;
  }
  const idx = CONFLICTS.findIndex((x) => x.id === S.conflict);
  const c = CONFLICTS[idx];
  const left = CONFLICTS.filter((x) => !S.choices[x.id]).length;
  const resolved = CONFLICTS.length - left;
  const ch = S.choices[c.id];
  const pane = (side, sw, title, sub, action) => `<div class="mpane">
      <div class="mpane-head"><span class="sw ${sw}"></span><b>${title}</b><span class="sub">${sub}</span><span class="grow"></span>
        ${action ? `<button class="btn ghost" style="height:24px" data-act="choose" data-id="${c.id}" data-choice="${side}" aria-pressed="${ch === side}">${ch === side ? ic('check', 13) : ''}${action}</button>` : ''}</div>
      ${codeBlock(conflictLines(c, side), { hl: hlYaml, start: c.start }).replace('<div class="code ">', `<div class="code ">${hiddenRow(c)}`)}</div>`;
  return `
    <div class="tabs"><div class="tab" role="tab" aria-selected="true">${ic('merge', 14)}<span class="tab-name">Fusion · openapi.yml</span></div></div>
    <div class="view-head">
      <span class="${mcls(c.method)}" style="width:auto;font-size:11px">${c.method}</span>
      <h1 class="view-title mono" style="font-size:13.5px;font-weight:500">${esc(c.path)}</h1>
      <span class="view-sub">${esc(c.field)} · ${esc(c.why)}</span>
      <span class="grow"></span>
      <span class="navgroup">
        <button class="icon-btn sm" data-act="conflict-nav" data-dir="-1" aria-label="Conflit précédent" title="Conflit précédent (${isMac ? '⌥↑' : 'Alt+↑'})">${ic('chev-up', 15)}</button>
        <span>Conflit ${idx + 1} sur ${CONFLICTS.length}</span>
        <button class="icon-btn sm" data-act="conflict-nav" data-dir="1" aria-label="Conflit suivant" title="Conflit suivant (${isMac ? '⌥↓' : 'Alt+↓'})">${ic('chev-down', 15)}</button>
      </span>
      <button class="btn ${S.showBase ? '' : 'ghost'}" data-act="toggle-base" aria-pressed="${S.showBase}">${ic('eye', 14)}Base</button>
    </div>
    <div class="merge">
      <div class="merge-top ${S.showBase ? 'with-base' : ''}">
        ${pane('ours', 'o', 'Équipe', 'collection', "Garder l'équipe")}
        ${S.showBase ? pane('base', 'b', 'Base', 'import du 12 sept.', '') : ''}
        ${pane('theirs', 't', 'Spec', 'openapi.yml v2.4.0', 'Prendre la spec')}
      </div>
      <div class="mpane result">
        <div class="mpane-head"><span class="sw r"></span><b>Résultat</b><span class="sub">${esc(c.file)}</span><span class="grow"></span>
          ${ch ? `<span class="state-chip good">${ic('check', 12)}${CHOICE_LABEL[ch]}</span>` : `<span class="state-chip warn">${ic('alert', 12)}Non arbitré : la version de l'équipe reste en place</span>`}</div>
        ${resultCode(c)}
      </div>
    </div>
    <div class="island-foot">
      <span class="foot-state ${left ? 'warn' : 'good'}"><span class="badge-ic">${ic(left ? 'alert' : 'check', 14)}</span>${left ? `${left} conflit${left > 1 ? 's' : ''} à arbitrer avant d'appliquer` : 'Tout est arbitré, prêt à appliquer'}</span>
      <span class="foot-note">Rien n'est écrit sur le disque avant ta validation. La base .oc-sync est réécrite en dernier.</span>
      <span class="grow"></span>
      <button class="btn lg ghost" data-act="sync-cancel">Annuler</button>
      <button class="btn-primary lg" data-act="sync-apply" ${left ? 'disabled' : ''}>Appliquer ${9 + resolved} changements</button>
    </div>`;
}

/* ================= rendu global ================= */

function renderEditor() {
  const ed = $('#editor');
  if (S.view === 'collections') {
    ed.innerHTML = editorCollections();
    if (S.tabs.length) { paintTabs(); paintCrumbs(); paintUrl(); paintReq(); paintRes(); }
  } else {
    ed.innerHTML = { env: editorEnv, scm: editorScm, sync: editorSync }[S.view]();
  }
}

function syncHash() {
  const h = `#${S.view}`;
  if (location.hash !== h) history.replaceState(null, '', `${location.pathname}${location.search}${h}`);
}

function render() {
  syncHash();
  $('#wb').classList.toggle('no-sidebar', !S.sidebar);
  renderTitlebar();
  renderActivity();
  renderSidebar();
  renderEditor();
  renderStatus();
}

function paintTree() {
  const t = $('#tree');
  if (t) t.innerHTML = treeHTML(TREE, 0) || '<p class="pal-empty">Aucune requête ne correspond.</p>';
}

function refreshRequest() {
  paintTabs();
  paintCrumbs();
  $('#urlText').innerHTML = urlHTML(REQ[S.active]);
  const counts = REQ_TABS.map(([id, label, f]) => [id, label, f(REQ[S.active]) || '']);
  $('#reqPane .subtabs').innerHTML = subtabs(counts, S.reqTab, 'req-tab');
}

/* ================= actions ================= */

function markDirty(id = S.active) {
  S.dirty.add(id);
  if (S.preview === id) S.preview = null;
}

function revealInTree(id) {
  const walk = (nodes, path) => nodes.some((n) => (typeof n === 'string' ? n === id && (path.forEach((p) => { S.open[p] = true; }), true) : walk(n.children, [...path, n.id])));
  walk(TREE, []);
}

function openReq(id, { pin = false } = {}) {
  if (!S.tabs.includes(id)) {
    const pi = S.tabs.indexOf(S.preview);
    if (!pin && pi >= 0 && !S.dirty.has(S.preview)) S.tabs.splice(pi, 1, id);
    else S.tabs.splice(S.tabs.indexOf(S.active) + 1 || S.tabs.length, 0, id);
    if (!pin) S.preview = id;
  } else if (pin && S.preview === id) S.preview = null;
  S.active = id;
  S.jsonFilter = '';
  S.view = 'collections';
  revealInTree(id);
  render();
}

function closeTab(id) {
  const i = S.tabs.indexOf(id);
  S.tabs.splice(i, 1);
  S.dirty.delete(id);
  if (S.preview === id) S.preview = null;
  if (S.active === id) S.active = S.tabs[Math.min(i, S.tabs.length - 1)] ?? null;
  render();
}

function send() {
  if (S.view !== 'collections' || !S.active) return;
  if (S.sending) { cancelSend(); return; }
  const id = S.active;
  S.sending = { id, t0: performance.now() };
  S.sendTimer = setTimeout(() => {
    S.responses[id] = buildResponse(id, { cold: !S.warm });
    S.warm = true;
    S.sending = null;
    S.history.unshift({ id, t: hhmm(), code: S.responses[id].status });
    if (S.view === 'collections') {
      renderSidebar();
      if (S.active === id) { paintUrl(); paintRes(); $('#resBody')?.classList.add('flash'); }
    }
    renderStatus();
  }, 650 + Math.random() * 450);
  paintUrl();
  paintRes();
}

function cancelSend() {
  clearTimeout(S.sendTimer);
  const { id, t0 } = S.sending;
  S.sending = null;
  S.responses[id] = { cancelled: true, after: Math.round(performance.now() - t0) };
  if (S.view === 'collections' && S.active === id) { paintUrl(); paintRes(); }
  renderStatus();
}

function save() {
  if (S.view !== 'collections' || !S.active) return;
  const id = S.active;
  const r = REQ[id];
  if (!S.dirty.has(id)) { toast(`Rien à enregistrer dans ${r.file}`); return; }
  S.dirty.delete(id);
  if (!S.scm.find((f) => f.req === id)) S.scm.push({ key: id, req: id, file: r.file, st: 'M', head: () => HEADS[id], work: () => yamlOf(REQ[id], 3) });
  render();
  toast(`Enregistré dans ${r.file}`);
}

function setTheme(t) {
  document.documentElement.dataset.theme = t;
  store.set('maquette-v2-theme', t);
  renderTitlebar();
}

function curlOf(r) {
  const h = r.headers.filter((x) => x.on).map((x) => `  -H '${x.k}: ${x.v.replace(/\{\{\$uuid\}\}/, crypto.randomUUID?.() ?? 'uuid')}'`);
  if (r.auth === 'inherit') h.push("  -H 'Authorization: Bearer $TOKEN'");
  const parts = [`curl -X ${r.method} '${resolvedUrl(r)}'`, ...h];
  if (r.body) parts.push(`  --data '${r.body.replace(/\s*\n\s*/g, ' ')}'`);
  return parts.join(' \\\n');
}

function copy(text, msg) {
  navigator.clipboard?.writeText(text).then(() => toast(msg), () => toast(msg));
}

function applySync() {
  const c1 = S.choices.c1;
  const c2 = S.choices.c2;
  const headDetail = yamlOf(REQ.detail, 3);
  const pick = (c) => resultLines(c).map((l) => l.t.replace(/^ {6}/, ''));
  REQ.annuler.body = ['{', ...pick(CONFLICTS[0]), '}'].join('\n');
  if (c2 !== 'ours') {
    const lines = pick(CONFLICTS[1]).map((l) => l.trim());
    const hs = [];
    for (let i = 0; i < lines.length; i += 2) hs.push({ k: lines[i].replace('- name: ', ''), v: (lines[i + 1] ?? '').replace('value: ', '').replace(/^"(.*)"$/, '$1'), on: true, spec: true });
    REQ.detail.headers = [REQ.detail.headers[0], ...hs, REQ.detail.headers.find((h) => h.k === 'X-Request-Id')];
  }
  REQ.detail.conflict = false;
  REQ.annuler.conflict = false;
  TREE[0].children.splice(2, 0, { id: 'f-remb', type: 'folder', name: 'Remboursements', open: true, isNew: true, children: ['remb-creer', 'remb-detail'] });
  TREE[0].children.find((n) => n.id === 'f-cl').children.push('cl-plafonds');
  S.open['f-remb'] = true;
  if (yamlOf(REQ.detail, 3) !== headDetail && !S.scm.find((f) => f.req === 'detail')) S.scm.push({ key: 'detail', req: 'detail', file: REQ.detail.file, st: 'M', head: () => headDetail, work: () => yamlOf(REQ.detail, 3) });
  ['remb-creer', 'remb-detail', 'cl-plafonds'].forEach((id, i) => S.scm.push({ key: id, req: id, file: REQ[id].file, st: 'A', head: () => '', work: () => yamlOf(REQ[id], i + 1) }));
  S.scm.push({ key: 'source', file: '.oc-sync/openapi/source.yml', st: 'M', head: () => SOURCE_HEAD, work: () => SOURCE_WORK });
  S.synced = true;
  render();
  toast(`11 changements appliqués · ${c1 ? CHOICE_LABEL[c1].toLowerCase() : ''} et ${c2 ? CHOICE_LABEL[c2].toLowerCase() : ''} · base mise à jour`);
}

function go(view) {
  if (S.view === view && S.sidebar) S.sidebar = false;
  else { S.view = view; S.sidebar = true; }
  render();
}

const ACTIONS = {
  go: (el) => { closeMenu(); closePalette(); go(el.dataset.view); },
  palette: (el) => openPalette(el.dataset.prefix ?? ''),
  theme: () => setTheme(document.documentElement.dataset.theme === 'light' ? 'dark' : 'light'),
  layout: () => toggleLayout(),
  toggle: (el) => {
    const n = findNode(TREE, el.dataset.id);
    S.open[n.id] = !(S.open[n.id] ?? n.open);
    paintTree();
  },
  'collapse-all': () => { const walk = (ns) => ns.forEach((n) => { if (typeof n !== 'string') { S.open[n.id] = n.type === 'collection'; walk(n.children); } }); walk(TREE); paintTree(); },
  open: (el) => openReq(el.dataset.id, { pin: Boolean(el.dataset.pin) }),
  tab: (el, e) => { if (e.target.closest('.tab-x')) return; S.active = el.dataset.id; S.jsonFilter = ''; render(); },
  'close-tab': (el) => closeTab(el.dataset.id),
  hist: () => { S.histOpen = !S.histOpen; renderSidebar(); },
  'req-tab': (el) => { S.reqTab = el.dataset.tab; paintReq(); },
  'res-tab': (el) => { if (S.view !== 'collections') { S.view = 'collections'; render(); } S.resTab = el.dataset.tab; hidePop(); paintRes(); },
  send: () => send(),
  raw: (el) => { S.raw = el.dataset.raw === '1'; paintRes(); },
  wrap: () => { S.wrap = !S.wrap; paintRes(); },
  fold: (el) => { const f = el.dataset.fold; S.folds.has(f) ? S.folds.delete(f) : S.folds.add(f); $('#resCode').innerHTML = bodyCode(S.responses[S.active]); },
  'copy-body': () => copy(JSON.stringify(S.responses[S.active].data, null, 2), 'Corps de la réponse copié'),
  'body-type': (el) => {
    const r = REQ[S.active];
    if (el.dataset.type === 'Aucun') r.body = null;
    else { r.body ??= '{\n  \n}'; r.bodyType = el.dataset.type; }
    markDirty();
    refreshRequest();
    paintReq();
  },
  'method-menu': (el) => openMenu(el, ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'].map((m) => ({ html: `<span class="${mcls(m)}" style="width:auto">${m}</span>`, checked: REQ[S.active].method === m, run: () => { REQ[S.active].method = m; markDirty(); render(); } }))),
  'code-menu': (el) => openMenu(el, [
    { label: 'cURL', sub: 'copie', run: () => copy(curlOf(REQ[S.active]), 'Commande cURL copiée · le jeton reste $TOKEN') },
    { label: 'JavaScript · fetch', sub: 'copie', run: () => copy(`await fetch('${resolvedUrl(REQ[S.active])}', { method: '${REQ[S.active].method}' });`, 'Code fetch copié') },
    ...['Python', 'Go', 'Java', 'Kotlin', 'Dart', 'PHP', 'C#'].map((l) => ({ label: l, run: () => toast(`Génération ${l} : non maquettée`) })),
  ], 'Générer du code'),
  'env-menu': (el) => openEnvMenu(el),
  'workspace-menu': (el) => openMenu(el, [
    { label: 'API Paiements', sub: '~/code/api-paiements', checked: true, run: () => {} },
    { label: 'Back-office KYC', sub: '~/code/backoffice-kyc', run: () => toast('Back-office KYC est déjà ouverte dans la même fenêtre.') },
    'sep',
    { label: 'Ouvrir un dossier…', sub: isMac ? '⌘O' : 'Ctrl+O', run: () => toast('Ouvrir un dossier : sélecteur natif du système.') },
    { label: 'Cloner un dépôt Git…', run: () => toast('Cloner un dépôt : écran non maquetté.') },
  ], 'Collections sur ce poste'),
  'env-sel': (el) => { S.envSel = el.dataset.env; render(); },
  'env-var': (el) => { S.envVar = el.dataset.var; renderEditor(); },
  'env-activate': () => { S.env = S.envSel; render(); toast(`Environnement actif : ${S.env}`); },
  'edit-var': (el) => { hidePop(); S.envVar = el.dataset.var; S.envSel = S.env ?? 'dev'; S.view = 'env'; S.sidebar = true; render(); },
  'scm-sel': (el) => { S.scmSel = el.dataset.key; renderSidebar(); renderEditor(); },
  commit: () => {
    if (!S.commitMsg.trim()) { S.commitError = true; renderSidebar(); $('#commitMsg').focus(); return; }
    const n = S.scm.length;
    S.scm = [];
    S.commitMsg = '';
    S.commitError = false;
    S.ahead = 0;
    render();
    toast(`${n} fichier${n > 1 ? 's' : ''} validé${n > 1 ? 's' : ''} et poussé${n > 1 ? 's' : ''} sur origin/main`);
  },
  conflict: (el) => { S.conflict = el.dataset.id; renderSidebar(); renderEditor(); },
  'conflict-nav': (el) => navConflict(Number(el.dataset.dir)),
  'toggle-base': () => { S.showBase = !S.showBase; renderEditor(); },
  choose: (el) => {
    const { id, choice } = el.dataset;
    S.choices[id] = S.choices[id] === choice ? undefined : choice;
    if (!S.choices[id]) delete S.choices[id];
    render();
    if (choice === 'edit' && S.choices[id]) $('[data-edit]')?.focus();
  },
  'sync-apply': () => applySync(),
  'sync-cancel': () => { S.choices = {}; S.view = 'collections'; render(); toast("Synchro abandonnée : rien n'a été écrit sur le disque."); },
  lang: () => { S.lang = S.lang === 'FR' ? 'EN' : 'FR'; renderStatus(); toast(S.lang === 'EN' ? 'Langue : English (traduction non maquettée)' : 'Langue : français'); },
  toast: (el) => toast(el.dataset.msg),
};

function findNode(nodes, id) {
  for (const n of nodes) {
    if (typeof n === 'string') continue;
    if (n.id === id) return n;
    const f = findNode(n.children, id);
    if (f) return f;
  }
  return null;
}

function navConflict(dir) {
  const i = CONFLICTS.findIndex((c) => c.id === S.conflict);
  S.conflict = CONFLICTS[(i + dir + CONFLICTS.length) % CONFLICTS.length].id;
  renderSidebar();
  renderEditor();
}

function toggleLayout() {
  S.layout = S.layout === 'h' ? 'v' : 'h';
  renderTitlebar();
  $('#split')?.classList.toggle('vertical', S.layout === 'v');
}

function toggleSidebar() {
  S.sidebar = !S.sidebar;
  $('#wb').classList.toggle('no-sidebar', !S.sidebar);
}

/* ================= menus, popovers, toast ================= */

let menuAnchor = null;

function openMenu(anchor, items, label = '') {
  const menu = $('#menu');
  if (menuAnchor === anchor && !menu.hidden) { closeMenu(); return; }
  menuAnchor = anchor;
  menu._items = items;
  menu.innerHTML = (label ? `<div class="menu-label">${esc(label)}</div>` : '') + items.map((it, i) => {
    if (it === 'sep') return '<div class="menu-sep"></div>';
    if (it.raw) return it.raw;
    return `<button class="menu-item" role="menuitemradio" aria-checked="${Boolean(it.checked)}" data-mi="${i}">
      <span class="chk">${it.checked ? ic('check', 14) : ''}</span>${it.html ?? esc(it.label)}${it.sub ? `<span class="sub">${esc(it.sub)}</span>` : ''}</button>`;
  }).join('');
  menu.hidden = false;
  const r = anchor.getBoundingClientRect();
  const w = menu.offsetWidth;
  const h = menu.offsetHeight;
  const top = r.bottom + h + 6 > innerHeight ? r.top - h - 6 : r.bottom + 6;
  menu.style.top = `${top}px`;
  menu.style.left = `${clamp(r.right - w > 8 && r.left + w > innerWidth - 8 ? r.right - w : r.left, 8, innerWidth - w - 8)}px`;
  menu.querySelector('.menu-item')?.focus();
}

function closeMenu() {
  $('#menu').hidden = true;
  menuAnchor = null;
}

function openEnvMenu(anchor) {
  const peekEnv = S.env ?? 'dev';
  const peek = ['baseUrl', 'txId', 'canal', 'token'].map((n) => {
    const r = resolve(n);
    return `<dt>${n}</dt><dd>${esc(r?.win?.val ?? 'non définie')}</dd>`;
  }).join('');
  openMenu(anchor, [
    { label: 'Aucun environnement', checked: !S.env, run: () => setEnv(null) },
    ...Object.keys(ENVS).map((e) => ({ html: `<span class="env-dot ${e}"></span><span>${e}</span>`, sub: e === 'prod' ? 'production' : `${e}.yml`, checked: S.env === e, run: () => setEnv(e) })),
    'sep',
    { raw: `<div class="menu-label">Valeurs résolues${S.env ? '' : ` (${peekEnv})`}</div><dl class="menu-peek">${peek}</dl>` },
    'sep',
    { label: 'Gérer les environnements…', run: () => { S.view = 'env'; S.envSel = S.env ?? 'dev'; S.sidebar = true; render(); } },
  ], 'Environnement actif');
}

function setEnv(e) {
  S.env = e;
  render();
  if (e === 'prod') toast('Production active : les requêtes touchent des données réelles.');
}

let toastTimer;
function toast(msg) {
  const t = $('#toast');
  t.innerHTML = `${ic('check-circle', 15)}<span>${esc(msg)}</span>`;
  t.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove('show'), 2600);
}

let popAnchor = null;
let showT;
let hideT;

function popHTML(anchor) {
  if (anchor.dataset.pop === 'timing') {
    const res = S.responses[S.active];
    if (!res || res.cancelled) return '';
    return `<div class="pop-head"><b>${res.ms} ms</b><span class="faint" style="font-size:12px">${res.reused ? 'connexion réutilisée' : 'nouvelle connexion'}</span></div>${timelineRows(res, true)}
      <div class="pop-foot"><span>Mesuré côté moteur Rust</span><span>clic : timeline complète</span></div>`;
  }
  const name = anchor.dataset.var;
  if (name.startsWith('$')) {
    return `<div class="pop-head"><span class="pop-name">{{${esc(name)}}}</span><span class="tag spec">dynamique</span></div>
      <div class="pop-val">${crypto.randomUUID?.() ?? '3f0c9a1e-…'}</div><p class="faint" style="margin:0;font-size:12px">Nouvelle valeur générée à chaque envoi. Exemple ci-dessus.</p>`;
  }
  if (name.startsWith('process.env.')) {
    return `<div class="pop-head"><span class="pop-name">{{${esc(name)}}}</span><span class="tag good">.env local</span></div>
      <div class="pop-val">••••••••••</div><p class="faint" style="margin:0;font-size:12px">Lue dans le fichier .env, ignoré par Git. Jamais affichée ni exportée.</p>`;
  }
  const res = resolve(name);
  if (!res || !res.win) {
    return `<div class="pop-head"><span class="pop-name" style="color:var(--bad)">{{${esc(name)}}}</span><span class="tag bad">non définie</span></div>
      <p style="margin:0 0 4px;font-size:12.5px;color:var(--muted)">Aucune portée ne définit cette variable${S.env ? ` pour ${esc(S.env)}` : ''}. Elle partira telle quelle dans la requête.</p>
      <div class="pop-foot"><span></span><a class="link" data-act="edit-var" data-var="${esc(VARS[name] ? name : 'baseUrl')}">${VARS[name] ? `Définir dans ${esc(S.env ?? 'dev')}.yml` : 'Ouvrir les environnements'}</a></div>`;
  }
  return `<div class="pop-head"><span class="pop-name">{{${esc(name)}}}</span><span class="tag">${res.type}</span></div>
    <div class="pop-val">${esc(res.win.val)}</div>
    ${ladderHTML(res)}
    <div class="pop-foot"><span>Ordre de priorité de Bruno</span><a class="link" data-act="edit-var" data-var="${esc(name)}">Modifier</a></div>`;
}

function showPop(anchor) {
  const html = popHTML(anchor);
  if (!html) return;
  const pop = $('#pop');
  pop.innerHTML = html;
  popAnchor = anchor;
  $$('.var.is-hot').forEach((x) => x.classList.remove('is-hot'));
  anchor.classList.add('is-hot');
  const r = anchor.getBoundingClientRect();
  const w = pop.offsetWidth;
  const h = pop.offsetHeight;
  pop.style.left = `${clamp(r.left - 10, 8, innerWidth - w - 8)}px`;
  pop.style.top = `${r.bottom + h + 8 > innerHeight ? r.top - h - 8 : r.bottom + 8}px`;
  pop.classList.add('open');
}

function hidePop() {
  $('#pop').classList.remove('open');
  popAnchor?.classList.remove('is-hot');
  popAnchor = null;
}
/* ================= palette ================= */

const COMMANDS = [
  { label: 'Envoyer la requête', icon: 'send', key: 'mod+enter', run: () => send() },
  { label: 'Enregistrer', icon: 'download', key: 'mod+S', run: () => save() },
  { label: 'Importer une collection (Bruno, Postman, Insomnia, OpenAPI, cURL)', icon: 'import', run: () => toast('Import : glisse un fichier ici ou colle une commande cURL.') },
  { label: 'Lancer la synchro OpenAPI', icon: 'merge', run: () => { S.view = 'sync'; S.sidebar = true; render(); } },
  { label: 'Ouvrir Source Control', icon: 'branch', key: 'ctrl+shift+G', run: () => { S.view = 'scm'; S.sidebar = true; render(); } },
  { label: 'Gérer les environnements', icon: 'variable', run: () => { S.view = 'env'; S.sidebar = true; render(); } },
  { label: 'Copier en cURL', icon: 'terminal', run: () => S.active && copy(curlOf(REQ[S.active]), 'Commande cURL copiée') },
  { label: 'Exécuter la collection (Runner)', icon: 'play', run: () => toast('Runner : écran prévu au lot V1, non maquetté.') },
  { label: 'Basculer le thème clair / sombre', icon: 'sun', run: () => ACTIONS.theme() },
  { label: 'Empiler ou juxtaposer requête et réponse', icon: 'rows', key: 'mod+\\', run: () => toggleLayout() },
  { label: 'Afficher ou masquer la barre latérale', icon: 'cols', key: 'mod+B', run: () => toggleSidebar() },
  { label: 'Changer la langue de l\'interface', icon: 'lang', run: () => ACTIONS.lang() },
];

let palItems = [];
let palSel = 0;

function fuzzy(text, q) {
  if (!q) return { score: 0, marks: [] };
  const t = norm(text);
  const n = norm(q);
  const i = t.indexOf(n);
  if (i >= 0) return { score: i === 0 ? 0 : 1, marks: [...Array(n.length).keys()].map((k) => k + i) };
  const marks = [];
  let j = 0;
  for (let k = 0; k < t.length && j < n.length; k++) if (t[k] === n[j]) { marks.push(k); j++; }
  return j === n.length ? { score: 2 + (marks.at(-1) - marks[0]) / 50, marks } : null;
}

const markText = (text, marks) => [...text].map((ch, i) => (marks.includes(i) ? `<mark>${esc(ch)}</mark>` : esc(ch))).join('');

function paletteModel(raw) {
  const cmd = raw.startsWith('>');
  const q = (cmd ? raw.slice(1) : raw).trim();
  const visible = new Set();
  const walk = (ns) => ns.forEach((n) => (typeof n === 'string' ? visible.add(n) : walk(n.children)));
  walk(TREE);
  const groups = [];
  const take = (list, max) => list.map((it) => ({ ...it, m: fuzzy(it.label + (it.subq ?? ''), q) })).filter((it) => it.m).sort((a, b) => a.m.score - b.m.score).slice(0, max);
  if (!cmd) {
    const recent = q ? [...visible] : [...new Set([...S.tabs, ...S.history.map((h) => h.id)])];
    groups.push(['Requêtes', take(recent.map((id) => ({ kind: 'req', id, label: REQ[id].name, sub: REQ[id].url.replace('{{baseUrl}}', ''), subq: '', run: () => openReq(id, { pin: true }) })), q ? 8 : 5)]);
  }
  groups.push(['Commandes', take(COMMANDS.map((c) => ({ ...c, kind: 'cmd' })), cmd || q ? 12 : 4)]);
  if (!cmd) groups.push(['Environnements', take(Object.keys(ENVS).map((e) => ({ kind: 'env', env: e, label: `Environnement : ${e}`, run: () => setEnv(e) })), 3)]);
  return groups.filter(([, items]) => items.length);
}

function renderPalette() {
  const raw = $('#palInput').value;
  const groups = paletteModel(raw);
  palItems = groups.flatMap(([, items]) => items);
  palSel = clamp(palSel, 0, Math.max(0, palItems.length - 1));
  let k = 0;
  $('#palList').innerHTML = groups.map(([title, items]) => `<div class="pal-group">${title}</div>${items.map((it) => {
    const i = k++;
    const lead = it.kind === 'req' ? `<span class="${mcls(REQ[it.id].method)}">${short(REQ[it.id].method)}</span>` : it.kind === 'env' ? `<span class="env-dot ${it.env}" style="margin:0 4px"></span>` : ic(it.icon, 15);
    return `<button class="pal-item" role="option" id="pal-${i}" aria-selected="${i === palSel}" data-pal="${i}">${lead}<span class="lbl">${markText(it.label, it.m.marks)}</span>
      <span class="sub">${esc(it.sub ?? '')}</span>${it.key ? kbd(it.key) : ''}</button>`;
  }).join('')}`).join('') || `<div class="pal-empty">Aucun résultat pour « ${esc(raw.replace(/^>/, ''))} ».</div>`;
  $('#palInput').setAttribute('aria-activedescendant', `pal-${palSel}`);
  $('#palFoot').innerHTML = `<span>${kbd('↑')}${kbd('↓')} naviguer</span><span>${kbd('enter')} ouvrir</span><span>${kbd('>')} commandes</span><span>${kbd('esc')} fermer</span>`;
}

function openPalette(prefix = '') {
  closeMenu();
  hidePop();
  const wrap = $('#palette');
  wrap.hidden = false;
  const input = $('#palInput');
  input.value = prefix;
  palSel = 0;
  renderPalette();
  input.focus();
  input.setSelectionRange(input.value.length, input.value.length);
}

function closePalette() { $('#palette').hidden = true; }

function runPal(i) {
  const it = palItems[i];
  if (!it) return;
  closePalette();
  it.run();
}

/* ================= événements ================= */

document.addEventListener('click', (e) => {
  if (e.target.id === 'palette') { closePalette(); return; }
  const mi = e.target.closest('[data-mi]');
  if (mi) { const it = $('#menu')._items[Number(mi.dataset.mi)]; closeMenu(); it.run(); return; }
  const pi = e.target.closest('[data-pal]');
  if (pi) { runPal(Number(pi.dataset.pal)); return; }
  if (!e.target.closest('#menu') && !e.target.closest('[aria-haspopup]')) closeMenu();
  const el = e.target.closest('[data-act]');
  if (!el) return;
  const fn = ACTIONS[el.dataset.act];
  if (fn) fn(el, e);
});

document.addEventListener('dblclick', (e) => {
  const el = e.target.closest('[data-act="open"], [data-act="tab"]');
  if (el && S.preview === el.dataset.id) { S.preview = null; paintTabs(); }
});

document.addEventListener('change', (e) => {
  const t = e.target;
  const r = REQ[S.active];
  if (t.dataset.q !== undefined) { r.query[t.dataset.q].on = t.checked; markDirty(); refreshRequest(); paintReq(); }
  else if (t.dataset.h !== undefined) { r.headers[t.dataset.h].on = t.checked; t.closest('.kv-row').classList.toggle('off', !t.checked); markDirty(); refreshRequest(); }
  else if (t.dataset.a !== undefined) { r.assertions[t.dataset.a].on = t.checked; t.closest('.kv-row').classList.toggle('off', !t.checked); markDirty(); refreshRequest(); paintRes(); }
});

document.addEventListener('input', (e) => {
  const t = e.target;
  if (t.id === 'treeFilter') { S.filter = t.value; paintTree(); return; }
  if (t.id === 'jpInput') {
    S.jsonFilter = t.value;
    const res = S.responses[S.active];
    $('#resCode').innerHTML = bodyCode(res);
    $('#jpWrap').classList.toggle('has-error', !jsonPath(res.data, t.value).ok);
    return;
  }
  if (t.id === 'commitMsg') { S.commitMsg = t.value; if (S.commitError && t.value.trim()) { S.commitError = false; t.setAttribute('aria-invalid', 'false'); t.nextElementSibling?.remove(); } return; }
  if (t.id === 'palInput') { palSel = 0; renderPalette(); return; }
  if (t.dataset.edit) { const [id, i] = t.dataset.edit.split(':'); S.edits[id] ??= resultLines(CONFLICTS.find((c) => c.id === id)).map((l) => l.t); S.edits[id][i] = t.textContent; return; }
  const r = REQ[S.active];
  if (t.dataset.qk !== undefined) { r.query[t.dataset.qk].k = t.value; markDirty(); refreshRequest(); }
  else if (t.dataset.qv !== undefined) { r.query[t.dataset.qv].v = t.value; markDirty(); refreshRequest(); }
});

document.addEventListener('keydown', (e) => {
  const mod = isMac ? e.metaKey : e.ctrlKey;
  const key = e.key.toLowerCase();
  const palOpen = !$('#palette').hidden;

  if (palOpen) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      palSel = clamp(palSel + (e.key === 'ArrowDown' ? 1 : -1), 0, palItems.length - 1);
      renderPalette();
      $(`#pal-${palSel}`)?.scrollIntoView({ block: 'nearest' });
      return;
    }
    if (e.key === 'Enter') { e.preventDefault(); runPal(palSel); return; }
    if (e.key === 'Escape') { e.preventDefault(); closePalette(); return; }
  }

  if (e.target.dataset?.qnew !== undefined && e.key === 'Enter' && e.target.value.trim()) {
    REQ[S.active].query.push({ k: e.target.value.trim(), v: '', on: true });
    markDirty();
    refreshRequest();
    paintReq();
    const inputs = document.querySelectorAll('[data-qv]');
    inputs[inputs.length - 1]?.focus();
    return;
  }
  if (e.target.id === 'commitMsg' && mod && e.key === 'Enter') { e.preventDefault(); ACTIONS.commit(); return; }
  if (e.target.closest?.('.tab') && (e.key === 'Enter' || e.key === ' ') && e.target.classList.contains('tab')) { e.preventDefault(); e.target.click(); return; }

  if (mod && key === 'k') { e.preventDefault(); openPalette(''); }
  else if (mod && e.shiftKey && key === 'p') { e.preventDefault(); openPalette('>'); }
  else if (mod && e.key === 'Enter') { e.preventDefault(); send(); }
  else if (mod && key === 's') { e.preventDefault(); save(); }
  else if (mod && key === 'b') { e.preventDefault(); toggleSidebar(); }
  else if (mod && e.key === '\\') { e.preventDefault(); toggleLayout(); }
  else if (e.ctrlKey && e.shiftKey && key === 'g') { e.preventDefault(); go('scm'); }
  else if (e.altKey && S.view === 'sync' && !S.synced && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) { e.preventDefault(); navConflict(e.key === 'ArrowDown' ? 1 : -1); }
  else if (e.key === 'Escape') {
    if (!$('#menu').hidden) closeMenu();
    else if (popAnchor) hidePop();
    else if (S.sending) cancelSend();
  }
});

document.addEventListener('mouseover', (e) => {
  const t = e.target.closest('[data-var], [data-pop]');
  if (t && !t.closest('#pop')) {
    clearTimeout(hideT);
    if (popAnchor !== t) { clearTimeout(showT); showT = setTimeout(() => showPop(t), 220); }
    return;
  }
  if (e.target.closest('#pop')) { clearTimeout(hideT); return; }
  clearTimeout(showT);
  if (popAnchor) { clearTimeout(hideT); hideT = setTimeout(hidePop, 180); }
});

document.addEventListener('focusin', (e) => {
  const t = e.target.closest?.('.var[data-var]');
  if (t) showPop(t);
  else if (!e.target.closest?.('#pop') && popAnchor) hidePop();
});

document.addEventListener('pointerdown', (e) => {
  const h = e.target.closest('[data-drag]');
  if (!h) return;
  e.preventDefault();
  h.classList.add('dragging');
  h.setPointerCapture(e.pointerId);
  const kind = h.dataset.drag;
  const move = (ev) => {
    if (kind === 'sidebar') {
      const w = clamp(ev.clientX - 44, 200, 460);
      document.documentElement.style.setProperty('--sb', `${w}px`);
      return;
    }
    const split = $('#split');
    const r = split.getBoundingClientRect();
    if (S.layout === 'v' || getComputedStyle(h).cursor === 'row-resize') {
      S.reqhPct = clamp(((ev.clientY - r.top) / r.height) * 100, 20, 80);
      split.style.setProperty('--reqh', `${S.reqhPct}%`);
    } else {
      S.reqPct = clamp(((ev.clientX - r.left) / r.width) * 100, 25, 72);
      split.style.setProperty('--req', `${S.reqPct}%`);
    }
  };
  const up = () => { h.classList.remove('dragging'); h.removeEventListener('pointermove', move); h.removeEventListener('pointerup', up); };
  h.addEventListener('pointermove', move);
  h.addEventListener('pointerup', up);
});

window.addEventListener('resize', () => { closeMenu(); hidePop(); });

const boot = new URLSearchParams(location.search);
if (boot.get('theme')) document.documentElement.dataset.theme = boot.get('theme');
const startView = location.hash.slice(1);
if (['collections', 'env', 'scm', 'sync'].includes(startView)) S.view = startView;
render();
if (startView === 'palette') openPalette('');
