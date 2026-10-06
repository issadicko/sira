import type { Api } from './api';
import { createDemoSync } from './demo-sync';
import { createDemoEnvironments } from './demo-env';
import { createDemoRunner } from './demo-runner';
import { DEMO_FOLDERS, DemoCollection, collectionAt, createDemoTree, refreshItem } from './demo-tree';
import { emptyReport } from './scripts';
import { CollectionInfo, EnvVar, KeyValue, OpenApiPreview, Param, RequestDoc, Rung, SendResult, TokenInfo, TreeItem, VariableInfo } from './model';

const ROOT = '~/démo/api-paiements';
const EXPORT = 'transactions/export.yml';

const doc = (name: string, method: string, url: string, extra: Partial<RequestDoc> = {}): RequestDoc => ({
  name,
  requestType: 'http',
  seq: 1,
  method,
  url,
  params: [],
  headers: [],
  body: { type: 'none' },
  auth: { type: 'inherit' },
  assertions: [{ expression: 'res.status', operator: 'eq', value: '200', enabled: true }],
  variables: [],
  scripts: [],
  docs: null,
  timeoutMs: null,
  ...extra,
});

const pathParam = (name: string, value: string): Param => ({ name, value, kind: 'path', enabled: true });
const header = (name: string, value: string): KeyValue => ({ name, value, enabled: true });

const desktopOnly = (): Promise<never> => Promise.reject("Disponible dans l'application desktop");

const files: Record<string, RequestDoc> = {
  'auth/connexion.yml': doc('Connexion', 'POST', '{{baseUrl}}/auth/connexion', {
    auth: { type: 'none' },
    body: { type: 'json', data: '{\n  "identifiant": "caisse-ouaga-01",\n  "motDePasse": "{{process.env.PSP_PASSWORD}}"\n}' },
  }),
  'auth/jeton.yml': doc('Jeton OAuth', 'POST', '{{baseUrl}}/auth/jeton', {
    auth: { type: 'none' },
    body: {
      type: 'form-urlencoded',
      fields: [
        { name: 'grant_type', value: 'client_credentials', enabled: true, description: null },
        { name: 'client_id', value: 'caisse-ouaga-01', enabled: true, description: 'Identifiant de la caisse' },
        { name: 'client_secret', value: '{{process.env.PSP_SECRET}}', enabled: true, description: null },
        { name: 'scope', value: 'transactions:lire', enabled: false, description: null },
      ],
    },
  }),
  'transactions/liste.yml': doc('Liste des transactions', 'GET', '{{baseUrl}}/transactions?page=1&statut=CONFIRMEE', {
    params: [
      { name: 'page', value: '1', kind: 'query', enabled: true },
      { name: 'statut', value: 'CONFIRMEE', kind: 'query', enabled: true },
    ],
  }),
  'transactions/detail.yml': doc("Détail d'une transaction", 'GET', '{{baseUrl}}/transactions/:id?expand=client', {
    params: [{ name: 'expand', value: 'client', kind: 'query', enabled: true }, pathParam('id', '{{txId}}')],
    headers: [header('X-Canal', '{{canal}}')],
    assertions: [
      { expression: 'res.status', operator: 'eq', value: '200', enabled: true },
      { expression: 'res.body.devise', operator: 'eq', value: 'XOF', enabled: true },
    ],
  }),
  'transactions/annuler.yml': doc('Annuler une transaction', 'PATCH', '{{baseUrl}}/transactions/:id/annuler', {
    params: [pathParam('id', '{{txId}}')],
    body: { type: 'json', data: '{\n  "motif": "Erreur de saisie",\n  "canal": "USSD"\n}' },
  }),
  'transactions/justificatif.yml': doc('Joindre un justificatif', 'POST', '{{baseUrl}}/transactions/:id/justificatif', {
    params: [pathParam('id', '{{txId}}')],
    body: {
      type: 'multipart-form',
      fields: [
        { name: 'motif', kind: 'text', value: 'Reçu client', enabled: true, contentType: null, description: null },
        { name: 'justificatif', kind: 'file', value: ['pieces/recu-0042.pdf'], enabled: true, contentType: 'application/pdf', description: null },
        { name: 'annexes', kind: 'file', value: ['pieces/photo-caisse.jpg', '../partage/signature.png'], enabled: true, contentType: null, description: null },
        { name: 'commentaire', kind: 'text', value: '', enabled: false, contentType: null, description: null },
      ],
    },
  }),
  [EXPORT]: doc('Export des transactions (10 Mo)', 'GET', '{{baseUrl}}/transactions/export'),
  'transactions/supprimer.yml': doc('Supprimer une transaction', 'DELETE', '{{baseUrl}}/transactions/:id', {
    params: [pathParam('id', '{{txId}}')],
    assertions: [{ expression: 'res.status', operator: 'eq', value: '204', enabled: true }],
  }),
};

const DEPRECATED = new Set(['transactions/supprimer.yml']);

function treeRequest(path: string): TreeItem {
  const { name, method, url } = files[path];
  return { kind: 'request', path, name, method, requestType: 'http', url, deprecated: DEPRECATED.has(path) };
}

const collection: CollectionInfo = {
  root: ROOT,
  name: 'API Paiements (démo)',
  environments: ['dev', 'prod'],
  defaultEnvironment: 'dev',
  requestCount: 8,
  items: [
    { kind: 'folder', path: 'auth', name: 'Auth', seq: 1, children: ['connexion', 'jeton'].map((f) => treeRequest(`auth/${f}.yml`)) },
    {
      kind: 'folder',
      path: 'transactions',
      name: 'Transactions',
      seq: 2,
      children: ['liste', 'detail', 'annuler', 'justificatif', 'export', 'supprimer'].map((f) => treeRequest(`transactions/${f}.yml`)),
    },
  ],
};

const variable = (name: string, value: string): EnvVar => ({ name, value, secret: false, enabled: true, description: null, dataType: null });
const secret = (name: string): EnvVar => ({ name, value: null, secret: true, enabled: true, description: null, dataType: null });

const environments: Record<string, EnvVar[]> = {
  dev: [variable('baseUrl', 'https://api.dev.local/v1'), variable('txId', 'TX-2026-0042'), variable('canal', 'MOBILE'), secret('token')],
  prod: [variable('baseUrl', 'https://api.paiements.example/v1'), variable('canal', 'MOBILE'), secret('token')],
};

const collections = new Map<string, DemoCollection>([[ROOT, { info: collection, files, environments }]]);
const PARENT = '~/démo';
const picks = [ROOT, ...Object.keys(DEMO_FOLDERS)];
let picked = 0;

/** Valeur d'une variable activée de l'environnement `env`, telle que l'enregistrement l'a laissée. */
const envValue = (env: string | null, name: string): string | null =>
  (env && environments[env]?.find((v) => v.name === name && v.enabled && !v.secret)?.value) || null;
const collectionVars: Record<string, string> = { baseUrl: 'https://api.paiements.test/v1' };
const folderVars: Record<string, string> = { canal: 'USSD' };

function variables(path: string, env: string | null): VariableInfo[] {
  const names = new Set([...Object.values(environments).flatMap((vars) => vars.filter((v) => !v.secret).map((v) => v.name)), ...Object.keys(collectionVars), ...Object.keys(folderVars), 'token']);
  return [...names].sort().map((name) => {
    const rungs: Rung[] = [
      { level: 'Runtime', source: 'bru.setVar()', value: null },
      { level: 'Requête', source: path, value: null },
      { level: 'Dossier Transactions', source: 'transactions/folder.yml', value: path.startsWith('transactions/') ? folderVars[name] ?? null : null },
      { level: `Environnement ${env ?? '(aucun)'}`, source: `environments/${env}.yml`, value: envValue(env, name) },
      { level: 'Collection', source: 'opencollection.yml', value: collectionVars[name] ?? null },
    ];
    const win = rungs.find((r) => r.value != null);
    return { name, value: win?.value ?? null, level: win?.level ?? null, secret: name === 'token', rungs };
  });
}

const body = (path: string) =>
  path === 'transactions/detail.yml'
    ? { id: 'TX-2026-0042', montant: 15000, frais: 150, devise: 'XOF', statut: 'CONFIRMEE', client: { id: 'CL-00318', nom: 'Aminata Ouédraogo' }, creeLe: '2026-09-29T10:42:11Z' }
    : { ok: true, source: 'mode démo du navigateur' };

const transactions = (count: number) =>
  Array.from({ length: count }, (_, i) => ({
    id: `TX-2026-${String(i).padStart(6, '0')}`,
    montant: 1500 + (i % 997) * 25,
    frais: 150,
    devise: 'XOF',
    statut: i % 7 ? 'CONFIRMEE' : 'ANNULEE',
    canal: 'MOBILE',
    client: { id: `CL-${String(i % 9000).padStart(5, '0')}`, nom: 'Aminata Ouédraogo' },
    creeLe: '2026-09-29T10:42:11Z',
  }));

let exported: { body: string; pretty: string } | undefined;

function payload(path: string): { body: string; pretty: string } {
  if (path !== EXPORT) {
    const text = JSON.stringify(body(path), null, 2);
    return { body: text, pretty: text };
  }
  if (!exported) {
    const value = { total: 56_000, donnees: transactions(56_000) };
    exported = { body: JSON.stringify(value), pretty: JSON.stringify(value, null, 2) };
  }
  return exported;
}

const SPEC: OpenApiPreview = {
  folderName: 'Petstore (démo)',
  summary: {
    title: 'Petstore (démo)',
    version: '1.0.0',
    format: 'openapi',
    formatVersion: '3.0.3',
    operationCount: 19,
    tags: ['pets', 'boutique', 'utilisateurs'],
    servers: ['https://petstore.example/v1', 'https://recette.petstore.example/v1'],
  },
};

const timers = new Map<string, () => void>();
/** Les jetons OAuth 2 de la démo, par requête : rien n'est demandé à un serveur. */
const tokens = new Map<string, TokenInfo>();

const demoSync = createDemoSync(files);

/**
 * Le sélecteur de dossier est simulé : « Ouvrir » propose tour à tour la collection de démo, un dossier vide, un dossier
 * non vide et un dossier .bru, pour que chaque cas de l'ouverture soit atteignable ; le dossier parent d'une nouvelle collection est fixe.
 */
export const demoApi: Api = {
  demo: true,
  pickFolder: async (title) => (title ? PARENT : picks[picked++ % picks.length]),
  pickSpecFile: async () => '~/démo/petstore.yaml',
  pickFile: async () => `${ROOT}/pieces/recu-0043.pdf`,
  openCollection: async (root) => structuredClone(collectionAt(collections, root).info),
  watchCollection: async () => undefined,
  onDiskChange: async () => () => undefined,
  readRequest: async (root, path) => {
    const doc = collectionAt(collections, root).files[path];
    if (!doc) throw `Fichier introuvable : ${path}`;
    return structuredClone(doc);
  },
  saveRequest: async (root, path, d) => {
    const c = collectionAt(collections, root);
    c.files[path] = structuredClone(d);
    refreshItem(c, path, d);
    return true;
  },
  variables: async (_root, path, _doc, env) => variables(path, env),
  oauthStatus: async (_root, path) => structuredClone(tokens.get(path) ?? null),
  oauthFetch: async (_root, path, doc) => {
    if (doc.auth.type !== 'oauth2') throw "Cette requête n'utilise pas OAuth 2";
    await new Promise((resolve) => setTimeout(resolve, 700));
    const token: TokenInfo = { id: doc.auth.tokenId, tokenType: 'Bearer', scope: doc.auth.scope || null, expiresAt: Date.now() + 3_600_000, expired: false, hasRefreshToken: doc.auth.flow !== 'client_credentials' };
    tokens.set(path, token);
    return structuredClone(token);
  },
  oauthClear: async (_root, path) => void tokens.delete(path),
  send: (id, _root, path, d, env) =>
    new Promise<SendResult>((resolve, reject) => {
      const t = setTimeout(() => {
        timers.delete(id);
        const vars = Object.fromEntries(variables(path, env).map((v) => [v.name, v.value]));
        const unresolved: string[] = [];
        const url = d.url.replace(/\{\{([^}]+)\}\}/g, (all, n: string) => {
          const v = vars[n.trim()];
          if (v == null) unresolved.push(n.trim());
          return v ?? all;
        });
        const { body: text, pretty } = payload(path);
        const total = 90 + Math.round(Math.random() * 60);
        resolve({
          method: d.method,
          url,
          unresolved,
          assertions: d.assertions.filter((a) => a.enabled).map((a) => ({ expression: a.expression, operator: a.operator, expected: a.value, actual: '200', passed: a.expression === 'res.status' || a.value === 'XOF' })),
          scripts: emptyReport(),
          skipped: false,
          response: {
            status: 200,
            reason: 'OK',
            httpVersion: 'HTTP/1.1',
            remoteAddr: '127.0.0.1:443',
            headers: [['content-type', 'application/json'], ['content-length', String(text.length)]],
            body: text,
            pretty,
            size: text.length,
            timings: { dnsMs: 4, tcpMs: 11, tlsMs: 28, ttfbMs: total - 55, downloadMs: 12, totalMs: total },
          },
        });
      }, 700);
      timers.set(id, () => {
        clearTimeout(t);
        reject('Requête annulée');
      });
    }),
  cancel: async (id) => {
    timers.get(id)?.();
    return timers.delete(id);
  },
  parseCurl: desktopOnly,
  createRequestFromCurl: desktopOnly,
  previewOpenApi: async () => structuredClone(SPEC),
  importOpenApi: desktopOnly,
  ...createDemoTree(collections),
  ...createDemoEnvironments(collections),
  ...createDemoRunner(collections),
  ...demoSync,
  syncStatus: async (root) =>
    root === ROOT ? demoSync.syncStatus(root) : { connected: false, source: null, groupBy: null, operationCount: 0, removedCount: 0 },
};
