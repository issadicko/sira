import type { TreeItem } from './model';

export interface FolderChoice {
  path: string;
  label: string;
  depth: number;
}

/** Dossiers de la collection dans l'ordre de l'arbre, avec leur profondeur. */
export function folderChoices(items: TreeItem[], depth = 0): FolderChoice[] {
  return items.flatMap((i) =>
    i.kind === 'folder' ? [{ path: i.path, label: i.name, depth }, ...folderChoices(i.children, depth + 1)] : [],
  );
}

/** Dossier parent d'un chemin relatif en `/` ; chaîne vide à la racine. */
export function dirname(path: string): string {
  const i = path.lastIndexOf('/');
  return i < 0 ? '' : path.slice(0, i);
}

/** Chemin d'un dossier à créer sous `parent`, avec le séparateur que `parent` utilise. */
export function joinPath(parent: string, name: string): string {
  const separator = parent.includes('\\') && !parent.includes('/') ? '\\' : '/';
  return `${parent.replace(/[/\\]+$/, '')}${separator}${name}`;
}

function parts(path: string): string[] {
  const out: string[] = [];
  for (const part of path.replace(/\\/g, '/').split('/')) {
    if (part === '..') out.pop();
    else if (part && part !== '.') out.push(part);
  }
  return out;
}

/** Chemin de `file` relatif à `root` en `/`, ou `null` si le fichier n'est pas dans la collection. */
export function relativeToRoot(root: string, file: string): string | null {
  const base = parts(root);
  const target = parts(file);
  const same = /^[A-Za-z]:/.test(root) ? (a: string, b: string) => a.toLowerCase() === b.toLowerCase() : (a: string, b: string) => a === b;
  const inside = target.length > base.length && base.every((part, i) => same(part, target[i]));
  return inside ? target.slice(base.length).join('/') : null;
}

/** Vrai si le moteur acceptera ce chemin de fichier : relatif, sans remontée `..`. */
export function isInsideCollection(path: string): boolean {
  return !/^([/\\]|[A-Za-z]:)/.test(path) && !path.split(/[/\\]/).includes('..');
}
