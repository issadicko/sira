import type { Auth, OAuth2Auth, OAuthParam, TokenInfo } from './model';

export interface AuthChoice {
  type: Auth['type'];
  label: string;
}

/** Les types d'auth que l'on peut choisir ; les autres (`other`) sont conservés tels quels dans le fichier. */
export const AUTH_TYPES: AuthChoice[] = [
  { type: 'inherit', label: 'Hériter du dossier ou de la collection' },
  { type: 'none', label: 'Aucune' },
  { type: 'bearer', label: 'Bearer Token' },
  { type: 'basic', label: 'Basic' },
  { type: 'apikey', label: 'API Key' },
  { type: 'digest', label: 'Digest' },
  { type: 'oauth2', label: 'OAuth 2.0' },
  { type: 'awsv4', label: 'AWS Signature V4' },
];

export const OAUTH_FLOWS = [
  { value: 'client_credentials', label: 'Client credentials' },
  { value: 'resource_owner_password_credentials', label: 'Mot de passe (password)' },
  { value: 'authorization_code', label: "Code d'autorisation" },
  { value: 'implicit', label: 'Implicite' },
];

/** Un type d'auth neuf, avec les valeurs que l'on attend d'abord. */
export function defaultAuth(type: Auth['type']): Auth {
  switch (type) {
    case 'bearer':
      return { type, token: '{{token}}' };
    case 'basic':
    case 'digest':
      return { type, username: '', password: '' };
    case 'apikey':
      return { type, key: 'X-API-Key', value: '', placement: 'header' };
    case 'awsv4':
      return { type, accessKeyId: '', secretAccessKey: '', sessionToken: '', service: '', region: '', profileName: '' };
    case 'oauth2':
      return defaultOAuth2();
    case 'none':
      return { type };
    default:
      return { type: 'inherit' };
  }
}

export function defaultOAuth2(): OAuth2Auth {
  return {
    type: 'oauth2',
    flow: 'client_credentials',
    authorizationUrl: '',
    accessTokenUrl: '',
    refreshTokenUrl: '',
    callbackUrl: '',
    clientId: '',
    clientSecret: '',
    credentialsPlacement: 'basic_auth_header',
    username: '',
    password: '',
    scope: '',
    state: '',
    pkce: false,
    tokenId: 'credentials',
    tokenPlacement: 'header',
    tokenPrefix: 'Bearer',
    tokenQueryKey: 'access_token',
    tokenSource: 'access_token',
    autoFetchToken: true,
    autoRefreshToken: true,
    parameters: [],
  };
}

/** L'auth avec le champ `field` remplacé ; sans effet sur un type qui n'a pas ce champ. */
export function withAuthField(auth: Auth, field: string, value: string | boolean): Auth {
  return field in auth ? ({ ...auth, [field]: value } as Auth) : auth;
}

/** Le libellé d'un type d'auth, pour l'onglet et les bandeaux. */
export function authLabel(auth: Auth): string {
  if (auth.type === 'other') return auth.label;
  return AUTH_TYPES.find((a) => a.type === auth.type)?.label ?? auth.type;
}

/** Les champs qu'un flux OAuth 2 utilise ; les autres ne sont ni montrés ni écrits dans le fichier. */
export interface OAuthFields {
  authorizationUrl: boolean;
  accessTokenUrl: boolean;
  callbackUrl: boolean;
  clientSecret: boolean;
  owner: boolean;
  pkce: boolean;
  state: boolean;
}

export function oauthFields(flow: string): OAuthFields {
  const interactive = flow === 'authorization_code' || flow === 'implicit';
  return {
    authorizationUrl: interactive,
    accessTokenUrl: flow !== 'implicit',
    callbackUrl: interactive,
    clientSecret: flow !== 'implicit',
    owner: flow === 'resource_owner_password_credentials',
    pkce: flow === 'authorization_code',
    state: interactive,
  };
}

export function addOAuthParam(auth: OAuth2Auth, stage: OAuthParam['stage'] = 'token'): OAuth2Auth {
  return { ...auth, parameters: [...auth.parameters, { stage, name: '', value: '', placement: 'body' }] };
}

export function patchOAuthParam(auth: OAuth2Auth, index: number, change: Partial<OAuthParam>): OAuth2Auth {
  return { ...auth, parameters: auth.parameters.map((p, i) => (i === index ? { ...p, ...change } : p)) };
}

export function removeOAuthParam(auth: OAuth2Auth, index: number): OAuth2Auth {
  return { ...auth, parameters: auth.parameters.filter((_, i) => i !== index) };
}

/** « 45 s », « 12 min », « 3 h 20 », « 2 j » : la durée restante, arrondie vers le bas. */
export function formatSpan(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s} s`;
  const min = Math.floor(s / 60);
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  if (h < 24) return min % 60 ? `${h} h ${String(min % 60).padStart(2, '0')}` : `${h} h`;
  return `${Math.floor(h / 24)} j`;
}

export interface TokenState {
  tone: 'none' | 'ok' | 'expired';
  text: string;
}

/** Ce que l'éditeur dit du jeton gardé, à l'instant `now` (millisecondes Unix). */
export function describeToken(info: TokenInfo | null, now: number): TokenState {
  if (!info) return { tone: 'none', text: "Aucun jeton pour l'instant. Il est demandé à l'envoi, ou ici." };
  const expired = info.expired || (info.expiresAt !== null && info.expiresAt <= now);
  if (expired) {
    return { tone: 'expired', text: info.hasRefreshToken ? 'Jeton expiré, un jeton de rafraîchissement est gardé.' : 'Jeton expiré.' };
  }
  const scope = info.scope ? ` · ${info.scope}` : '';
  if (info.expiresAt === null) return { tone: 'ok', text: `Jeton valide, sans durée annoncée par le serveur${scope}` };
  return { tone: 'ok', text: `Jeton valide, expire dans ${formatSpan(info.expiresAt - now)}${scope}` };
}
