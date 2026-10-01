import { InjectionToken, signal } from '@angular/core';

/** Action nommée, proposée dans la palette et, si elle a un raccourci, au clavier. */
export interface Command {
  id: string;
  title: string;
  group: string;
  icon: string;
  /** Raccourci au format `mod+shift+p` ; `mod` vaut ⌘ sur macOS et Ctrl ailleurs. */
  keys?: string;
  when?: () => boolean;
  run: () => unknown;
}

export type KeyLike = Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey'>;

export const isMac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac');

const LABELS: Record<string, [mac: string, other: string]> = {
  mod: ['⌘', 'Ctrl'],
  shift: ['⇧', 'Maj'],
  alt: ['⌥', 'Alt'],
  enter: ['↵', '↵'],
  esc: ['Échap', 'Échap'],
};

const KEYS: Record<string, string> = { esc: 'escape' };

/** Libellé d'un raccourci : ⌘⇧P sur macOS, Ctrl+Maj+P ailleurs. */
export function shortcutLabel(keys: string, mac = isMac): string {
  return keys
    .split('+')
    .map((k) => LABELS[k]?.[mac ? 0 : 1] ?? k.toUpperCase())
    .join(mac ? '' : '+');
}

/** Vrai si la touche correspond au raccourci ; Maj et Alt comptent sauf pour un symbole (`\`, `/`…). */
export function matchesShortcut(keys: string, event: KeyLike, mac = isMac): boolean {
  const parts = keys.split('+');
  const key = parts[parts.length - 1];
  const symbol = !/^([a-z0-9]|enter|esc)$/.test(key);
  return (
    (mac ? event.metaKey : event.ctrlKey) === parts.includes('mod') &&
    (symbol || (event.shiftKey === parts.includes('shift') && event.altKey === parts.includes('alt'))) &&
    event.key.toLowerCase() === (KEYS[key] ?? key)
  );
}

/** Registre des commandes de l'application. */
export class Commands {
  private readonly list = signal<Command[]>([]);
  readonly all = this.list.asReadonly();

  /** Ajoute ou remplace des commandes (par `id`) ; renvoie de quoi les retirer. */
  register(...commands: Command[]): () => void {
    const ids = new Set(commands.map((c) => c.id));
    this.list.update((list) => [...list.filter((c) => !ids.has(c.id)), ...commands]);
    return () => this.list.update((list) => list.filter((c) => !commands.includes(c)));
  }

  enabled(command: Command): boolean {
    return command.when?.() ?? true;
  }

  /** Exécute la commande active liée à la touche ; la touche n'est consommée que dans ce cas. */
  dispatch(event: KeyLike & Pick<Event, 'preventDefault'>, mac = isMac): boolean {
    const command = this.list().find(
      (c) => c.keys && matchesShortcut(c.keys, event, mac) && this.enabled(c),
    );
    if (!command) return false;
    event.preventDefault();
    command.run();
    return true;
  }
}

export const COMMANDS = new InjectionToken<Commands>('Commandes', {
  providedIn: 'root',
  factory: () => new Commands(),
});
