import type { NetworkPrefs, ProxyConfig, ProxyMode } from './model';

export const PROXY_PROTOCOLS = ['http', 'https', 'socks4', 'socks5'] as const;

export const DEFAULT_PROXY: ProxyConfig = { protocol: 'http', hostname: '', port: '', username: '', authDisabled: false, bypassProxy: '' };

export const DEFAULT_NETWORK: NetworkPrefs = {
  verifyTls: true,
  sendCookies: true,
  storeCookies: true,
  caFile: null,
  keepDefaultRoots: true,
  clientCertificates: [],
  proxy: { mode: 'system', config: DEFAULT_PROXY },
};

export const PROXY_MODES: { id: ProxyMode; label: string }[] = [
  { id: 'off', label: 'Aucun' },
  { id: 'system', label: 'Système' },
  { id: 'manual', label: 'Manuel' },
];

/** Ce qui ne va pas dans un port saisi (vide : le port du protocole), `null` quand il convient. */
export function portProblem(text: string): string | null {
  const port = text.trim();
  if (port === '') return null;
  return /^\d{1,5}$/.test(port) && Number(port) >= 1 && Number(port) <= 65535 ? null : 'Le port va de 1 à 65535.';
}

/** Ce qui empêcherait d'enregistrer : un proxy manuel sans hôte, ou avec un port invalide. Même règle que l'application. */
export function proxyProblem(prefs: NetworkPrefs): string | null {
  const { mode, config } = prefs.proxy;
  if (mode !== 'manual') return null;
  if (config.hostname.trim() === '') return 'Le proxy manuel demande un nom d’hôte.';
  return portProblem(config.port);
}

export const withProxyMode = (prefs: NetworkPrefs, mode: ProxyMode): NetworkPrefs => ({ ...prefs, proxy: { ...prefs.proxy, mode } });

export const withProxy = (prefs: NetworkPrefs, patch: Partial<ProxyConfig>): NetworkPrefs => ({
  ...prefs,
  proxy: { ...prefs.proxy, config: { ...prefs.proxy.config, ...patch } },
});

export const sameNetwork = (a: NetworkPrefs, b: NetworkPrefs): boolean => JSON.stringify(a) === JSON.stringify(b);

/** Le dernier élément d'un chemin, pour l'afficher sans le dossier. */
export const fileName = (path: string): string => path.split(/[\\/]/).filter(Boolean).pop() ?? path;
