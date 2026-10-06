import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';

import {
  CodeLanguage,
  CollectionInfo,
  CookieDraft,
  CookieKey,
  CookieView,
  DataInfo,
  DiskChange,
  DropPosition,
  EnvVar,
  ExportArgs,
  FolderKind,
  GeneratedCode,
  GroupBy,
  HistoryEntry,
  ExportFormat,
  ImportedCollection,
  NetworkPrefs,
  NetworkView,
  OpenApiPreview,
  OpView,
  ReportFormat,
  RequestDoc,
  RunArgs,
  RunDone,
  RunEvent,
  SendResult,
  StoredSchema,
  SyncDecisions,
  SyncPlan,
  SyncReport,
  SyncStatus,
  TokenInfo,
  VariableInfo,
} from './model';

export interface Api {
  readonly demo: boolean;
  pickFolder(title?: string): Promise<string | null>;
  pickSpecFile(): Promise<string | null>;
  pickFile(defaultDir?: string): Promise<string | null>;
  pickCertificate(): Promise<string | null>;
  inspectFolder(path: string): Promise<FolderKind>;
  createCollection(parent: string, name: string): Promise<string>;
  initCollection(dir: string, name: string): Promise<string>;
  openCollection(root: string): Promise<CollectionInfo>;
  /** Surveille le dossier de la collection ; les changements arrivent par `onDiskChange`. Remplace la surveillance précédente. */
  watchCollection(root: string): Promise<void>;
  /** S'abonne aux changements du disque ; renvoie la fonction qui se désabonne. */
  onDiskChange(handler: (change: DiskChange) => void): Promise<() => void>;
  createRequest(root: string, folder: string, name: string): Promise<string>;
  createGraphqlRequest(root: string, folder: string, name: string): Promise<string>;
  createFolder(root: string, parent: string, name: string): Promise<string>;
  renameItem(root: string, path: string, name: string): Promise<string>;
  cloneItem(root: string, path: string, name: string): Promise<string>;
  deleteItem(root: string, path: string): Promise<void>;
  moveItem(root: string, path: string, target: string, position: DropPosition): Promise<string>;
  readRequest(root: string, path: string): Promise<RequestDoc>;
  saveRequest(root: string, path: string, doc: RequestDoc): Promise<boolean>;
  readEnvironment(root: string, name: string): Promise<EnvVar[]>;
  /** Enregistre les variables de l'environnement ; un fichier absent est refusé sauf avec `create`. `false` : rien n'a changé. */
  saveEnvironment(root: string, name: string, vars: EnvVar[], create: boolean): Promise<boolean>;
  createEnvironment(root: string, name: string): Promise<string>;
  renameEnvironment(root: string, from: string, name: string): Promise<string>;
  cloneEnvironment(root: string, from: string, name: string): Promise<string>;
  deleteEnvironment(root: string, name: string): Promise<void>;
  /** Environnement que la collection ouvre par défaut ; `null` pour n'en choisir aucun. */
  setDefaultEnvironment(root: string, name: string | null): Promise<void>;
  variables(root: string, path: string, doc: RequestDoc, env: string | null): Promise<VariableInfo[]>;
  /** Le code de la requête dans `language`, variables résolues ; les secrets y sont des repères `<nom>`. */
  generateCode(root: string, path: string, doc: RequestDoc, env: string | null, language: CodeLanguage): Promise<GeneratedCode>;
  /** Le schéma GraphQL déjà gardé pour l'URL de la requête, sans appel réseau ; `null` s'il n'y en a pas. */
  graphqlSchema(root: string, path: string, doc: RequestDoc, env: string | null): Promise<StoredSchema | null>;
  /** Demande le schéma au serveur de la requête (introspection) et le garde pour les prochaines ouvertures. */
  graphqlFetchSchema(root: string, path: string, doc: RequestDoc, env: string | null): Promise<StoredSchema>;
  send(id: string, root: string, path: string, doc: RequestDoc, env: string | null): Promise<SendResult>;
  cancel(id: string): Promise<boolean>;
  /** Les requêtes envoyées depuis la collection, la plus récente d'abord. */
  historyList(root: string): Promise<HistoryEntry[]>;
  historyClear(root: string): Promise<void>;
  cookiesList(): Promise<CookieView[]>;
  /** Ajoute un cookie, ou remplace `previous` quand il est donné ; rend le pot à jour. */
  cookieSave(previous: CookieKey | null, cookie: CookieDraft): Promise<CookieView[]>;
  cookieDelete(cookie: CookieKey): Promise<CookieView[]>;
  cookiesDeleteDomain(domain: string): Promise<CookieView[]>;
  cookiesClear(): Promise<CookieView[]>;
  networkGet(): Promise<NetworkView>;
  /** `proxyPassword` : `null` garde le mot de passe en place, une chaîne vide l'oublie, un texte le range dans le trousseau. */
  networkSave(prefs: NetworkPrefs, proxyPassword: string | null): Promise<NetworkView>;
  parseCurl(command: string): Promise<RequestDoc | null>;
  createRequestFromCurl(root: string, folder: string, name: string, command: string): Promise<string>;
  previewOpenApi(source: string): Promise<OpenApiPreview>;
  importOpenApi(source: string, location: string, groupBy: GroupBy): Promise<string>;
  pickPostmanFile(): Promise<string | null>;
  /** Importe un export Postman (collection v2.0 ou v2.1) dans un nouveau dossier de `location`. */
  importPostman(source: string, location: string): Promise<ImportedCollection>;
  /** Ajoute un environnement Postman à la collection `root` ; renvoie son nom. */
  importPostmanEnvironment(root: string, source: string): Promise<string>;
  /** Convertit une collection `.bru` (dossier avec `bruno.json`) en YAML dans un nouveau dossier de `location` ; la source n'est pas modifiée. */
  importBru(source: string, location: string): Promise<ImportedCollection>;
  pickInsomniaFile(): Promise<string | null>;
  /** Importe un export Insomnia (v4 JSON ou v5 YAML), environnements compris, dans un nouveau dossier de `location`. */
  importInsomnia(source: string, location: string): Promise<ImportedCollection>;
  /** Où enregistrer l'export d'une collection ; `null` si l'utilisateur renonce. */
  pickExportPath(defaultName: string): Promise<string | null>;
  /** Exporte la collection `root` dans le fichier `path` ; renvoie ce qui n'a pas pu l'être, un message par élément. */
  exportCollection(root: string, format: ExportFormat, path: string): Promise<string[]>;
  syncStatus(root: string): Promise<SyncStatus>;
  syncPlan(root: string, source: string | null, pairings: [string, string][]): Promise<SyncPlan>;
  syncOpView(planId: string, key: string, decisions: SyncDecisions): Promise<OpView>;
  syncApply(planId: string, decisions: SyncDecisions): Promise<SyncReport>;
  pickDataFile(): Promise<string | null>;
  /** Où enregistrer un rapport ; `null` si l'utilisateur renonce. */
  pickSavePath(defaultName: string, format: ReportFormat): Promise<string | null>;
  inspectRunData(path: string): Promise<DataInfo>;
  /** Exécute la sélection ; les requêtes arrivent une à une par `onRunEvent`, le résumé à la fin. */
  startRun(args: RunArgs): Promise<RunDone>;
  cancelRun(runId: string): Promise<boolean>;
  onRunEvent(handler: (event: RunEvent) => void): Promise<() => void>;
  exportRun(args: ExportArgs): Promise<void>;
  /** Le jeton OAuth 2 gardé pour la requête (sans sa valeur) ; `null` s'il n'y en a pas. */
  oauthStatus(root: string, path: string, doc: RequestDoc, env: string | null): Promise<TokenInfo | null>;
  /** Demande un jeton neuf ; les flux interactifs ouvrent une fenêtre de connexion. */
  oauthFetch(root: string, path: string, doc: RequestDoc, env: string | null): Promise<TokenInfo>;
  oauthClear(root: string, path: string, doc: RequestDoc, env: string | null): Promise<void>;
  /** Les secrets de l'environnement dont le trousseau garde une valeur ; l'interface ne reçoit jamais plus que leurs noms. */
  secretNames(root: string, env: string): Promise<string[]>;
}

const REPORT_FILTERS: Record<ReportFormat, { name: string; extensions: string[] }> = {
  html: { name: 'Page HTML', extensions: ['html'] },
  junit: { name: 'JUnit (XML)', extensions: ['xml'] },
  json: { name: 'JSON', extensions: ['json'] },
};

async function pick(options: Parameters<typeof open>[0]): Promise<string | null> {
  const picked = await open({ ...options, multiple: false });
  return typeof picked === 'string' ? picked : null;
}

const tauriApi: Api = {
  demo: false,
  pickFolder: (title = 'Ouvrir un dossier') => pick({ directory: true, title }),
  pickSpecFile: () =>
    pick({ title: 'Choisir une spécification OpenAPI', filters: [{ name: 'OpenAPI et Swagger', extensions: ['yaml', 'yml', 'json'] }] }),
  pickFile: (defaultDir) => pick({ title: 'Choisir un fichier à envoyer', defaultPath: defaultDir }),
  pickCertificate: () =>
    pick({ title: 'Choisir un fichier d\'autorités de certification', filters: [{ name: 'Certificats PEM', extensions: ['pem', 'crt', 'cer'] }] }),
  inspectFolder: (path) => invoke('inspect_folder', { path }),
  createCollection: (parent, name) => invoke('create_collection', { parent, name }),
  initCollection: (dir, name) => invoke('init_collection', { dir, name }),
  openCollection: (root) => invoke('open_collection', { root }),
  watchCollection: (root) => invoke('watch_collection', { root }),
  onDiskChange: async (handler) => {
    const { listen } = await import('@tauri-apps/api/event');
    return listen<DiskChange>('collection-changed', (event) => handler(event.payload));
  },
  createRequest: (root, folder, name) => invoke('create_request', { root, folder, name }),
  createGraphqlRequest: (root, folder, name) => invoke('create_graphql_request', { root, folder, name }),
  createFolder: (root, parent, name) => invoke('create_folder', { root, parent, name }),
  renameItem: (root, path, name) => invoke('rename_item', { root, path, name }),
  cloneItem: (root, path, name) => invoke('clone_item', { root, path, name }),
  deleteItem: (root, path) => invoke('delete_item', { root, path }),
  moveItem: (root, path, target, position) => invoke('move_item', { root, path, target, position }),
  readRequest: (root, path) => invoke('read_request', { root, path }),
  saveRequest: (root, path, doc) => invoke('save_request', { root, path, doc }),
  readEnvironment: (root, name) => invoke('read_environment', { root, name }),
  saveEnvironment: (root, name, vars, create) => invoke('save_environment', { root, name, vars, create }),
  createEnvironment: (root, name) => invoke('create_environment', { root, name }),
  renameEnvironment: (root, from, name) => invoke('rename_environment', { root, from, name }),
  cloneEnvironment: (root, from, name) => invoke('clone_environment', { root, from, name }),
  deleteEnvironment: (root, name) => invoke('delete_environment', { root, name }),
  setDefaultEnvironment: (root, name) => invoke('set_default_environment', { root, name }),
  variables: (root, path, doc, env) => invoke('variables', { root, path, doc, env }),
  generateCode: (root, path, doc, env, language) => invoke('generate_code', { root, path, doc, env, language }),
  graphqlSchema: (root, path, doc, env) => invoke('graphql_schema', { root, path, doc, env }),
  graphqlFetchSchema: (root, path, doc, env) => invoke('graphql_fetch_schema', { root, path, doc, env }),
  send: (id, root, path, doc, env) => invoke('send_request', { args: { id, root, path, doc, env } }),
  cancel: (id) => invoke('cancel_request', { id }),
  historyList: (root) => invoke('history_list', { root }),
  historyClear: (root) => invoke('history_clear', { root }),
  cookiesList: () => invoke('cookies_list'),
  cookieSave: (previous, cookie) => invoke('cookie_save', { previous, cookie }),
  cookieDelete: (cookie) => invoke('cookie_delete', { cookie }),
  cookiesDeleteDomain: (domain) => invoke('cookies_delete_domain', { domain }),
  cookiesClear: () => invoke('cookies_clear'),
  networkGet: () => invoke('network_get'),
  networkSave: (prefs, proxyPassword) => invoke('network_save', { prefs, proxyPassword }),
  parseCurl: (command) => invoke('parse_curl', { command }),
  createRequestFromCurl: (root, folder, name, command) => invoke('create_request_from_curl', { root, folder, name, command }),
  previewOpenApi: (source) => invoke('preview_openapi', { source }),
  importOpenApi: (source, location, groupBy) => invoke('import_openapi', { source, location, groupBy }),
  pickPostmanFile: () => pick({ title: 'Choisir un export Postman', filters: [{ name: 'Export Postman (JSON)', extensions: ['json'] }] }),
  importPostman: (source, location) => invoke('import_postman', { source, location }),
  importPostmanEnvironment: (root, source) => invoke('import_postman_environment', { root, source }),
  pickInsomniaFile: () => pick({ title: 'Choisir un export Insomnia', filters: [{ name: 'Export Insomnia (JSON ou YAML)', extensions: ['json', 'yaml', 'yml'] }] }),
  importInsomnia: (source, location) => invoke('import_insomnia', { source, location }),
  importBru: (source, location) => invoke('import_bru', { source, location }),
  pickExportPath: (defaultName) => save({ title: 'Exporter la collection', defaultPath: defaultName, filters: [{ name: 'JSON', extensions: ['json'] }] }),
  exportCollection: (root, format, path) => invoke('export_collection', { root, format, path }),
  syncStatus: (root) => invoke('sync_status', { root }),
  syncPlan: (root, source, pairings) => invoke('sync_plan', { root, source, pairings }),
  syncOpView: (planId, key, decisions) => invoke('sync_op_view', { planId, key, decisions }),
  syncApply: (planId, decisions) => invoke('sync_apply', { planId, decisions }),
  pickDataFile: () => pick({ title: "Choisir un fichier de données d'itération", filters: [{ name: 'CSV ou JSON', extensions: ['csv', 'json'] }] }),
  pickSavePath: (defaultName, format) => save({ title: 'Enregistrer le rapport', defaultPath: defaultName, filters: [REPORT_FILTERS[format]] }),
  inspectRunData: (path) => invoke('inspect_run_data', { path }),
  startRun: (args) => invoke('start_run', { args }),
  cancelRun: (runId) => invoke('cancel_run', { runId }),
  onRunEvent: async (handler) => {
    const { listen } = await import('@tauri-apps/api/event');
    return listen<RunEvent>('run-event', (event) => handler(event.payload));
  },
  exportRun: (args) => invoke('export_run', { args }),
  oauthStatus: (root, path, doc, env) => invoke('oauth_status', { args: { root, path, doc, env } }),
  oauthFetch: (root, path, doc, env) => invoke('oauth_fetch', { args: { root, path, doc, env } }),
  oauthClear: (root, path, doc, env) => invoke('oauth_clear', { args: { root, path, doc, env } }),
  secretNames: (root, env) => invoke('secret_names', { root, env }),
};

type Call = (...args: unknown[]) => unknown;

/** Le mode démo n'est chargé qu'au premier appel, dans le navigateur : l'application desktop n'embarque pas son code au démarrage. */
const demoApi: Api = new Proxy({ demo: true } as Api, {
  get: (target, key) =>
    typeof key !== 'string' || key === 'then'
      ? undefined
      : key === 'demo'
        ? target.demo
        : (...args: unknown[]) => import('./demo').then((m) => (m.demoApi as unknown as Record<string, Call>)[key](...args)),
});

export const isTauri = '__TAURI_INTERNALS__' in window;
export const api: Api = isTauri ? tauriApi : demoApi;
