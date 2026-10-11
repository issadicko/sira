import type { VaultPrefs } from './model';

export const sameVault = (a: VaultPrefs, b: VaultPrefs): boolean => JSON.stringify(a) === JSON.stringify(b);

/** Ce qui empêcherait d'enregistrer : une adresse sans `http://` ou `https://`. Même règle que l'application. */
export function vaultProblem(prefs: VaultPrefs): string | null {
  const addr = prefs.addr.trim();
  if (addr !== '' && !/^https?:\/\//.test(addr)) return "L'adresse de Vault commence par http:// ou https://.";
  return null;
}

/** La variable à écrire dans une requête pour lire `champ` du secret `chemin` (`montage/chemin`). */
export const vaultVariable = (secret: string, field: string): string => `{{$vault.${secret.replace(/^\/+|\/+$/g, '')}#${field.trim()}}}`;

/** Une durée de vie de jeton lisible : « 59 min », « 2 h », « 3 j ». */
export function ttlLabel(seconds: number | null): string {
  if (seconds === null) return 'sans expiration';
  if (seconds < 90) return `${seconds} s`;
  if (seconds < 5400) return `${Math.round(seconds / 60)} min`;
  if (seconds < 172800) return `${Math.round(seconds / 3600)} h`;
  return `${Math.round(seconds / 86400)} j`;
}
