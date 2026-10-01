import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

import { demoApi } from './demo';
import {
  CollectionInfo,
  DropPosition,
  EnvVar,
  FolderKind,
  GroupBy,
  OpenApiPreview,
  OpView,
  RequestDoc,
  SendResult,
  SyncDecisions,
  SyncPlan,
  SyncReport,
  SyncStatus,
  VariableInfo,
} from './model';

export interface Api {
  readonly demo: boolean;
  pickFolder(title?: string): Promise<string | null>;
  pickSpecFile(): Promise<string | null>;
  pickFile(defaultDir?: string): Promise<string | null>;
  inspectFolder(path: string): Promise<FolderKind>;
  createCollection(parent: string, name: string): Promise<string>;
  initCollection(dir: string, name: string): Promise<string>;
  openCollection(root: string): Promise<CollectionInfo>;
  createRequest(root: string, folder: string, name: string): Promise<string>;
  createFolder(root: string, parent: string, name: string): Promise<string>;
  renameItem(root: string, path: string, name: string): Promise<string>;
  cloneItem(root: string, path: string, name: string): Promise<string>;
  deleteItem(root: string, path: string): Promise<void>;
  moveItem(root: string, path: string, target: string, position: DropPosition): Promise<string>;
  readRequest(root: string, path: string): Promise<RequestDoc>;
  saveRequest(root: string, path: string, doc: RequestDoc): Promise<boolean>;
  readEnvironment(root: string, name: string): Promise<EnvVar[]>;
  variables(root: string, path: string, doc: RequestDoc, env: string | null): Promise<VariableInfo[]>;
  send(id: string, root: string, path: string, doc: RequestDoc, env: string | null): Promise<SendResult>;
  cancel(id: string): Promise<boolean>;
  parseCurl(command: string): Promise<RequestDoc | null>;
  createRequestFromCurl(root: string, folder: string, name: string, command: string): Promise<string>;
  previewOpenApi(source: string): Promise<OpenApiPreview>;
  importOpenApi(source: string, location: string, groupBy: GroupBy): Promise<string>;
  syncStatus(root: string): Promise<SyncStatus>;
  syncPlan(root: string, source: string | null, pairings: [string, string][]): Promise<SyncPlan>;
  syncOpView(planId: string, key: string, decisions: SyncDecisions): Promise<OpView>;
  syncApply(planId: string, decisions: SyncDecisions): Promise<SyncReport>;
}

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
  inspectFolder: (path) => invoke('inspect_folder', { path }),
  createCollection: (parent, name) => invoke('create_collection', { parent, name }),
  initCollection: (dir, name) => invoke('init_collection', { dir, name }),
  openCollection: (root) => invoke('open_collection', { root }),
  createRequest: (root, folder, name) => invoke('create_request', { root, folder, name }),
  createFolder: (root, parent, name) => invoke('create_folder', { root, parent, name }),
  renameItem: (root, path, name) => invoke('rename_item', { root, path, name }),
  cloneItem: (root, path, name) => invoke('clone_item', { root, path, name }),
  deleteItem: (root, path) => invoke('delete_item', { root, path }),
  moveItem: (root, path, target, position) => invoke('move_item', { root, path, target, position }),
  readRequest: (root, path) => invoke('read_request', { root, path }),
  saveRequest: (root, path, doc) => invoke('save_request', { root, path, doc }),
  readEnvironment: (root, name) => invoke('read_environment', { root, name }),
  variables: (root, path, doc, env) => invoke('variables', { root, path, doc, env }),
  send: (id, root, path, doc, env) => invoke('send_request', { args: { id, root, path, doc, env } }),
  cancel: (id) => invoke('cancel_request', { id }),
  parseCurl: (command) => invoke('parse_curl', { command }),
  createRequestFromCurl: (root, folder, name, command) => invoke('create_request_from_curl', { root, folder, name, command }),
  previewOpenApi: (source) => invoke('preview_openapi', { source }),
  importOpenApi: (source, location, groupBy) => invoke('import_openapi', { source, location, groupBy }),
  syncStatus: (root) => invoke('sync_status', { root }),
  syncPlan: (root, source, pairings) => invoke('sync_plan', { root, source, pairings }),
  syncOpView: (planId, key, decisions) => invoke('sync_op_view', { planId, key, decisions }),
  syncApply: (planId, decisions) => invoke('sync_apply', { planId, decisions }),
};

export const isTauri = '__TAURI_INTERNALS__' in window;
export const api: Api = isTauri ? tauriApi : demoApi;
