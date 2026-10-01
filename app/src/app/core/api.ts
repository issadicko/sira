import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

import { demoApi } from './demo';
import { CollectionInfo, EnvVar, RequestDoc, SendResult, VariableInfo } from './model';

export interface Api {
  readonly demo: boolean;
  pickFolder(): Promise<string | null>;
  openCollection(root: string): Promise<CollectionInfo>;
  readRequest(root: string, path: string): Promise<RequestDoc>;
  saveRequest(root: string, path: string, doc: RequestDoc): Promise<boolean>;
  readEnvironment(root: string, name: string): Promise<EnvVar[]>;
  variables(root: string, path: string, doc: RequestDoc, env: string | null): Promise<VariableInfo[]>;
  send(id: string, root: string, path: string, doc: RequestDoc, env: string | null): Promise<SendResult>;
  cancel(id: string): Promise<boolean>;
}

const tauriApi: Api = {
  demo: false,
  pickFolder: async () => {
    const picked = await open({ directory: true, multiple: false, title: 'Ouvrir une collection' });
    return typeof picked === 'string' ? picked : null;
  },
  openCollection: (root) => invoke('open_collection', { root }),
  readRequest: (root, path) => invoke('read_request', { root, path }),
  saveRequest: (root, path, doc) => invoke('save_request', { root, path, doc }),
  readEnvironment: (root, name) => invoke('read_environment', { root, name }),
  variables: (root, path, doc, env) => invoke('variables', { root, path, doc, env }),
  send: (id, root, path, doc, env) => invoke('send_request', { args: { id, root, path, doc, env } }),
  cancel: (id) => invoke('cancel_request', { id }),
};

export const isTauri = '__TAURI_INTERNALS__' in window;
export const api: Api = isTauri ? tauriApi : demoApi;
