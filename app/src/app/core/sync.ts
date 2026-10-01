import type { ChangeKind, Choice, Hunk, LineRange, OpStatus, OpView, SyncChange, SyncDecisions, SyncOperation, SyncPlan, SyncReport } from './model';

export interface ConflictRef {
  key: string;
  changeId: string;
}

export interface Selection {
  key: string;
  changeId: string | null;
}

export interface LineMark {
  from: number;
  to: number;
  cls: string;
}

export interface FoldRange {
  from: number;
  to: number;
  label: string;
}

export interface LensChoice {
  key: Choice;
  label: string;
}

export interface Lens {
  id: string;
  line: number;
  label: string | null;
  choices: LensChoice[];
  active: Choice | null;
}

export interface Tone {
  text: string;
  tone: 'good' | 'warn' | '';
}

export interface SpecStatus {
  tone: 'good' | 'warn' | '';
  label: string;
}

export interface PlanGroups {
  conflicts: (ConflictRef & { op: SyncOperation; change: SyncChange })[];
  auto: SyncOperation[];
  created: SyncOperation[];
  removed: SyncOperation[];
  missing: SyncOperation[];
}

export const CONTEXT_LINES = 3;
export const MIN_FOLDED_LINES = 3;

export const CHOICE_LABELS: Record<Choice, string> = {
  team: "Garder l'équipe",
  spec: 'Prendre la spec',
  both: 'Combiner les deux',
  edit: 'Éditer à la main',
};

const CHOICE_ORDER: Choice[] = ['team', 'spec', 'both', 'edit'];

const KIND_LABELS: Record<ChangeKind, string> = {
  applied: 'Appliqué depuis la spec',
  kept: "Gardé de l'équipe",
  same: 'Identique des deux côtés',
  merged: 'Fusionné',
  conflict: 'À arbitrer',
};

const KIND_TAGS: Record<ChangeKind, string> = { applied: 'spec', kept: 'équipe', same: 'identique', merged: 'fusionné', conflict: 'à arbitrer' };

const OP_STATES: Record<OpStatus, Tone> = {
  unchanged: { text: 'inchangée', tone: '' },
  updated: { text: 'spec appliquée', tone: 'good' },
  kept: { text: 'équipe gardée', tone: '' },
  merged: { text: 'fusionnée', tone: 'good' },
  conflict: { text: 'à arbitrer', tone: 'warn' },
  new: { text: 'nouvelle', tone: '' },
  removed: { text: 'conservée', tone: '' },
  restored: { text: 'rétablie', tone: 'good' },
  missing: { text: 'fichier introuvable', tone: '' },
};

const ORIGIN_CLASSES: Record<Choice, string> = { team: 'r-o', spec: 'r-t', both: 'r-b', edit: 'r-e' };
const KIND_ORIGINS: Record<ChangeKind, string> = { applied: 'r-t', kept: 'r-o', merged: 'r-b', same: '', conflict: '' };
const AUTO_STATUSES: OpStatus[] = ['updated', 'merged', 'kept', 'restored'];
const WRITTEN_AUTO_STATUSES: OpStatus[] = ['updated', 'merged', 'restored'];

export const noDecisions = (): SyncDecisions => ({ choices: {}, skip: [], recreate: [] });

export const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n > 1 ? many : one}`;

export const choiceLabel = (choice: Choice) => CHOICE_LABELS[choice];
export const kindLabel = (kind: ChangeKind) => KIND_LABELS[kind];
export const kindTag = (kind: ChangeKind) => KIND_TAGS[kind];
export const opState = (status: OpStatus) => OP_STATES[status];

/** Les changements de type conflit de tout le plan, dans l'ordre des opérations. */
export function conflictRefs(plan: SyncPlan): ConflictRef[] {
  return plan.operations.flatMap((op) => op.changes.filter((c) => c.kind === 'conflict').map((c) => ({ key: op.key, changeId: c.id })));
}

export function isArbitrated(decisions: SyncDecisions, changeId: string): boolean {
  const decision = decisions.choices[changeId];
  return !!decision && (decision.choice !== 'edit' || decision.value !== undefined);
}

export function conflictsLeft(plan: SyncPlan | null, decisions: SyncDecisions): number {
  return plan ? conflictRefs(plan).filter((r) => !isArbitrated(decisions, r.changeId)).length : 0;
}

/** Applique le choix ; refaire le choix déjà actif l'annule. `value` accompagne « Éditer à la main ». */
export function choose(decisions: SyncDecisions, changeId: string, choice: Choice, value?: string): SyncDecisions {
  const { [changeId]: previous, ...others } = decisions.choices;
  if (previous?.choice === choice) return { ...decisions, choices: others };
  return { ...decisions, choices: { ...others, [changeId]: value === undefined ? { choice } : { choice, value } } };
}

export function editValue(decisions: SyncDecisions, changeId: string, value: string): SyncDecisions {
  return { ...decisions, choices: { ...decisions.choices, [changeId]: { choice: 'edit', value } } };
}

export function toggleSkip(decisions: SyncDecisions, key: string): SyncDecisions {
  const skipped = decisions.skip.includes(key);
  return { ...decisions, skip: skipped ? decisions.skip.filter((k) => k !== key) : [...decisions.skip, key] };
}

export function setRecreate(decisions: SyncDecisions, key: string, recreate: boolean): SyncDecisions {
  const others = decisions.recreate.filter((k) => k !== key);
  return { ...decisions, recreate: recreate ? [...others, key] : others };
}

/** Valeur proposée quand on passe en « Éditer à la main » : le résultat courant, sinon l'équipe, sinon la spec. */
export function initialEditValue(change: SyncChange): string {
  return change.result ?? change.ours ?? change.theirs ?? '';
}

export function isCodeField(change: SyncChange): boolean {
  return change.field === 'body';
}

export function codeLanguage(text: string): 'json' | 'text' {
  return /^\s*[{[]/.test(text) ? 'json' : 'text';
}

export function groupOps(plan: SyncPlan): PlanGroups {
  const byStatus = (statuses: OpStatus[]) => plan.operations.filter((o) => statuses.includes(o.status));
  const conflicts = plan.operations.flatMap((op) =>
    op.changes.filter((change) => change.kind === 'conflict').map((change) => ({ key: op.key, changeId: change.id, op, change })),
  );
  return { conflicts, auto: byStatus(AUTO_STATUSES), created: byStatus(['new']), removed: byStatus(['removed']), missing: byStatus(['missing']) };
}

/** Nombre de lignes de la barre latérale que l'application écrira : un changement chacune. */
export function applyCount(plan: SyncPlan, decisions: SyncDecisions): number {
  const { conflicts, auto, created, removed, missing } = groupOps(plan);
  return (
    conflicts.length +
    auto.filter((o) => WRITTEN_AUTO_STATUSES.includes(o.status)).length +
    created.filter((o) => !decisions.skip.includes(o.key)).length +
    removed.length +
    missing.filter((o) => decisions.recreate.includes(o.key)).length
  );
}

export function isUpToDate(plan: SyncPlan): boolean {
  const s = plan.summary;
  return plan.hasBase && s.updated + s.merged + s.conflicts + s.created + s.removed + s.restored + s.missing === 0;
}

export function applyLabel(plan: SyncPlan, decisions: SyncDecisions): string {
  const n = applyCount(plan, decisions);
  if (n === 0) return plan.hasBase ? 'Mettre à jour la base' : 'Enregistrer la connexion';
  return `Appliquer ${plural(n, 'changement')}`;
}

/** Libellé et teinte de l'item de la barre d'état ; `plan` est `null` tant que la comparaison n'a pas eu lieu. */
export function specStatus(plan: SyncPlan | null, decisions: SyncDecisions, synced: boolean): SpecStatus {
  if (!plan) return synced ? { tone: 'good', label: 'Spec à jour' } : { tone: '', label: 'Spec OpenAPI' };
  const left = conflictsLeft(plan, decisions);
  if (left > 0) return { tone: 'warn', label: `Spec : ${plural(left, 'conflit')}` };
  return isUpToDate(plan) ? { tone: 'good', label: 'Spec à jour' } : { tone: '', label: 'Spec : prête à appliquer' };
}

/** Fichiers des requêtes qui ont encore un conflit à arbitrer. */
export function conflictPaths(plan: SyncPlan | null, decisions: SyncDecisions): Set<string> {
  const paths = new Set<string>();
  for (const op of plan?.operations ?? []) {
    if (op.file && op.changes.some((c) => c.kind === 'conflict' && !isArbitrated(decisions, c.id))) paths.add(op.file);
  }
  return paths;
}

/** Conflit suivant ou précédent, en bouclant ; sans sélection, le premier ou le dernier. */
export function stepConflict(refs: ConflictRef[], selection: Selection | null, direction: 1 | -1): ConflictRef | null {
  if (!refs.length) return null;
  const index = refs.findIndex((r) => r.changeId === selection?.changeId);
  if (index < 0) return direction > 0 ? refs[0] : refs[refs.length - 1];
  return refs[(index + direction + refs.length) % refs.length];
}

/** Sélection à l'ouverture d'un plan : le premier conflit non arbitré, sinon le premier conflit. */
export function firstSelection(plan: SyncPlan, decisions: SyncDecisions): Selection | null {
  const refs = conflictRefs(plan);
  const target = refs.find((r) => !isArbitrated(decisions, r.changeId)) ?? refs[0];
  return target ? { key: target.key, changeId: target.changeId } : null;
}

export function isValidSelection(plan: SyncPlan, selection: Selection | null): boolean {
  const op = plan.operations.find((o) => o.key === selection?.key);
  return !!selection && !!op && (selection.changeId === null || op.changes.some((c) => c.id === selection.changeId));
}

const same = (a: SyncChange, b: SyncChange) => a.ours === b.ours && a.theirs === b.theirs && a.base === b.base && a.choices.join() === b.choices.join();

/** Garde les décisions qui portent encore sur le même contenu dans le nouveau plan. */
export function keepDecisions(previous: SyncPlan | null, next: SyncPlan, decisions: SyncDecisions): SyncDecisions {
  if (!previous) return noDecisions();
  const changes = (plan: SyncPlan) => new Map(plan.operations.flatMap((o) => o.changes).map((c) => [c.id, c]));
  const before = changes(previous);
  const after = changes(next);
  const statusOf = (key: string) => next.operations.find((o) => o.key === key)?.status;
  const kept = Object.entries(decisions.choices).filter(([id, d]) => {
    const [old, current] = [before.get(id), after.get(id)];
    return old && current && current.kind === 'conflict' && same(old, current) && current.choices.includes(d.choice);
  });
  return {
    choices: Object.fromEntries(kept),
    skip: decisions.skip.filter((k) => statusOf(k) === 'new'),
    recreate: decisions.recreate.filter((k) => statusOf(k) === 'missing'),
  };
}

/** Nom de fichier ou dernier segment d'URL d'une source. */
export const sourceName = (source: string) => source.split(/[\\/]/).filter(Boolean).pop() ?? source;

export const versionOf = (version: string) => (/^v/i.test(version) ? version : `v${version}`);

/** « v2.3.0 → v2.4.0 », « v2.4.0 · sans base » à la première synchro, « v2.4.0 · même version » si seule la spec a bougé. */
export function versionLabel(plan: SyncPlan): string {
  const to = versionOf(plan.to.version);
  if (!plan.from) return `${to} · sans base`;
  const from = versionOf(plan.from.version);
  return from === to ? `${to} · même version` : `${from} → ${to}`;
}

export function pairingLabel(plan: SyncPlan, key: string): string {
  const op = plan.operations.find((o) => o.key === key);
  return op ? `${op.method} ${op.path}` : key;
}

export function lineCount(text: string): number {
  return trimEnd(text).split('\n').length;
}

/** Texte sans retour à la ligne final : CodeMirror compterait sinon une ligne vide de plus. */
export function trimEnd(text: string): string {
  return text.replace(/\n$/, '');
}

export function foldLabel({ from, to }: Pick<FoldRange, 'from' | 'to'>): string {
  return from === to ? `ligne ${from} masquée, identique des deux côtés` : `lignes ${from} à ${to} masquées, identiques des deux côtés`;
}

/** Lignes à replier : tout ce qui est à plus de `context` lignes d'une plage mise en avant, si cela fait assez de lignes. */
export function foldRanges(total: number, ranges: LineRange[], context = CONTEXT_LINES, minimum = MIN_FOLDED_LINES): FoldRange[] {
  if (!ranges.length) return [];
  const shown: LineRange[] = [];
  for (const [first, last] of [...ranges].sort((a, b) => a[0] - b[0])) {
    const [from, to] = [Math.max(1, first - context), Math.min(total, last + context)];
    const tail = shown[shown.length - 1];
    if (tail && from <= tail[1] + 1) tail[1] = Math.max(tail[1], to);
    else shown.push([from, to]);
  }
  const gaps: LineRange[] = [];
  let next = 1;
  for (const [from, to] of shown) {
    gaps.push([next, from - 1]);
    next = to + 1;
  }
  gaps.push([next, total]);
  return gaps.filter(([from, to]) => to - from + 1 >= minimum).map(([from, to]) => ({ from, to, label: foldLabel({ from, to }) }));
}

function tinted(kind: ChangeKind, side: 'ours' | 'theirs'): boolean {
  if (kind === 'same') return false;
  if (kind === 'applied') return side === 'theirs';
  if (kind === 'kept') return side === 'ours';
  return true;
}

export type PaneSide = 'ours' | 'theirs' | 'base' | 'result';

export function hunkRanges(view: OpView, side: PaneSide): LineRange[] {
  return view.hunks.flatMap((h) => {
    const range = h[side];
    return range ? [range] : [];
  });
}

const toMarks = (marks: [LineRange | null, string][]): LineMark[] => marks.flatMap(([range, cls]) => (range ? [{ from: range[0], to: range[1], cls }] : []));

/** Lignes à teinter dans un volet Équipe, Spec ou Base : seulement ce que ce côté a changé. */
export function paneMarks(view: OpView, changes: SyncChange[], side: 'ours' | 'theirs' | 'base'): LineMark[] {
  const cls = { ours: 'h-o', theirs: 'h-t', base: 'h-b' }[side];
  const kindOf = (h: Hunk) => changes.find((c) => c.id === h.changeId)?.kind ?? 'conflict';
  return toMarks(view.hunks.map((h): [LineRange | null, string] => [side === 'base' || tinted(kindOf(h), side) ? h[side] : null, cls]));
}

/** Teinte et gouttière du Résultat : ambre tant que le conflit n'est pas arbitré, sinon l'origine du choix. */
export function resultMarks(view: OpView, changes: SyncChange[], decisions: SyncDecisions): LineMark[] {
  const markOf = (h: Hunk): [LineRange | null, string] => {
    const change = changes.find((c) => c.id === h.changeId);
    if (!change || change.kind === 'same') return [null, ''];
    if (change.kind !== 'conflict') return [h.result, KIND_ORIGINS[change.kind]];
    const decision = decisions.choices[change.id];
    return [h.result, decision && isArbitrated(decisions, change.id) ? ORIGIN_CLASSES[decision.choice] : 'r-u'];
  };
  return toMarks(view.hunks.map(markOf));
}

/** Lentilles du Résultat, une par conflit ; sans ligne dans le résultat, elle est posée en tête avec le nom du champ. */
export function lensesFor(view: OpView, changes: SyncChange[], decisions: SyncDecisions): Lens[] {
  return changes
    .filter((c) => c.kind === 'conflict')
    .map((c) => {
      const line = view.hunks.find((h) => h.changeId === c.id)?.result?.[0] ?? null;
      return {
        id: c.id,
        line: line ?? 1,
        label: line === null ? c.label : null,
        choices: CHOICE_ORDER.filter((k) => c.choices.includes(k)).map((key) => ({ key, label: CHOICE_LABELS[key] })),
        active: decisions.choices[c.id]?.choice ?? null,
      };
    });
}

const agree = (n: number, noun: string, adjective: string) => `${plural(n, noun)} ${adjective}${n > 1 ? 's' : ''}`;

export function reportSummary(report: SyncReport): string {
  const parts = [
    report.written.length && agree(report.written.length, 'requête', 'modifiée'),
    report.created.length && agree(report.created.length, 'requête', 'créée'),
    report.removed.length && agree(report.removed.length, 'requête', 'dépréciée'),
  ].filter(Boolean);
  return parts.length ? parts.join(', ') : 'Aucune requête modifiée';
}

export function touchedFiles(report: SyncReport): string[] {
  return [...report.written, ...report.created, ...report.removed];
}
