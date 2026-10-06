export type ParamKind = 'query' | 'path';

export type FolderKind = 'collection' | 'empty' | 'bru' | 'other';
export type DropPosition = 'before' | 'after' | 'inside';

export interface KeyValue {
  name: string;
  value: string;
  enabled: boolean;
  description?: string | null;
}

export interface Param extends KeyValue {
  kind: ParamKind;
}

export type MultipartField = {
  name: string;
  enabled: boolean;
  contentType?: string | null;
  description?: string | null;
} & ({ kind: 'text'; value: string } | { kind: 'file'; value: string[] });

export type Body =
  | { type: 'none' }
  | { type: 'json' | 'text' | 'xml'; data: string }
  | { type: 'form-urlencoded'; fields: KeyValue[] }
  | { type: 'multipart-form'; fields: MultipartField[] }
  | { type: 'other'; label: string; config?: string };

/** Un paramètre ajouté à une des requêtes du flux OAuth 2. */
export interface OAuthParam {
  stage: 'authorization' | 'token' | 'refresh';
  name: string;
  value: string;
  placement: 'header' | 'query' | 'body';
}

export interface OAuth2Auth {
  type: 'oauth2';
  flow: string;
  authorizationUrl: string;
  accessTokenUrl: string;
  refreshTokenUrl: string;
  callbackUrl: string;
  clientId: string;
  clientSecret: string;
  credentialsPlacement: string;
  username: string;
  password: string;
  scope: string;
  state: string;
  pkce: boolean;
  tokenId: string;
  tokenPlacement: string;
  tokenPrefix: string;
  tokenQueryKey: string;
  tokenSource: string;
  autoFetchToken: boolean;
  autoRefreshToken: boolean;
  parameters: OAuthParam[];
}

/** Le jeton OAuth 2 gardé pour une requête ; sa valeur ne quitte jamais le processus de l'application. */
export interface TokenInfo {
  id: string;
  tokenType: string | null;
  scope: string | null;
  /** Millisecondes Unix ; `null` : le serveur n'a pas annoncé de durée. */
  expiresAt: number | null;
  expired: boolean;
  hasRefreshToken: boolean;
}

export type Auth =
  | { type: 'inherit' }
  | { type: 'none' }
  | { type: 'bearer'; token: string }
  | { type: 'basic'; username: string; password: string }
  | { type: 'apikey'; key: string; value: string; placement: string }
  | { type: 'digest'; username: string; password: string }
  | { type: 'awsv4'; accessKeyId: string; secretAccessKey: string; sessionToken: string; service: string; region: string; profileName: string }
  | OAuth2Auth
  | { type: 'other'; label: string; config?: string };

export interface Assertion {
  expression: string;
  operator: string;
  value?: string | null;
  enabled: boolean;
  description?: string | null;
}

export interface RequestDoc {
  name: string;
  requestType: string;
  seq?: number | null;
  method: string;
  url: string;
  params: Param[];
  headers: KeyValue[];
  body: Body;
  auth: Auth;
  assertions: Assertion[];
  variables: KeyValue[];
  scripts: Script[];
  /** Variables posées après la réponse (lues seulement : le fichier garde ses actions). */
  postVariables?: { name: string; expression: string; enabled: boolean }[];
  docs?: string | null;
  timeoutMs?: number | null;
}

export type TreeItem =
  | { kind: 'folder'; path: string; name: string; seq?: number | null; children: TreeItem[] }
  | { kind: 'request'; path: string; name: string; seq?: number | null; method: string; requestType: string; url: string; deprecated: boolean; error?: string };

export interface CollectionInfo {
  root: string;
  name: string;
  items: TreeItem[];
  environments: string[];
  defaultEnvironment?: string | null;
  requestCount: number;
}

/** Lot de changements du disque dans la collection `root` ; `truncated` : tout a pu changer, il faut tout relire. */
export interface DiskChange {
  root: string;
  paths: string[];
  truncated: boolean;
}

export interface EnvVar {
  name: string;
  value?: string | null;
  secret: boolean;
  enabled: boolean;
  description?: string | null;
  /** Type d'une valeur qui n'est pas du texte (`number`, `boolean`…) : conservé à l'enregistrement. */
  dataType?: string | null;
}

export interface Rung {
  level: string;
  source: string;
  value?: string | null;
}

export interface VariableInfo {
  name: string;
  value?: string | null;
  level?: string | null;
  secret: boolean;
  rungs: Rung[];
}

export interface Timings {
  dnsMs: number;
  tcpMs: number;
  tlsMs: number;
  ttfbMs: number;
  downloadMs: number;
  totalMs: number;
}

export interface ResponseDto {
  status: number;
  reason: string;
  httpVersion: string;
  remoteAddr: string;
  headers: [string, string][];
  body: string;
  /** Corps ré-indenté par le moteur, jetons intacts ; `null` si ce n'est pas du JSON. */
  pretty: string | null;
  size: number;
  timings: Timings;
}

export interface AssertionResult {
  expression: string;
  operator: string;
  expected?: string | null;
  actual: string;
  passed: boolean;
  error?: string | null;
}

export interface Script {
  /** `before-request`, `after-response` ou `tests`. */
  kind: string;
  code: string;
}

export interface TestResult {
  description: string;
  status: string;
  error?: string | null;
  actual?: unknown;
  expected?: unknown;
}

export interface LogLine {
  level: string;
  args: unknown[];
}

/** Ce que les scripts d'une phase ont produit. */
export interface PhaseReport {
  results: TestResult[];
  logs: LogLine[];
  error?: string | null;
}

export interface ScriptsReport {
  pre: PhaseReport;
  post: PhaseReport;
  tests: PhaseReport;
}

export interface RunError {
  stage: 'prepare' | 'preRequestScript' | 'send';
  message: string;
}

export interface SendResult {
  method: string;
  url: string;
  unresolved: string[];
  /** Absente quand la requête n'est pas partie ou n'a pas abouti. */
  response: ResponseDto | null;
  assertions: AssertionResult[];
  scripts: ScriptsReport;
  error?: RunError | null;
  skipped: boolean;
}

/** Un envoi qui a abouti : la réponse est là. */
export type Sent = SendResult & { response: ResponseDto };

export type GroupBy = 'tags' | 'path';

export interface SpecSummary {
  title: string;
  version?: string | null;
  /** `openapi` ou `swagger`. */
  format: string;
  formatVersion?: string | null;
  operationCount: number;
  tags: string[];
  servers: string[];
}

export interface OpenApiPreview {
  summary: SpecSummary;
  /** Nom du dossier que l'import créerait. */
  folderName: string;
}

export type ChangeKind = 'applied' | 'kept' | 'same' | 'merged' | 'conflict';
export type Choice = 'team' | 'spec' | 'both' | 'edit';
export type OpStatus = 'unchanged' | 'updated' | 'kept' | 'merged' | 'conflict' | 'new' | 'removed' | 'restored' | 'missing';

export interface SyncStatus {
  connected: boolean;
  source: string | null;
  groupBy: GroupBy | null;
  operationCount: number;
  removedCount: number;
}

export interface SpecRef {
  title: string;
  version: string;
}

export interface SyncChange {
  id: string;
  field: 'method' | 'url' | 'param' | 'header' | 'body' | 'auth';
  label: string;
  reason: string;
  kind: ChangeKind;
  base: string | null;
  ours: string | null;
  theirs: string | null;
  result: string | null;
  choices: Choice[];
}

export interface SyncOperation {
  key: string;
  name: string;
  method: string;
  path: string;
  file: string | null;
  status: OpStatus;
  changes: SyncChange[];
}

export interface SyncPairing {
  removed: string;
  added: string;
  reason: string;
}

export interface SyncSummary {
  unchanged: number;
  updated: number;
  kept: number;
  merged: number;
  conflicts: number;
  conflictFields: number;
  created: number;
  removed: number;
  restored: number;
  missing: number;
}

export interface SyncPlan {
  id: string;
  source: string;
  groupBy: GroupBy;
  hasBase: boolean;
  from: SpecRef | null;
  to: SpecRef;
  summary: SyncSummary;
  operations: SyncOperation[];
  suggestions: SyncPairing[];
}

export interface SyncDecisions {
  choices: Record<string, { choice: Choice; value?: string }>;
  skip: string[];
  recreate: string[];
}

export type LineRange = [first: number, last: number];

export interface Hunk {
  changeId: string;
  ours: LineRange | null;
  theirs: LineRange | null;
  base: LineRange | null;
  result: LineRange | null;
}

export interface OpView {
  ours: string;
  theirs: string;
  base: string | null;
  result: string;
  hunks: Hunk[];
}

export interface SyncReport {
  written: string[];
  created: string[];
  removed: string[];
  ignored: string[];
}

/** Ce que le runner exécute : toute la collection (`targets` vide) ou des dossiers. */
export interface RunArgs {
  runId: string;
  root: string;
  targets: string[];
  env: string | null;
  bail: boolean;
  delayMs: number;
  /** Fichier CSV ou JSON : une itération par ligne. */
  data: string | null;
}

/** `fail` : un test, une assertion ou un script a échoué ; `error` : la requête n'a pas abouti. */
export type RunStatus = 'pass' | 'fail' | 'error' | 'skipped';
export type RunSkip = 'script' | 'bail' | 'stopExecution' | 'unreadable';

export interface RunResult {
  iteration: number;
  name: string;
  path: string;
  method: string;
  url: string;
  status: RunStatus;
  skipped: RunSkip | null;
  http: { status: number; reason: string; size: number; timeMs: number } | null;
  error: RunError | null;
  assertions: AssertionResult[];
  scripts: ScriptsReport;
  durationMs: number;
}

export interface RunSummary {
  totalRequests: number;
  passedRequests: number;
  failedRequests: number;
  errorRequests: number;
  skippedRequests: number;
  skippedByBail: number;
  totalAssertions: number;
  passedAssertions: number;
  failedAssertions: number;
  totalTests: number;
  passedTests: number;
  failedTests: number;
  totalPreRequestTests: number;
  passedPreRequestTests: number;
  failedPreRequestTests: number;
  totalPostResponseTests: number;
  passedPostResponseTests: number;
  failedPostResponseTests: number;
}

export type RunHalt =
  | { kind: 'bail'; request: string; reason: string; remaining: number }
  | { kind: 'stopExecution'; request: string; remaining: number }
  | { kind: 'loop' }
  | { kind: 'cancelled' };

export interface RunDone {
  runId: string;
  summary: RunSummary;
  halt: RunHalt | null;
  failed: boolean;
  elapsedMs: number;
}

export type RunEvent =
  | { kind: 'begin'; runId: string; requests: number; iterations: number }
  | { kind: 'iteration'; runId: string; index: number; total: number; row: Record<string, unknown> | null }
  | { kind: 'started'; runId: string; iteration: number; path: string; name: string; method: string }
  | { kind: 'finished'; runId: string; result: RunResult }
  | { kind: 'waiting'; runId: string; ms: number }
  | { kind: 'warning'; runId: string; message: string };

/** Ce qu'un fichier de données d'itération contient. */
export interface DataInfo {
  rows: number;
  columns: string[];
}

export type ReportFormat = 'html' | 'junit' | 'json';

export interface ExportArgs {
  runId: string;
  root: string;
  format: ReportFormat;
  path: string;
  skipHeaders: boolean;
  skipBodies: boolean;
}
