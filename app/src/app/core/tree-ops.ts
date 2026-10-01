import type { DropPosition, TreeItem } from './model';

export type ItemKind = 'request' | 'folder';
export type NameKind = ItemKind | 'collection';

export interface Reorder {
  target: string;
  position: 'before' | 'after';
}

export interface KeyInput {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

export type TreeKey = 'rename' | 'clone' | 'delete' | 'up' | 'down' | 'menu';

const DEVICE_NAME = /^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])$/i;
const CONTROL = /[\u0000-\u001f\u007f]/;
const RESERVED_REQUESTS = ['collection', 'folder'];
const HIDDEN_FOLDERS = ['node_modules', 'opencollection.yml', 'folder.yml'];
const ROOT_FOLDERS = ['environments', 'mocks'];

/** Vrai si `path` est `ancestor` ou se trouve dessous (chemins relatifs en `/`). */
export function isUnder(path: string, ancestor: string): boolean {
  return path === ancestor || path.startsWith(`${ancestor}/`);
}

/** Chemin que prend `path` quand `from` devient `to` : valable pour un fichier comme pour un dossier (préfixe). */
export function remapPath(path: string, from: string, to: string): string {
  return path === from ? to : path.startsWith(`${from}/`) ? to + path.slice(from.length) : path;
}

export function remapPaths<T extends { path: string }>(items: T[], from: string, to: string): T[] {
  return items.map((item) => {
    const path = remapPath(item.path, from, to);
    return path === item.path ? item : { ...item, path };
  });
}

export function remapSet(paths: ReadonlySet<string>, from: string, to: string): Set<string> {
  return new Set([...paths].map((path) => remapPath(path, from, to)));
}

/** Onglets restants quand `path` disparaît, et onglet actif : le suivant, à défaut le dernier. */
export function closeUnder<T extends { path: string }>(tabs: T[], active: string | null, path: string): { tabs: T[]; active: string | null } {
  const rest = tabs.filter((t) => !isUnder(t.path, path));
  if (active === null || !isUnder(active, path)) return { tabs: rest, active };
  const before = tabs.slice(0, tabs.findIndex((t) => t.path === active)).filter((t) => !isUnder(t.path, path)).length;
  return { tabs: rest, active: rest[Math.min(before, rest.length - 1)]?.path ?? null };
}

/** Position de dépôt selon la zone survolée (`ratio` = hauteur survolée / hauteur de la ligne) : tiers haut et bas pour `before` et `after`, milieu d'un dossier pour `inside`. */
export function dropPosition(ratio: number, kind: ItemKind, open = false): DropPosition {
  if (kind === 'request') return ratio < 0.5 ? 'before' : 'after';
  if (ratio < 1 / 3) return 'before';
  return open || ratio < 2 / 3 ? 'inside' : 'after';
}

/** Un élément ne se dépose ni sur lui-même ni dans l'un de ses descendants ; `target` vaut `''` pour la racine. */
export function canDrop(dragged: string, target: string): boolean {
  return !isUnder(target, dragged);
}

/** Message d'erreur d'un nom refusé, `null` s'il convient ; le moteur reste la référence. */
export function validateName(raw: string, kind: NameKind, parent = ''): string | null {
  const name = raw.trim();
  const lower = name.toLowerCase();
  if (!name) return 'Donne un nom.';
  if (name.length > 255) return 'Le nom est trop long (255 caractères au plus).';
  if (CONTROL.test(name)) return 'Le nom contient un caractère invisible.';
  if (name.startsWith('-')) return 'Le nom ne peut pas commencer par un tiret.';
  if (DEVICE_NAME.test(name)) return `« ${name} » est un nom réservé par Windows.`;
  if (/^[.\s]+$/.test(name)) return 'Le nom doit contenir autre chose que des points.';
  if (kind === 'request' && RESERVED_REQUESTS.includes(lower)) return `« ${name} » est réservé aux fichiers de la collection.`;
  if (kind === 'folder' && (lower.startsWith('.') || HIDDEN_FOLDERS.includes(lower) || (parent === '' && ROOT_FOLDERS.includes(lower)))) {
    return `« ${name} » est un nom réservé : le dossier n'apparaîtrait pas dans l'arbre.`;
  }
  return null;
}

export function cloneName(name: string): string {
  return `${name} copie`;
}

export function findItem(items: TreeItem[], path: string): TreeItem | null {
  for (const item of items) {
    if (item.path === path) return item;
    if (item.kind === 'folder' && isUnder(path, item.path)) return findItem(item.children, path);
  }
  return null;
}

/** Liste qui contient `path` (frères compris), dans l'ordre de l'arbre. */
export function siblingsOf(items: TreeItem[], path: string): TreeItem[] {
  if (items.some((i) => i.path === path)) return items;
  for (const item of items) if (item.kind === 'folder' && isUnder(path, item.path)) return siblingsOf(item.children, path);
  return [];
}

/** Déplacement d'un cran vers le haut (-1) ou le bas (1) parmi les frères, `null` au bout de la liste. */
export function reorderMove(items: TreeItem[], path: string, direction: 1 | -1): Reorder | null {
  const siblings = siblingsOf(items, path);
  const index = siblings.findIndex((s) => s.path === path);
  const neighbour = index < 0 ? undefined : siblings[index + direction];
  return neighbour ? { target: neighbour.path, position: direction < 0 ? 'before' : 'after' } : null;
}

/** Élément à sélectionner quand `path` disparaît : le frère suivant, le précédent, à défaut le dossier parent. */
export function neighbourOf(items: TreeItem[], path: string): string | null {
  const siblings = siblingsOf(items, path);
  const index = siblings.findIndex((s) => s.path === path);
  const sibling = siblings[index + 1] ?? siblings[index - 1];
  return sibling?.path ?? (path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : null);
}

/** Requêtes et sous-dossiers contenus dans un élément (lui-même exclu). */
export function contentsOf(item: TreeItem): { requests: number; folders: number } {
  if (item.kind === 'request') return { requests: 0, folders: 0 };
  return item.children.reduce(
    (n, child) => {
      const inner = contentsOf(child);
      return child.kind === 'request'
        ? { requests: n.requests + 1, folders: n.folders }
        : { requests: n.requests + inner.requests, folders: n.folders + 1 + inner.folders };
    },
    { requests: 0, folders: 0 },
  );
}

/** Action de l'arbre liée à une touche : F2, ⌘D, ⌘⌫ ou Suppr, ⌥↑ ou ⌥↓, touche Menu ou Maj+F10. */
export function treeKeyAction(e: KeyInput, mac: boolean): TreeKey | null {
  const mod = mac ? e.metaKey : e.ctrlKey;
  const other = mac ? e.ctrlKey : e.metaKey;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (key === 'ContextMenu') return 'menu';
  if (key === 'F10') return e.shiftKey && !mod && !other && !e.altKey ? 'menu' : null;
  if (other || e.shiftKey) return null;
  if (e.altKey) return !mod && key === 'ArrowUp' ? 'up' : !mod && key === 'ArrowDown' ? 'down' : null;
  if (key === 'F2') return mod ? null : 'rename';
  if (key === 'd') return mod ? 'clone' : null;
  if (key === 'Delete') return mod ? null : 'delete';
  if (key === 'Backspace') return mod ? 'delete' : null;
  return null;
}

/** « 6 requêtes et 1 sous-dossier » ; chaîne vide pour un dossier vide. */
export function describeContents({ requests, folders }: { requests: number; folders: number }): string {
  const parts = [
    requests ? `${requests} ${requests > 1 ? 'requêtes' : 'requête'}` : '',
    folders ? `${folders} ${folders > 1 ? 'sous-dossiers' : 'sous-dossier'}` : '',
  ];
  return parts.filter(Boolean).join(' et ');
}

/** Avertissement de la confirmation de suppression quand des onglets contiennent des modifications non enregistrées. */
export function unsavedMessage(names: string[]): string {
  if (names.length === 1) return `L'onglet « ${names[0]} » contient des modifications non enregistrées : elles seront perdues.`;
  return `${names.length} onglets contiennent des modifications non enregistrées (${names.join(', ')}) : elles seront perdues.`;
}
