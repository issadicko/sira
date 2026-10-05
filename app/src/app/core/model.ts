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

export type Auth =
  | { type: 'inherit' }
  | { type: 'none' }
  | { type: 'bearer'; token: string }
  | { type: 'basic'; username: string; password: string }
  | { type: 'apikey'; key: string; value: string; placement: string }
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
  scripts: { kind: string; code: string }[];
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

export interface SendResult {
  method: string;
  url: string;
  unresolved: string[];
  response: ResponseDto;
  assertions: AssertionResult[];
}

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
