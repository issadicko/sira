import { ChangeDetectionStrategy, Component, ElementRef, computed, effect, inject, signal, viewChild } from '@angular/core';

import { shortcutLabel } from '../core/commands';
import { Choice, SyncChange } from '../core/model';
import {
  ADDRESS_HINT,
  CHOICE_LABELS,
  PaneSide,
  QUERY_DROPPED,
  applyLabel,
  codeLanguage,
  foldRanges,
  hunkRanges,
  isArbitrated,
  isCodeField,
  isUpToDate,
  kindTag,
  lensesFor,
  lineCount,
  noBaseHint,
  paneMarks,
  plural,
  resultMarks,
  sourceName,
  trimEnd,
  versionOf,
  withoutQuery,
} from '../core/sync';
import { SyncStore } from '../core/sync-store';
import { CodeEditor } from './code-editor';
import { Icon } from './icon';
import { methodClass } from './method';

/** Éditeur de fusion à 3 voies : Équipe et Spec au-dessus, Résultat dessous, pied d'îlot avec l'application. */
@Component({
  selector: 'app-merge-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon, CodeEditor],
  host: { style: 'display: contents' },
  template: `
    @if (plan(); as p) {
      <div class="tabs"><div class="tab" role="tab" aria-selected="true"><app-ic name="merge" [size]="14" /><span class="tab-name">Fusion · {{ specName() }}</span></div></div>
      @if (sync.error(); as e) {
        <div class="banner err merge-banner" role="alert"><app-ic name="alert" [size]="15" /><span><b>La comparaison a échoué</b> : {{ e }} Le plan affiché est celui de la comparaison précédente : relance la comparaison avant d'appliquer.</span></div>
      }
      @if (op(); as o) {
        <div class="view-head">
          <span [class]="methodClass(o.method)" style="width: auto; font-size: 11px">{{ o.method }}</span>
          <h1 class="view-title mono" style="font-size: 13.5px; font-weight: 500">{{ o.path }}</h1>
          <span class="view-sub" [title]="subtitle()">{{ subtitle() }}</span>
          <span class="grow"></span>
          @if (sync.refs().length) {
            <span class="navgroup">
              <button class="icon-btn sm" (click)="sync.next(-1)" aria-label="Conflit précédent" [title]="'Conflit précédent (' + keys.previous + ')'"><app-ic name="chev-up" [size]="15" /></button>
              <span>{{ position() }}</span>
              <button class="icon-btn sm" (click)="sync.next(1)" aria-label="Conflit suivant" [title]="'Conflit suivant (' + keys.next + ')'"><app-ic name="chev-down" [size]="15" /></button>
            </span>
          }
          <button class="btn" [class.ghost]="!showBase()" [disabled]="view()?.base == null" [title]="baseTitle()" [attr.aria-pressed]="showBase()" (click)="sync.showBase.set(!sync.showBase())">
            <app-ic name="eye" [size]="14" />Base
          </button>
        </div>
        @if (listed()) {
          <div class="chg-list" role="group" aria-label="Changements de l'opération">
            @for (c of o.changes; track c.id) {
              <button class="chg" [attr.aria-pressed]="sync.selection()?.changeId === c.id" [title]="c.reason" (click)="sync.select({ key: o.key, changeId: c.id })">
                @if (c.kind === 'conflict') {
                  <span class="dot" [class.warn]="!arbitrated(c)" [class.good]="arbitrated(c)"></span>
                }
                {{ c.label }}
                <span class="tag" [class.spec]="c.kind === 'applied'" [class.new]="c.kind === 'kept'" [class.warn]="c.kind === 'conflict' && !arbitrated(c)" [class.good]="c.kind === 'conflict' && arbitrated(c)">{{ tag(c) }}</span>
              </button>
            }
          </div>
        }
        @if (sync.viewError(); as e) {
          <div class="banner err merge-banner" role="alert"><app-ic name="alert" [size]="15" /><span>Impossible de recomposer le résultat : {{ e }}</span></div>
        }
        @if (view(); as v) {
          <div class="merge">
            <div class="merge-top" [class.with-base]="showBase() && v.base !== null">
              <div class="mpane">
                <div class="mpane-head">
                  <span class="sw o"></span><b>Équipe</b><span class="sub">collection</span><span class="grow"></span>
                  @if (canChoose('team')) {
                    <button class="btn ghost" style="height: 24px" [attr.aria-pressed]="active() === 'team'" (click)="pick('team')">
                      @if (active() === 'team') {
                        <app-ic name="check" [size]="13" />
                      }
                      {{ labels.team }}
                    </button>
                  }
                </div>
                <app-code-editor class="mcode" [readonly]="true" language="yaml" label="Fichier de l'équipe" [value]="text(v.ours)" [marks]="marks().ours" [folds]="folds().ours" [reveal]="reveal().ours" />
              </div>
              @if (showBase() && v.base !== null) {
                <div class="mpane">
                  <div class="mpane-head"><span class="sw b"></span><b>Base</b><span class="sub">{{ baseSub() }}</span></div>
                  <app-code-editor class="mcode" [readonly]="true" language="yaml" label="Base" [value]="text(v.base)" [marks]="marks().base" [folds]="folds().base" [reveal]="reveal().base" />
                </div>
              }
              <div class="mpane">
                <div class="mpane-head">
                  <span class="sw t"></span><b>Spec</b><span class="sub" [title]="p.source">{{ specName() }} {{ versionOf(p.to.version) }}</span><span class="grow"></span>
                  @if (canChoose('spec')) {
                    <button class="btn ghost" style="height: 24px" [attr.aria-pressed]="active() === 'spec'" (click)="pick('spec')">
                      @if (active() === 'spec') {
                        <app-ic name="check" [size]="13" />
                      }
                      {{ labels.spec }}
                    </button>
                  }
                </div>
                <app-code-editor class="mcode" [readonly]="true" language="yaml" label="Spec" [value]="text(v.theirs)" [marks]="marks().theirs" [folds]="folds().theirs" [reveal]="reveal().theirs" />
              </div>
            </div>
            <div class="mpane result">
              <div class="mpane-head">
                <span class="sw r"></span><b>Résultat</b><span class="sub">{{ o.file ?? 'nouveau fichier' }}</span><span class="grow"></span>
                @if (chip(); as c) {
                  <span class="state-chip" [class.good]="c.good" [class.warn]="!c.good"><app-ic [name]="c.good ? 'check' : 'alert'" [size]="12" />{{ c.text }}</span>
                }
              </div>
              @if (editing(); as c) {
                <div class="edit-box">
                  <label class="edit-label" for="edit-value">Valeur saisie à la main · <b>{{ c.label }}</b></label>
                  @if (isCode(c)) {
                    <app-code-editor #code class="edit-code" id="edit-value" [language]="language()" [label]="'Valeur de ' + c.label" [value]="editText()" (valueChange)="sync.setEditValue(c.id, $event)" />
                  } @else {
                    <input #line id="edit-value" class="input mono" type="text" spellcheck="false" autocomplete="off" [value]="editText()" (input)="onLine(c, $any($event.target))" />
                  }
                  @if (isAddress(c)) {
                    <span class="edit-hint" [class.is-dropped]="queryDropped()" aria-live="polite">{{ queryDropped() ? queryDroppedNote : addressHint }}</span>
                  }
                </div>
              }
              <app-code-editor class="mcode" [readonly]="true" language="yaml" label="Résultat" [value]="text(v.result)" [marks]="marks().result" [folds]="folds().result" [lenses]="lenses()" [reveal]="reveal().result" (lensPicked)="onLens($event)" />
            </div>
          </div>
        } @else if (!sync.viewError()) {
          <div class="empty"><span class="spinner"></span><p>Chargement de l'opération…</p></div>
        }
      } @else {
        <div class="empty">
          <span class="empty-ic" [style.background]="'var(--good-soft)'" [style.color]="'var(--good)'"><app-ic name="check" [size]="20" /></span>
          <h2>{{ upToDate() ? 'Spec à jour' : 'Aucun conflit à arbitrer' }}</h2>
          <p>{{ upToDate() ? 'La collection suit déjà la spec : rien à appliquer.' : 'Choisis une opération dans la barre latérale pour voir ce que la synchro va écrire.' }}</p>
        </div>
      }
      @if (!upToDate()) {
        <div class="island-foot">
          <span class="foot-state" [class.warn]="sync.conflictsLeft()" [class.good]="!sync.conflictsLeft()">
            <span class="badge-ic"><app-ic [name]="sync.conflictsLeft() ? 'alert' : 'check'" [size]="14" /></span>
            @if (sync.conflictsLeft()) {
              {{ plural(sync.conflictsLeft(), 'conflit') }} à arbitrer avant d'appliquer
            } @else {
              Tout est arbitré, prêt à appliquer
            }
          </span>
          <span class="foot-note">Rien n'est écrit sur le disque avant ta validation. La base .oc-sync est réécrite en dernier.</span>
          <span class="grow"></span>
          <button class="btn lg ghost" [disabled]="sync.applying()" (click)="sync.abandon()">Annuler</button>
          <button class="btn-primary lg" [class.is-sending]="sync.applying()" [disabled]="sync.conflictsLeft() > 0 || sync.applying()" (click)="sync.apply()">
            @if (sync.applying()) {
              <span class="spinner"></span>Application…
            } @else {
              {{ label() }}
            }
          </button>
        </div>
      }
    }
  `,
  styles: `
    .mcode { flex: 1 1 0; min-height: 0; }
    .empty { flex: 1; height: auto; }
    .edit-box { flex-shrink: 0; display: flex; flex-direction: column; gap: 6px; padding: 8px 12px 10px; border-bottom: 1px solid var(--line); }
    .edit-label { font-size: 12px; color: var(--muted); }
    .edit-label b { color: var(--ink); font-weight: 500; }
    .edit-hint { font-size: 11.5px; color: var(--faint); }
    .edit-hint.is-dropped { color: var(--ink); }
    .edit-code { height: 132px; border: 1px solid var(--line); border-radius: 6px; background: var(--sunken); }
    .edit-code:focus-within { border-color: var(--accent-line); box-shadow: 0 0 0 3px var(--accent-soft); }
    .merge-banner { margin: 10px 12px 0; }
    .chg-list { flex-shrink: 0; display: flex; gap: 4px; padding: 6px 12px; border-bottom: 1px solid var(--line); overflow-x: auto; scrollbar-width: none; }
    .chg { flex-shrink: 0; height: 24px; display: inline-flex; align-items: center; gap: 6px; padding: 0 6px 0 8px; border-radius: 6px; color: var(--muted); font-size: 12px; white-space: nowrap; }
    .chg:hover { background: var(--hover); color: var(--ink); }
    .chg[aria-pressed='true'] { background: var(--accent-soft); color: var(--ink); }
  `,
})
export class MergeEditor {
  protected readonly sync = inject(SyncStore);
  protected readonly methodClass = methodClass;
  protected readonly plural = plural;
  protected readonly versionOf = versionOf;
  protected readonly labels = CHOICE_LABELS;
  protected readonly addressHint = ADDRESS_HINT;
  protected readonly queryDroppedNote = QUERY_DROPPED;
  protected readonly queryDropped = signal(false);
  protected readonly keys = { previous: shortcutLabel('alt+arrowup'), next: shortcutLabel('alt+arrowdown') };
  private readonly code = viewChild<CodeEditor>('code');
  private readonly line = viewChild<ElementRef<HTMLInputElement>>('line');

  protected readonly plan = this.sync.plan;
  protected readonly op = this.sync.selectedOp;
  protected readonly view = this.sync.opView;
  protected readonly showBase = this.sync.showBase;
  protected readonly changes = computed(() => this.op()?.changes ?? []);
  protected readonly upToDate = computed(() => {
    const plan = this.plan();
    return !!plan && isUpToDate(plan);
  });
  protected readonly listed = computed(() => this.changes().length > 1 || this.changes().some((c) => c.kind !== 'conflict'));
  protected readonly active = computed<Choice | null>(() => {
    const change = this.sync.selectedChange();
    return (change && this.sync.decisions().choices[change.id]?.choice) || null;
  });
  protected readonly editing = computed(() => {
    const change = this.sync.selectedChange();
    return change && this.active() === 'edit' ? change : null;
  });
  protected readonly editText = computed(() => {
    const change = this.editing();
    return (change && this.sync.decisions().choices[change.id]?.value) ?? '';
  });
  protected readonly language = computed(() => codeLanguage(this.editText()));
  protected readonly specName = computed(() => sourceName(this.plan()?.source ?? ''));
  protected readonly label = computed(() => {
    const plan = this.plan();
    return plan ? applyLabel(plan, this.sync.decisions()) : '';
  });
  protected readonly subtitle = computed(() => {
    const change = this.sync.selectedChange();
    return change ? `${change.label} · ${change.reason}` : (this.op()?.name ?? '');
  });
  protected readonly position = computed(() => {
    const index = this.sync.refs().findIndex((r) => r.changeId === this.sync.selection()?.changeId);
    const total = this.sync.refs().length;
    return index < 0 ? plural(total, 'conflit') : `Conflit ${index + 1} sur ${total}`;
  });
  protected readonly baseSub = computed(() => versionOf(this.plan()?.from?.version ?? ''));
  protected readonly baseTitle = computed(() => {
    const [plan, op, view] = [this.plan(), this.op(), this.view()];
    return plan && op && view && view.base == null ? noBaseHint(plan, op) : 'Afficher la base';
  });
  protected readonly chip = computed(() => {
    const change = this.sync.selectedChange();
    if (!change || change.kind !== 'conflict') return null;
    const choice = this.active();
    return choice && isArbitrated(this.sync.decisions(), change.id)
      ? { good: true, text: CHOICE_LABELS[choice] }
      : { good: false, text: "Non arbitré : la version de l'équipe reste en place" };
  });
  protected readonly marks = computed(() => {
    const view = this.view();
    const changes = this.changes();
    if (!view) return { ours: [], theirs: [], base: [], result: [] };
    return {
      ours: paneMarks(view, changes, 'ours'),
      theirs: paneMarks(view, changes, 'theirs'),
      base: paneMarks(view, changes, 'base'),
      result: resultMarks(view, changes, this.sync.decisions()),
    };
  });
  protected readonly folds = computed(() => {
    const view = this.view();
    const side = (name: PaneSide, text: string | null) => (view && text !== null ? foldRanges(lineCount(text), hunkRanges(view, name)) : []);
    return { ours: side('ours', view?.ours ?? null), theirs: side('theirs', view?.theirs ?? null), base: side('base', view?.base ?? null), result: side('result', view?.result ?? null) };
  });
  protected readonly lenses = computed(() => {
    const view = this.view();
    return view ? lensesFor(view, this.changes(), this.sync.decisions()) : [];
  });
  protected readonly reveal = computed(() => {
    const view = this.view();
    const id = this.sync.selection()?.changeId;
    const hunk = (id ? view?.hunks.find((h) => h.changeId === id) : view?.hunks[0]) ?? null;
    const at = (side: PaneSide) => {
      const range = hunk?.[side] ?? null;
      return range ? { line: range[0] } : null;
    };
    return { ours: at('ours'), theirs: at('theirs'), base: at('base'), result: at('result') };
  });

  constructor() {
    effect(() => {
      this.queryDropped.set(false);
      if (!this.editing()) return;
      setTimeout(() => (this.code() ?? this.line()?.nativeElement)?.focus());
    });
  }

  protected text(value: string) {
    return trimEnd(value);
  }

  protected arbitrated(change: SyncChange) {
    return isArbitrated(this.sync.decisions(), change.id);
  }

  protected tag(change: SyncChange) {
    return change.kind === 'conflict' && this.arbitrated(change) ? 'arbitré' : kindTag(change.kind);
  }

  protected isCode(change: SyncChange) {
    return isCodeField(change);
  }

  protected isAddress(change: SyncChange) {
    return change.field === 'url';
  }

  protected onLine(change: SyncChange, input: HTMLInputElement) {
    const value = this.isAddress(change) ? withoutQuery(input.value) : input.value;
    if (value !== input.value) {
      input.value = value;
      this.queryDropped.set(true);
    }
    this.sync.setEditValue(change.id, value);
  }

  protected canChoose(choice: Choice) {
    const change = this.sync.selectedChange();
    return change?.kind === 'conflict' && change.choices.includes(choice);
  }

  protected pick(choice: Choice) {
    const change = this.sync.selectedChange();
    const op = this.op();
    if (change && op) this.sync.choose(change, op.key, choice);
  }

  protected onLens({ id, key }: { id: string; key: string }) {
    const op = this.op();
    const change = this.changes().find((c) => c.id === id);
    if (change && op) this.sync.choose(change, op.key, key as Choice);
  }
}
