import type { Api } from './api';
import type {
  ChangeKind,
  Choice,
  Hunk,
  LineRange,
  OpStatus,
  OpView,
  RequestDoc,
  SyncChange,
  SyncDecisions,
  SyncOperation,
  SyncPlan,
  SyncReport,
  SyncStatus,
} from './model';

type Side = 'ours' | 'theirs' | 'base' | 'result';

interface Chunk {
  id: string;
  field: SyncChange['field'];
  label: string;
  reason: string;
  kind: ChangeKind;
  base: string | null;
  ours: string | null;
  theirs: string | null;
  both?: string;
  merged?: string;
  choices?: Choice[];
  render: (value: string | null) => string[];
}

type Segment = string | Chunk;

interface DemoOp {
  key: string;
  name: string;
  method: string;
  path: string;
  file: string | null;
  status: OpStatus;
  segments: Segment[];
}

interface DemoPlan {
  ops: DemoOp[];
  noBase: boolean;
}

const SOURCE = './spec/paiements.openapi.yml';
const PAIRING = { removed: 'GET /transactions/export', added: 'GET /transactions/exports' };
const DEFAULT_CHOICES: Partial<Record<SyncChange['field'], Choice[]>> = { body: ['team', 'spec', 'both', 'edit'], url: ['team', 'spec', 'edit'] };

const specPath = (url: string) =>
  url
    .replace(/^\{\{[^}]+\}\}/, '')
    .replace(/\?.*$/, '')
    .replace(/:(\w+)/g, '{$1}');
const scalar = (value: string) => (/[{}":#]|^\d+$|^(true|false|null)$/.test(value) ? JSON.stringify(value) : value);
const queryOf = (doc: RequestDoc) => (doc.url.includes('?') ? doc.url.slice(doc.url.indexOf('?')) : '');
const bodyOf = (doc: RequestDoc) => (doc.body.type === 'json' ? doc.body.data : '');

const info = (doc: RequestDoc, seq: number) => ['info:', `  name: ${doc.name}`, '  type: http', `  seq: ${seq}`, 'http:', `  method: ${doc.method}`];
const urlLine = (doc: RequestDoc) => `  url: ${JSON.stringify(doc.url)}`;
const paramItem = (p: RequestDoc['params'][number]) => [`    - name: ${p.name}`, `      value: ${JSON.stringify(p.value)}`, `      type: ${p.kind}`];
const params = (doc: RequestDoc) => (doc.params.length ? ['  params:', ...doc.params.flatMap(paramItem)] : []);
const assertions = (doc: RequestDoc) => [
  'runtime:',
  '  assertions:',
  ...doc.assertions.flatMap((a) => [`    - expression: ${a.expression}`, `      operator: ${a.operator}`, `      value: ${JSON.stringify(a.value ?? '')}`]),
];
const tail = (doc: RequestDoc) => ['  auth: inherit', ...assertions(doc), 'settings:', '  encodeUrl: true'];

const lines = (build: (value: string) => string[]) => (value: string | null) => (value === null ? [] : build(value));
const bodyBlock = lines((v) => ['  body:', '    type: json', '    data: |-', ...v.split('\n').map((l) => `      ${l}`)]);
const header = (name: string) => lines((v) => ['  headers:', `    - name: ${name}`, `      value: ${scalar(v)}`]);
const formField = (name: string) => lines((v) => [`      - name: ${name}`, `        value: ${scalar(v)}`]);
const headerItem = (name: string) => lines((v) => [`    - name: ${name}`, `      value: ${scalar(v)}`]);

export function createDemoSync(files: Record<string, RequestDoc>): Pick<Api, 'syncStatus' | 'syncPlan' | 'syncOpView' | 'syncApply'> {
  const plans = new Map<string, DemoPlan>();
  let counter = 0;

  const doc = (path: string) => files[path];
  const op = (path: string, key: string, status: OpStatus, segments: Segment[], extra: Partial<DemoOp> = {}): DemoOp => ({
    key,
    name: doc(path).name,
    method: doc(path).method,
    path: specPath(doc(path).url),
    file: path,
    status,
    segments,
    ...extra,
  });

  const list = doc('transactions/liste.yml');
  const detail = doc('transactions/detail.yml');
  const cancel = doc('transactions/annuler.yml');
  const login = doc('auth/connexion.yml');
  const token = doc('auth/jeton.yml');
  const proof = doc('transactions/justificatif.yml');
  const exported = doc('transactions/export.yml');

  const operations = (paired: boolean): DemoOp[] => [
    op('transactions/liste.yml', 'listTransactions', 'conflict', [
      ...info(list, 2),
      {
        id: 'url',
        field: 'url',
        label: 'Adresse',
        reason: 'L\'équipe a corrigé « transaction » en « transactions », la spec renomme la ressource en « paiements ».',
        kind: 'conflict',
        base: '{{baseUrl}}/transaction',
        ours: '{{baseUrl}}/transactions',
        theirs: '{{baseUrl}}/paiements',
        render: lines((v) => [`  url: ${JSON.stringify(v + queryOf(list))}`]),
      },
      ...params(list),
      ...tail(list),
    ], { path: '/paiements' }),
    op('transactions/detail.yml', 'getTransaction', 'conflict', [
      ...info(detail, 3),
      urlLine(detail),
      ...params(detail),
      {
        id: 'header/x-canal',
        field: 'header',
        label: 'En-tête X-Canal',
        reason: "L'équipe a rendu le canal variable, la spec retire cet en-tête.",
        kind: 'conflict',
        base: 'WEB',
        ours: '{{canal}}',
        theirs: null,
        choices: ['team', 'spec'],
        render: header('X-Canal'),
      },
      ...tail(detail),
    ]),
    op('transactions/annuler.yml', 'cancelTransaction', 'conflict', [
      ...info(cancel, 4),
      urlLine(cancel),
      '  params:',
      ...paramItem(cancel.params[0]),
      {
        id: 'param/path/id',
        field: 'param',
        label: 'Paramètre id',
        reason: 'La spec décrit le paramètre de chemin.',
        kind: 'applied',
        base: null,
        ours: null,
        theirs: 'Identifiant de la transaction',
        render: lines((v) => [`      description: ${v}`]),
      },
      {
        id: 'body',
        field: 'body',
        label: 'Corps JSON',
        reason: "L'équipe a réécrit le motif, la spec en fait une énumération et ajoute « notifier ».",
        kind: 'conflict',
        base: '{\n  "motif": "ERREUR",\n  "canal": "USSD"\n}',
        ours: bodyOf(cancel),
        theirs: '{\n  "motif": "ERREUR_SAISIE",\n  "canal": "USSD",\n  "notifier": true\n}',
        both: '{\n  "motif": "Erreur de saisie",\n  "canal": "USSD",\n  "notifier": true\n}',
        render: bodyBlock,
      },
      ...tail(cancel),
    ]),
    op('auth/connexion.yml', 'login', 'kept', [
      ...info(login, 1),
      urlLine(login),
      {
        id: 'body',
        field: 'body',
        label: 'Corps JSON',
        reason: "Seule l'équipe a modifié le corps : il est conservé.",
        kind: 'kept',
        base: '{\n  "identifiant": "caisse-01",\n  "motDePasse": "{{process.env.PSP_PASSWORD}}"\n}',
        ours: bodyOf(login),
        theirs: '{\n  "identifiant": "caisse-01",\n  "motDePasse": "{{process.env.PSP_PASSWORD}}"\n}',
        render: bodyBlock,
      },
      ...tail(login),
    ]),
    op('auth/jeton.yml', 'issueToken', 'merged', [
      ...info(token, 5),
      urlLine(token),
      '  body:',
      '    type: form-urlencoded',
      '    data:',
      '      - name: grant_type',
      '        value: client_credentials',
      {
        id: 'body/form/client_id',
        field: 'body',
        label: 'Champ client_id',
        reason: "Seule l'équipe a changé cette valeur : elle est conservée.",
        kind: 'kept',
        base: 'caisse-01',
        ours: 'caisse-ouaga-01',
        theirs: 'caisse-01',
        render: formField('client_id'),
      },
      '      - name: client_secret',
      '        value: "{{process.env.PSP_SECRET}}"',
      '      - name: scope',
      '        value: transactions:lire',
      '        disabled: true',
      {
        id: 'body/form/audience',
        field: 'body',
        label: 'Champ audience',
        reason: 'Champ ajouté par la spec.',
        kind: 'applied',
        base: null,
        ours: null,
        theirs: 'psp-api',
        render: formField('audience'),
      },
      ...tail(token),
    ]),
    op('transactions/justificatif.yml', 'attachProof', 'updated', [
      ...info(proof, 6),
      urlLine(proof),
      ...params(proof),
      {
        id: 'header/accept',
        field: 'header',
        label: 'En-tête Accept',
        reason: 'En-tête ajouté par la spec.',
        kind: 'applied',
        base: null,
        ours: null,
        theirs: 'application/json',
        render: lines((v) => ['  headers:', ...headerItem('Accept')(v)]),
      },
      '  body:',
      '    type: multipart-form',
      ...tail(proof),
    ]),
    paired
      ? op('transactions/export.yml', PAIRING.added, 'updated', [
          ...info(exported, 7),
          {
            id: 'url',
            field: 'url',
            label: 'Adresse',
            reason: 'Chemin rapproché à la main : la spec le renomme.',
            kind: 'applied',
            base: '{{baseUrl}}/transactions/export',
            ours: '{{baseUrl}}/transactions/export',
            theirs: '{{baseUrl}}/transactions/exports',
            render: lines((v) => [`  url: ${JSON.stringify(v)}`]),
          },
          ...tail(exported),
        ], { path: '/transactions/exports' })
      : op('transactions/export.yml', PAIRING.removed, 'removed', [...info(exported, 7), urlLine(exported), ...tail(exported)]),
    {
      key: 'createRefund',
      name: 'Créer un remboursement',
      method: 'POST',
      path: '/remboursements',
      file: 'remboursements/Créer un remboursement.yml',
      status: 'new',
      segments: [],
    },
    {
      key: 'getRefund',
      name: "Détail d'un remboursement",
      method: 'GET',
      path: '/remboursements/{id}',
      file: "remboursements/Détail d'un remboursement.yml",
      status: 'new',
      segments: [],
    },
    ...(paired
      ? []
      : [{ key: PAIRING.added, name: 'Export des transactions', method: 'GET', path: '/transactions/exports', file: 'transactions/Export des transactions.yml', status: 'new' as const, segments: [] }]),
    { key: 'getClient', name: "Détail d'un client", method: 'GET', path: '/clients/{id}', file: 'clients/detail.yml', status: 'missing', segments: [] },
  ];

  const withoutBase = (c: Chunk): Chunk | null => {
    if (c.ours === c.theirs) return null;
    const keyed = c.field === 'param' || c.field === 'header';
    return { ...c, base: null, kind: keyed ? (c.ours === null ? 'applied' : 'kept') : 'conflict' };
  };

  const noBaseStatus = (chunks: Chunk[]): OpStatus => {
    if (chunks.some((c) => c.kind === 'conflict')) return 'conflict';
    const kinds = new Set(chunks.map((c) => c.kind));
    return kinds.size === 2 ? 'merged' : kinds.has('applied') ? 'updated' : kinds.has('kept') ? 'kept' : 'unchanged';
  };

  const withoutBasePlan = (ops: DemoOp[]): DemoOp[] =>
    ops
      .filter((o) => o.status !== 'removed' && o.status !== 'missing')
      .map((o) => {
        const segments = o.segments.flatMap((s): Segment[] => {
          const chunk = typeof s === 'string' ? s : withoutBase(s);
          return chunk === null ? [] : [chunk];
        });
        const chunks = segments.filter((s): s is Chunk => typeof s !== 'string');
        return o.status === 'new' ? o : { ...o, segments, status: noBaseStatus(chunks) };
      });

  const chunksOf = (o: DemoOp) => o.segments.filter((s): s is Chunk => typeof s !== 'string');
  const changeId = (o: DemoOp, c: Chunk) => `${o.key}::${c.id}`;
  const choicesOf = (c: Chunk): Choice[] => (c.kind === 'conflict' ? (c.choices ?? DEFAULT_CHOICES[c.field] ?? ['team', 'spec']) : []);

  const resultValue = (c: Chunk, decision?: SyncDecisions['choices'][string]): string | null => {
    if (c.kind === 'applied') return c.theirs;
    if (c.kind === 'merged') return c.merged ?? c.ours;
    if (c.kind !== 'conflict') return c.ours;
    if (decision?.choice === 'spec') return c.theirs;
    if (decision?.choice === 'both') return c.both ?? c.ours;
    if (decision?.choice === 'edit') return decision.value ?? c.ours;
    return c.ours;
  };

  const changeOf = (o: DemoOp, c: Chunk): SyncChange => ({
    id: changeId(o, c),
    field: c.field,
    label: c.label,
    reason: c.reason,
    kind: c.kind,
    base: c.base,
    ours: c.ours,
    theirs: c.theirs,
    result: resultValue(c),
    choices: choicesOf(c),
  });

  const summarize = (ops: DemoOp[]): SyncPlan['summary'] => {
    const count = (status: OpStatus) => ops.filter((o) => o.status === status).length;
    const fields = ops.flatMap(chunksOf).filter((c) => c.kind === 'conflict').length;
    return {
      unchanged: count('unchanged'),
      updated: count('updated'),
      kept: count('kept'),
      merged: count('merged'),
      conflicts: count('conflict'),
      conflictFields: fields,
      created: count('new'),
      removed: count('removed'),
      restored: count('restored'),
      missing: count('missing'),
    };
  };

  const compose = (o: DemoOp, decisions: SyncDecisions, noBase: boolean): OpView => {
    const text: Record<Side, string[]> = { ours: [], theirs: [], base: [], result: [] };
    const hunks: Hunk[] = [];
    const sides: Side[] = ['ours', 'theirs', 'base', 'result'];
    for (const segment of o.segments) {
      if (typeof segment === 'string') {
        for (const side of sides) text[side].push(segment);
        continue;
      }
      const block: Record<Side, string[]> = {
        ours: segment.render(segment.ours),
        theirs: segment.render(segment.theirs),
        base: segment.render(segment.base),
        result: segment.render(resultValue(segment, decisions.choices[changeId(o, segment)])),
      };
      const range = (side: Side): LineRange | null => (block[side].length ? [text[side].length + 1, text[side].length + block[side].length] : null);
      hunks.push({ changeId: changeId(o, segment), ours: range('ours'), theirs: range('theirs'), base: noBase ? null : range('base'), result: range('result') });
      for (const side of sides) text[side].push(...block[side]);
    }
    const join = (side: Side) => `${text[side].join('\n')}\n`;
    return { ours: join('ours'), theirs: join('theirs'), base: noBase ? null : join('base'), result: join('result'), hunks };
  };

  const asPlan = (id: string, source: string, { ops, noBase }: DemoPlan, pairings: [string, string][]): SyncPlan => {
    const paired = pairings.some(([removed, added]) => removed === PAIRING.removed && added === PAIRING.added);
    return {
      id,
      source,
      groupBy: 'tags',
      hasBase: !noBase,
      from: noBase ? null : { title: 'API Paiements', version: '2.3.0' },
      to: { title: 'API Paiements', version: '2.4.0' },
      summary: summarize(ops),
      operations: ops.map((o): SyncOperation => ({
        key: o.key,
        name: o.name,
        method: o.method,
        path: o.path,
        file: o.file,
        status: o.status,
        changes: chunksOf(o).map((c) => changeOf(o, c)),
      })),
      suggestions: noBase || paired ? [] : [{ ...PAIRING, reason: 'Même méthode, chemin voisin : /transactions/export devient /transactions/exports.' }],
    };
  };

  const status = (): SyncStatus => ({ connected: true, source: SOURCE, groupBy: 'tags', operationCount: 9, removedCount: 1 });

  return {
    syncStatus: async () => status(),
    syncPlan: async (_root, source, pairings) => {
      const noBase = source !== null && source !== SOURCE;
      const paired = pairings.length > 0;
      const all = operations(paired);
      const plan: DemoPlan = { ops: noBase ? withoutBasePlan(all) : all, noBase };
      const id = `demo-${++counter}`;
      plans.set(id, plan);
      return asPlan(id, source ?? SOURCE, plan, pairings);
    },
    syncOpView: async (planId, key, decisions) => {
      const plan = plans.get(planId);
      const o = plan?.ops.find((x) => x.key === key);
      if (!plan || !o) return Promise.reject('Plan inconnu : relance la comparaison.');
      return compose(o, decisions, plan.noBase);
    },
    syncApply: async (planId, decisions) => {
      const plan = plans.get(planId);
      if (!plan) return Promise.reject('Plan inconnu : relance la comparaison.');
      const open = plan.ops.flatMap((o) => chunksOf(o).filter((c) => c.kind === 'conflict').map((c) => changeId(o, c))).filter((id) => !decisions.choices[id]);
      if (open.length) return Promise.reject(`Il reste ${open.length} conflit(s) à arbitrer.`);
      plans.delete(planId);
      const withFile = (statuses: OpStatus[]) => plan.ops.filter((o) => statuses.includes(o.status)).map((o) => o.file);
      const report: SyncReport = {
        written: withFile(['updated', 'merged', 'restored', 'conflict']).filter((f): f is string => !!f),
        created: plan.ops.filter((o) => o.status === 'new' && !decisions.skip.includes(o.key)).flatMap((o) => (o.file ? [o.file] : [])),
        removed: withFile(['removed']).filter((f): f is string => !!f),
        ignored: plan.ops.filter((o) => (o.status === 'new' && decisions.skip.includes(o.key)) || (o.status === 'missing' && !decisions.recreate.includes(o.key))).map((o) => o.key),
      };
      return report;
    },
  };
}
