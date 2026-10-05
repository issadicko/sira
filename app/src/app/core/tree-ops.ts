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

/** `up` et `down` réordonnent (⌥↑ et ⌥↓) ; les autres touches de déplacement se lisent avec `navigate`. */
export type TreeKey = 'rename' | 'clone' | 'delete' | 'up' | 'down' | 'menu' | 'next' | 'prev' | 'first' | 'last' | 'expand' | 'collapse' | 'activate';

/** Ligne affichée de l'arbre, dans l'ordre de l'écran ; `open` vaut vrai pour un dossier dont les enfants suivent. */
export interface VisibleRow {
  path: string;
  kind: ItemKind;
  depth: number;
  parent: string;
  open: boolean;
}

export type TreeStep = { focus: string } | { toggle: string } | { activate: string };

export interface Box {
  top: number;
  height: number;
}

export interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

const DEVICE_NAME = /^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])$/i;
const CONTROL = /[\u0000-\u001f\u007f]/;
const RESERVED_REQUESTS = ['collection', 'folder'];
const HIDDEN_FOLDERS = ['node_modules', 'opencollection.yml', 'folder.yml'];
const ROOT_FOLDERS = ['environments', 'mocks'];
const NAVIGATION = new Map<string, TreeKey>([
  ['ArrowDown', 'next'],
  ['ArrowUp', 'prev'],
  ['Home', 'first'],
  ['End', 'last'],
  ['ArrowRight', 'expand'],
  ['ArrowLeft', 'collapse'],
  ['Enter', 'activate'],
  [' ', 'activate'],
]);

/** Dossier parent d'un chemin relatif en `/` ; chaîne vide à la racine. */
const parentOf = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf('/')));

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

/**
 * Position de dépôt selon la zone survolée (`ratio` = hauteur survolée / hauteur de la ligne) : tiers haut et bas pour `before` et `after`,
 * milieu d'un dossier pour `inside`. Sans `reorderable` (filtre actif, voisins masqués), seul `inside` d'un dossier reste possible.
 */
export function dropPosition(ratio: number, kind: ItemKind, open = false, reorderable = true): DropPosition | null {
  if (!reorderable) return kind === 'folder' ? 'inside' : null;
  if (kind === 'request') return ratio < 0.5 ? 'before' : 'after';
  if (ratio < 1 / 3) return 'before';
  return open || ratio < 2 / 3 ? 'inside' : 'after';
}

/** Position de dépôt au point d'ordonnée `y` sur la ligne `box` : calculée à partir des coordonnées de l'événement, jamais d'un survol antérieur. */
export function dropPositionAt(y: number, box: Box, kind: ItemKind, open = false, reorderable = true): DropPosition | null {
  const ratio = box.height > 0 ? Math.min(1, Math.max(0, (y - box.top) / box.height)) : 0.5;
  return dropPosition(ratio, kind, open, reorderable);
}

/** Vrai si le point est dans le rectangle ; sert à décider qu'un glisser a quitté une zone, `relatedTarget` n'étant pas fiable sous WebKit. */
export function insideRect(x: number, y: number, rect: Rect): boolean {
  return x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
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

/** Nom de fichier ou de dossier tiré d'un nom affiché, comme `sanitize_name` du moteur : caractères interdits en `-`, ni espace ni `-` en tête, ni point ni espace en queue. */
export function sanitizeName(name: string): string {
  return name
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, '-')
    .replace(/^[\s-]+/, '')
    .replace(/[.\s]+$/, '');
}

/** `seq` d'un élément placé en fin de liste : au-delà du plus grand `seq` et du nombre de frères. */
export function endSeq(seqs: number[]): number {
  return Math.max(seqs.length, ...seqs) + 1;
}

export function cloneName(name: string): string {
  return `${name} copie`;
}

/** Nom que prendrait le fichier d'un environnement : le nom assaini, avec ` 1`, ` 2`… s'il est pris (la casse ne distingue pas deux noms). */
export function envFileName(raw: string, taken: string[], except?: string): string {
  const stem = sanitizeName(raw.trim()) || 'Environnement';
  const used = new Set(taken.filter((name) => name !== except).map((name) => name.toLowerCase()));
  let name = stem;
  for (let n = 1; used.has(name.toLowerCase()); n++) name = `${stem} ${n}`;
  return name;
}

export function findItem(items: TreeItem[], path: string): TreeItem | null {
  for (const item of items) {
    if (item.path === path) return item;
    if (item.kind === 'folder' && isUnder(path, item.path)) return findItem(item.children, path);
  }
  return null;
}

/** Texte du filtre sans casse ni accents. */
export function normalizeQuery(raw: string): string {
  return raw.trim().toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '');
}

/** Vrai si l'élément passe le filtre : une requête par son nom ou son chemin, un dossier par un de ses descendants. */
export function matchesQuery(item: TreeItem, query: string): boolean {
  if (!query) return true;
  if (item.kind === 'folder') return item.children.some((c) => matchesQuery(c, query));
  return normalizeQuery(`${item.name} ${item.path}`).includes(query);
}

/** Lignes affichées, dans l'ordre de l'écran : un filtre actif ouvre tous les dossiers qui le satisfont. */
export function visibleRows(items: TreeItem[], open: ReadonlySet<string>, query: string, depth = 0, parent = ''): VisibleRow[] {
  return items
    .filter((item) => matchesQuery(item, query))
    .flatMap((item) => {
      const expanded = item.kind === 'folder' && (query !== '' || open.has(item.path));
      const row: VisibleRow = { path: item.path, kind: item.kind, depth, parent, open: expanded };
      return item.kind === 'folder' && expanded ? [row, ...visibleRows(item.children, open, query, depth + 1, item.path)] : [row];
    });
}

/** Ligne qui reçoit le focus quand on entre dans l'arbre au clavier : la sélection si elle est affichée, sinon la première ligne. */
export function tabbablePath(rows: VisibleRow[], selected: string | null): string | null {
  return rows.some((r) => r.path === selected) ? selected : (rows[0]?.path ?? null);
}

/**
 * Effet d'une touche de déplacement depuis la ligne `path` : ↑ ↓ Début Fin parcourent les lignes affichées, → ouvre un dossier fermé ou va à son
 * premier enfant, ← ferme un dossier ouvert ou va au parent, Entrée et Espace activent. `filtered` : le filtre garde tous les dossiers ouverts.
 */
export function navigate(rows: VisibleRow[], path: string, key: TreeKey, filtered = false): TreeStep | null {
  const index = rows.findIndex((r) => r.path === path);
  const row = rows[index];
  if (!row) return null;
  const focus = (to: VisibleRow | undefined): TreeStep | null => (to ? { focus: to.path } : null);
  switch (key) {
    case 'next':
      return focus(rows[index + 1]);
    case 'prev':
      return focus(rows[index - 1]);
    case 'first':
      return focus(rows[0]);
    case 'last':
      return focus(rows[rows.length - 1]);
    case 'expand':
      if (row.kind !== 'folder') return null;
      return row.open ? focus(rows[index + 1]?.parent === path ? rows[index + 1] : undefined) : { toggle: path };
    case 'collapse':
      return row.kind === 'folder' && row.open && !filtered ? { toggle: path } : focus(rows.find((r) => r.path === row.parent));
    case 'activate':
      return { activate: path };
    default:
      return null;
  }
}

/** Chemins de `paths` qui ne sont plus dans l'arbre. */
export function missingPaths(items: TreeItem[], paths: string[]): string[] {
  return paths.filter((path) => !findItem(items, path));
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

/** Message quand ⌥↑ ou ⌥↓ n'a nulle part où aller. */
export function reorderNotice(direction: 1 | -1): string {
  return direction < 0 ? 'Déjà en première position' : 'Déjà en dernière position';
}

/** Vrai si déposer `path` là ne change rien : juste avant son frère suivant, juste après son frère précédent, ou en fin de son propre dossier. */
export function isNoopMove(items: TreeItem[], path: string, target: string, position: DropPosition): boolean {
  const siblings = siblingsOf(items, path);
  const index = siblings.findIndex((s) => s.path === path);
  if (index < 0) return false;
  if (position === 'inside') return target === parentOf(path) && index === siblings.length - 1;
  if (target === path) return true;
  return siblings[index + (position === 'before' ? 1 : -1)]?.path === target;
}

/** Élément à sélectionner quand `path` disparaît : le frère suivant, le précédent, à défaut le dossier parent. */
export function neighbourOf(items: TreeItem[], path: string): string | null {
  const siblings = siblingsOf(items, path);
  const index = siblings.findIndex((s) => s.path === path);
  const sibling = siblings[index + 1] ?? siblings[index - 1];
  return sibling?.path ?? (path.includes('/') ? parentOf(path) : null);
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

/** Action de l'arbre liée à une touche : F2, ⌘D, ⌘⌫ ou Suppr, ⌥↑ ou ⌥↓, touche Menu ou Maj+F10, flèches, Début, Fin, Entrée et Espace. */
export function treeKeyAction(e: KeyInput, mac: boolean): TreeKey | null {
  const mod = mac ? e.metaKey : e.ctrlKey;
  const other = mac ? e.ctrlKey : e.metaKey;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (key === 'ContextMenu') return 'menu';
  if (key === 'F10') return e.shiftKey && !mod && !other && !e.altKey ? 'menu' : null;
  if (other || e.shiftKey) return null;
  if (e.altKey) return !mod && key === 'ArrowUp' ? 'up' : !mod && key === 'ArrowDown' ? 'down' : null;
  if (!mod && NAVIGATION.has(key)) return NAVIGATION.get(key) ?? null;
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
