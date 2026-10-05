import type { Api } from './api';
import { CollectionInfo, EnvVar, FolderKind, RequestDoc, TreeItem } from './model';
import { dirname, joinPath } from './paths';
import { canDrop, endSeq, findItem, sanitizeName, validateName } from './tree-ops';

export interface DemoCollection {
  info: CollectionInfo;
  files: Record<string, RequestDoc>;
  environments: Record<string, EnvVar[]>;
}

type DemoTree = Pick<
  Api,
  'inspectFolder' | 'createCollection' | 'initCollection' | 'createRequest' | 'createFolder' | 'renameItem' | 'cloneItem' | 'deleteItem' | 'moveItem'
>;

/** Dossiers que le sélecteur simulé propose en plus de la collection de démo ; tout autre dossier est vide. */
export const DEMO_FOLDERS: Record<string, FolderKind> = {
  '~/démo/nouveau-projet': 'empty',
  '~/démo/notes': 'other',
  '~/démo/ancienne-collection': 'bru',
};

const EXT = '.yml';

const basename = (path: string) => path.slice(path.lastIndexOf('/') + 1);
const join = (folder: string, name: string) => (folder ? `${folder}/${name}` : name);
const extOf = (item: TreeItem) => (item.kind === 'request' ? EXT : '');
const countRequests = (items: TreeItem[]): number => items.reduce((n, i) => n + (i.kind === 'request' ? 1 : countRequests(i.children)), 0);

const blank = (name: string, seq: number): RequestDoc => ({
  name,
  requestType: 'http',
  seq,
  method: 'GET',
  url: '',
  params: [],
  headers: [],
  body: { type: 'none' },
  auth: { type: 'inherit' },
  assertions: [],
  variables: [],
  scripts: [],
  docs: null,
  timeoutMs: null,
});

export function createDemoCollection(root: string, name: string): DemoCollection {
  return { info: { root, name, items: [], environments: [], defaultEnvironment: null, requestCount: 0 }, files: {}, environments: {} };
}

export function collectionAt(collections: Map<string, DemoCollection>, root: string): DemoCollection {
  const found = collections.get(root);
  if (!found) throw `Collection introuvable en mode démo : ${root}. Les collections créées ne survivent pas au rechargement de la page.`;
  return found;
}

/** Reporte dans l'arbre le nom, la méthode et l'URL d'une requête enregistrée, comme la relecture du dossier. */
export function refreshItem(c: DemoCollection, path: string, doc: RequestDoc) {
  const item = findItem(c.info.items, path);
  if (item?.kind === 'request') Object.assign(item, { name: doc.name, method: doc.method, url: doc.url });
}

function locate(c: DemoCollection, path: string): TreeItem {
  const item = findItem(c.info.items, path);
  if (!item) throw `Introuvable : ${path}`;
  return item;
}

function childrenOf(c: DemoCollection, folder: string): TreeItem[] {
  if (folder === '') return c.info.items;
  const item = locate(c, folder);
  if (item.kind !== 'folder') throw `${folder} n'est pas un dossier`;
  return item.children;
}

function uniqueName(list: TreeItem[], stem: string, ext: string, except?: TreeItem): string {
  const used = new Set(list.filter((i) => i !== except).map((i) => basename(i.path).toLowerCase()));
  let name = `${stem}${ext}`;
  for (let n = 1; used.has(name.toLowerCase()); n++) name = `${stem} ${n}${ext}`;
  return name;
}

const seqOf = (c: DemoCollection, item: TreeItem) => item.seq ?? (item.kind === 'request' ? c.files[item.path]?.seq : null) ?? 0;
const nextSeq = (c: DemoCollection, list: TreeItem[]) => endSeq(list.map((i) => seqOf(c, i)));

function setSeq(c: DemoCollection, item: TreeItem, seq: number) {
  item.seq = seq;
  if (item.kind === 'request' && c.files[item.path]) c.files[item.path].seq = seq;
}

function relocate(c: DemoCollection, item: TreeItem, path: string) {
  const old = item.path;
  if (item.kind === 'request') {
    c.files[path] = c.files[old];
    if (path !== old) delete c.files[old];
  } else {
    for (const child of item.children) relocate(c, child, join(path, basename(child.path)));
  }
  item.path = path;
}

function duplicate(c: DemoCollection, item: TreeItem, path: string, name: string): TreeItem {
  if (item.kind === 'request') {
    c.files[path] = { ...structuredClone(c.files[item.path]), name };
    return { ...item, path, name };
  }
  return { ...item, path, name, children: item.children.map((child) => duplicate(c, child, join(path, basename(child.path)), child.name)) };
}

function forget(c: DemoCollection, item: TreeItem) {
  if (item.kind === 'request') delete c.files[item.path];
  else for (const child of item.children) forget(c, child);
}

function checked(name: string, kind: 'request' | 'folder' | 'collection', parent = ''): string {
  const problem = validateName(name, kind, parent);
  if (problem) throw problem;
  return name.trim();
}

/**
 * Opérations de collection du mode démo : appliquées pour de bon, mais à la collection gardée en mémoire, avec les règles de nommage et de `seq` du moteur.
 * Le suivi OpenAPI (`.oc-sync`) n'est pas simulé : renommer, déplacer ou supprimer ne le met pas à jour.
 */
export function createDemoTree(collections: Map<string, DemoCollection>): DemoTree {
  const at = (root: string) => collectionAt(collections, root);

  const created = (c: DemoCollection, list: TreeItem[], item: TreeItem) => {
    list.push(item);
    c.info.requestCount = countRequests(c.info.items);
    return item.path;
  };

  const register = (root: string, name: string) => {
    collections.set(root, createDemoCollection(root, name));
    return root;
  };

  return {
    inspectFolder: async (path) => (collections.has(path) ? 'collection' : DEMO_FOLDERS[path] ?? 'empty'),

    createCollection: async (parent, name) => {
      const title = checked(name, 'collection');
      const stem = sanitizeName(title);
      let root = joinPath(parent, stem);
      for (let n = 1; collections.has(root); n++) root = joinPath(parent, `${stem} ${n}`);
      return register(root, title);
    },

    initCollection: async (dir, name) => {
      const title = checked(name, 'collection');
      if (collections.has(dir)) throw 'opencollection.yml existe déjà dans ce dossier.';
      return register(dir, title);
    },

    createRequest: async (root, folder, name) => {
      const c = at(root);
      const title = checked(name, 'request');
      const list = childrenOf(c, folder);
      const path = join(folder, uniqueName(list, sanitizeName(title), EXT));
      const seq = list.length + 1;
      c.files[path] = blank(title, seq);
      return created(c, list, { kind: 'request', path, name: title, seq, method: 'GET', requestType: 'http', url: '', deprecated: false });
    },

    createFolder: async (root, parent, name) => {
      const c = at(root);
      const title = checked(name, 'folder', parent);
      const list = childrenOf(c, parent);
      const path = join(parent, uniqueName(list, sanitizeName(title), ''));
      return created(c, list, { kind: 'folder', path, name: title, seq: list.length + 1, children: [] });
    },

    renameItem: async (root, path, name) => {
      const c = at(root);
      const item = locate(c, path);
      const parent = dirname(path);
      const title = checked(name, item.kind, parent);
      const file = uniqueName(childrenOf(c, parent), sanitizeName(title), extOf(item), item);
      item.name = title;
      if (item.kind === 'request') c.files[path].name = title;
      const next = join(parent, file);
      relocate(c, item, next);
      return next;
    },

    cloneItem: async (root, path, name) => {
      const c = at(root);
      const item = locate(c, path);
      const parent = dirname(path);
      const title = checked(name, item.kind, parent);
      const list = childrenOf(c, parent);
      const next = join(parent, uniqueName(list, sanitizeName(title), extOf(item)));
      const copy = duplicate(c, item, next, title);
      setSeq(c, copy, nextSeq(c, list));
      return created(c, list, copy);
    },

    deleteItem: async (root, path) => {
      const c = at(root);
      const item = locate(c, path);
      const list = childrenOf(c, dirname(path));
      list.splice(list.indexOf(item), 1);
      forget(c, item);
      c.info.requestCount = countRequests(c.info.items);
    },

    moveItem: async (root, path, target, position) => {
      const c = at(root);
      const item = locate(c, path);
      if (!canDrop(path, target)) throw "Un dossier ne peut pas être déplacé dans lui-même ni dans l'un de ses descendants.";
      const from = childrenOf(c, dirname(path));
      const folder = position === 'inside' ? target : dirname(target);
      const to = childrenOf(c, folder);
      from.splice(from.indexOf(item), 1);
      const sibling = to.findIndex((i) => i.path === target);
      const index = position === 'inside' ? to.length : position === 'before' ? sibling : sibling + 1;
      const ext = extOf(item);
      const name = basename(path);
      const next = join(folder, uniqueName(to, name.slice(0, name.length - ext.length), ext));
      const seq = nextSeq(c, to);
      to.splice(index, 0, item);
      relocate(c, item, next);
      if (position === 'inside') setSeq(c, item, seq);
      else to.forEach((other, i) => setSeq(c, other, i + 1));
      return next;
    },
  };
}
