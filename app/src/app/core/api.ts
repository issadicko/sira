import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

import { demoApi } from './demo';
import { CollectionInfo, EnvVar, GroupBy, OpenApiPreview, RequestDoc, SendResult, VariableInfo } from './model';

export interface Api {
  readonly demo: boolean;
  pickFolder(title?: string): Promise<string | null>;
  pickSpecFile(): Promise<string | null>;
  pickFile(defaultDir?: string): Promise<string | null>;
  openCollection(root: string): Promise<CollectionInfo>;
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
}

async function pick(options: Parameters<typeof open>[0]): Promise<string | null> {
  const picked = await open({ ...options, multiple: false });
  return typeof picked === 'string' ? picked : null;
}

const tauriApi: Api = {
  demo: false,
  pickFolder: (title = 'Ouvrir une collection') => pick({ directory: true, title }),
  pickSpecFile: () =>
    pick({ title: 'Choisir une spécification OpenAPI', filters: [{ name: 'OpenAPI et Swagger', extensions: ['yaml', 'yml', 'json'] }] }),
  pickFile: (defaultDir) => pick({ title: 'Choisir un fichier à envoyer', defaultPath: defaultDir }),
  openCollection: (root) => invoke('open_collection', { root }),
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
};

export const isTauri = '__TAURI_INTERNALS__' in window;
export const api: Api = isTauri ? tauriApi : demoApi;
