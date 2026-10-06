import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, effect, inject, input, output, signal } from '@angular/core';
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { json } from '@codemirror/lang-json';
import { xml } from '@codemirror/lang-xml';
import { StreamLanguage, bracketMatching, codeFolding, foldGutter, foldKeymap, indentOnInput, indentUnit, syntaxHighlighting } from '@codemirror/language';
import { search, searchKeymap } from '@codemirror/search';
import { Annotation, Compartment, EditorState, Extension, Range, RangeSet, StateEffect, StateField } from '@codemirror/state';
import { Decoration, DecorationSet, EditorView, GutterMarker, ViewUpdate, WidgetType, drawSelection, gutterLineClass, highlightActiveLine, highlightActiveLineGutter, highlightSpecialChars, keymap, lineNumbers } from '@codemirror/view';
import { tagHighlighter, tags as t } from '@lezer/highlight';

import type { FoldRange, Lens, LineMark } from '../core/sync';

export type CodeLanguage = 'json' | 'xml' | 'yaml' | 'javascript' | 'graphql' | 'text';

const YAML = StreamLanguage.define({
  name: 'yaml',
  token(stream) {
    if (stream.eatSpace()) return null;
    if (stream.match(/^"(?:[^"\\]|\\.)*"(?=\s*:)/)) return 'propertyName';
    if (stream.match(/^"(?:[^"\\]|\\.)*"/) || stream.match(/^'[^']*'/)) return 'string';
    if (stream.match(/^-?\d+(?:\.\d+)?(?=[\s,]|$)/)) return 'number';
    if (stream.match(/^(?:true|false|null)(?=[\s,]|$)/)) return 'keyword';
    if (stream.match(/^[\w.$-]+(?=:(?:\s|$))/)) return 'propertyName';
    if (stream.match(/^#.*/)) return 'comment';
    stream.next();
    return null;
  },
});

const JS_KEYWORDS = new Set([
  'async', 'await', 'break', 'case', 'catch', 'class', 'const', 'continue', 'default', 'delete', 'do', 'else', 'export', 'extends',
  'false', 'finally', 'for', 'function', 'if', 'import', 'in', 'instanceof', 'let', 'new', 'null', 'of', 'return', 'switch', 'this',
  'throw', 'true', 'try', 'typeof', 'undefined', 'var', 'while', 'yield',
]);

/** Coloration légère des scripts : mots-clés, chaînes, nombres, commentaires. */
const JAVASCRIPT = StreamLanguage.define<{ block: boolean }>({
  name: 'javascript',
  startState: () => ({ block: false }),
  token(stream, state) {
    if (state.block) {
      if (stream.match(/^.*?\*\//)) state.block = false;
      else stream.skipToEnd();
      return 'comment';
    }
    if (stream.eatSpace()) return null;
    if (stream.match('//')) {
      stream.skipToEnd();
      return 'comment';
    }
    if (stream.match('/*')) {
      if (!stream.match(/^.*?\*\//)) {
        state.block = true;
        stream.skipToEnd();
      }
      return 'comment';
    }
    if (stream.match(/^"(?:[^"\\]|\\.)*"?/) || stream.match(/^'(?:[^'\\]|\\.)*'?/) || stream.match(/^`(?:[^`\\]|\\.)*`?/)) return 'string';
    if (stream.match(/^\d[\d_]*(?:\.\d+)?/)) return 'number';
    if (stream.match(/^[A-Za-z_$][\w$]*/)) return JS_KEYWORDS.has(stream.current()) ? 'keyword' : null;
    stream.next();
    return null;
  },
});

const LANGUAGES: Record<Exclude<CodeLanguage, 'graphql'>, Extension> = { json: json(), xml: xml(), yaml: YAML, javascript: JAVASCRIPT, text: [] };
/** Raccourcis globaux de l'application (⌘↵ envoie) que l'éditeur ne doit pas capturer. */
const APP_SHORTCUTS = new Set(['Mod-Enter']);
const external = Annotation.define<boolean>();

const HIGHLIGHTER = tagHighlighter([
  { tag: t.propertyName, class: 't-key' },
  { tag: [t.string, t.attributeValue], class: 't-str' },
  { tag: [t.number, t.character, t.variableName], class: 't-num' },
  { tag: [t.bool, t.null, t.keyword, t.tagName], class: 't-kw' },
  { tag: [t.comment, t.meta], class: 't-com' },
]);

const PHRASES = {
  Find: 'Rechercher',
  Replace: 'Remplacer',
  next: 'Suivant',
  previous: 'Précédent',
  all: 'Tout',
  'match case': 'Casse',
  regexp: 'Regex',
  'by word': 'Mot entier',
  replace: 'Remplacer',
  'replace all': 'Tout remplacer',
  close: 'Fermer',
  'current match': 'Correspondance',
  'on line': 'ligne',
  'replaced match on line $': 'Remplacé ligne $',
  'replaced $ matches': '$ remplacements',
  'Folded lines': 'Lignes repliées',
  'Unfolded lines': 'Lignes dépliées',
  'Control character': 'Caractère de contrôle',
  'Go to line': 'Aller à la ligne',
  go: 'Aller',
  to: 'à',
  'Selection deleted': 'Sélection supprimée',
  'folded code': 'code replié',
  unfold: 'Déplier',
  'Fold line': 'Replier la ligne',
  'Unfold line': 'Déplier la ligne',
};

const icon = (path: string) =>
  `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23000' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='${path}'/%3E%3C/svg%3E") center / 14px no-repeat`;
const ICONS = { next: icon('m6 9 6 6 6-6'), prev: icon('m18 15-6-6-6 6'), close: icon('M18 6 6 18M6 6l12 12') };

const THEME = EditorView.theme({
  '&': { height: '100%', color: 'var(--ink)', backgroundColor: 'transparent' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { font: 'var(--code-size)/1.6 var(--code-font)', fontVariantLigatures: 'none', fontFeatureSettings: '"calt" 0, "liga" 0' },
  '.cm-content': { padding: '6px 0 16px', caretColor: 'var(--accent)' },
  '.cm-line': { padding: '0 20px 0 6px' },
  '.cm-gutters': { backgroundColor: 'transparent', color: 'var(--faint)', border: 'none', borderRight: '1px solid var(--line)' },
  '.cm-lineNumbers .cm-gutterElement': { minWidth: '44px', padding: '0 0 0 8px', fontVariantNumeric: 'tabular-nums' },
  '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'transparent' },
  '&.cm-focused .cm-activeLine': { backgroundColor: 'var(--active-line)' },
  '&.cm-focused .cm-activeLineGutter': { color: 'var(--muted)' },
  '.cm-foldGutter .cm-gutterElement': { width: '18px', display: 'flex', alignItems: 'center', justifyContent: 'center' },
  '.cm-fold-marker': { width: '16px', height: '18px', display: 'grid', placeItems: 'center', borderRadius: '4px', padding: '0', opacity: '0' },
  '&:hover .cm-fold-marker, .cm-fold-marker.is-folded': { opacity: '1' },
  '.cm-fold-marker:hover': { color: 'var(--ink)', backgroundColor: 'var(--hover)' },
  '.cm-foldPlaceholder': { display: 'inline-block', margin: '0 2px', padding: '0 5px', border: 'none', borderRadius: '4px', backgroundColor: 'var(--raised)', color: 'var(--muted)' },
  '.cm-foldPlaceholder:hover': { color: 'var(--ink)' },
  '.cm-cursor, .cm-dropCursor': { borderLeft: '2px solid var(--accent)', marginLeft: '-1px' },
  '.cm-selectionBackground': { background: 'var(--accent-soft)' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground': { background: 'var(--accent-line)' },
  '&.cm-focused .cm-matchingBracket': { backgroundColor: 'var(--accent-soft)', outline: '1px solid var(--accent-line)', borderRadius: '4px' },
  '&.cm-focused .cm-nonmatchingBracket': { backgroundColor: 'var(--bad-soft)', outline: '1px solid var(--bad-strong)', borderRadius: '4px' },
  '.cm-searchMatch': { backgroundColor: 'var(--accent-soft)', outline: '1px solid var(--accent-line)', borderRadius: '4px' },
  '.cm-searchMatch-selected': { backgroundColor: 'var(--accent-line)' },
  '.cm-specialChar': { color: 'var(--bad)' },
  '.cm-tooltip': { border: '1px solid var(--line-strong)', borderRadius: '8px', backgroundColor: 'var(--pop)', color: 'var(--ink)', boxShadow: 'var(--shadow)', overflow: 'hidden' },
  '.cm-tooltip-autocomplete > ul': { maxHeight: '230px', padding: '3px', font: 'calc(12 * var(--px)) var(--font-mono)', fontVariantLigatures: 'none' },
  '.cm-tooltip-autocomplete > ul > li': { padding: '3px 8px', borderRadius: '5px', lineHeight: '1.5', color: 'var(--ink)' },
  '.cm-tooltip-autocomplete > ul > li[aria-selected]': { backgroundColor: 'var(--accent-soft)', color: 'var(--ink)' },
  '.cm-completionMatchedText': { textDecoration: 'none', color: 'var(--accent)', fontWeight: '600' },
  '.cm-completionDetail': { marginLeft: '14px', color: 'var(--faint)', fontStyle: 'normal' },
  '.cm-completionInfo': { maxWidth: '320px', padding: '8px 10px', font: 'calc(12 * var(--px)) var(--font-ui)', lineHeight: '1.45', color: 'var(--muted)', whiteSpace: 'pre-wrap' },
  '.cm-tooltip.cm-completionInfo': { border: '1px solid var(--line-strong)', backgroundColor: 'var(--pop)' },
  '.cm-diagnostic': { padding: '5px 10px', borderLeft: '3px solid var(--bad)', font: 'calc(12 * var(--px)) var(--font-ui)', lineHeight: '1.45' },
  '.cm-diagnostic-warning': { borderLeftColor: 'var(--warn)' },
  '.cm-diagnostic-info': { borderLeftColor: 'var(--info)' },
  '.cm-lintRange-error, .cm-lintRange-warning, .cm-lintRange-info': { backgroundImage: 'none', textDecorationLine: 'underline', textDecorationStyle: 'wavy', textUnderlineOffset: '3px' },
  '.cm-lintRange-error': { textDecorationColor: 'var(--bad)' },
  '.cm-lintRange-warning': { textDecorationColor: 'var(--warn)' },
  '.cm-lintRange-info': { textDecorationColor: 'var(--info)' },
  '.cm-panels': { backgroundColor: 'var(--raised)', color: 'var(--ink)' },
  '.cm-panels-top': { borderBottom: '1px solid var(--line)' },
  '.cm-panel.cm-search': { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '4px', padding: '6px 40px 6px 8px', font: 'calc(12 * var(--px)) var(--font-ui)' },
  '.cm-panel.cm-search:has([name=replace])::after': { content: '""', flexBasis: '100%', order: '1' },
  '.cm-panel.cm-search [name=replace], .cm-panel.cm-search [name=replaceAll]': { order: '2' },
  '.cm-panel.cm-search br': { display: 'none' },
  '.cm-panel.cm-search input, .cm-panel.cm-search button, .cm-panel.cm-search label': { margin: '0' },
  '.cm-textfield': { flex: '1 1 140px', maxWidth: '280px', height: '26px', padding: '0 8px', border: '1px solid var(--line)', borderRadius: '6px', backgroundColor: 'var(--sunken)', color: 'var(--ink)', font: 'calc(12 * var(--px)) var(--font-mono)', fontVariantLigatures: 'none', outline: 'none' },
  '.cm-textfield:focus': { borderColor: 'var(--accent-line)', boxShadow: '0 0 0 3px var(--accent-soft)' },
  '.cm-button, .cm-panel.cm-search [name=close]': { height: '24px', padding: '0 8px', border: 'none', borderRadius: '6px', backgroundImage: 'none', backgroundColor: 'transparent', color: 'var(--muted)', font: '500 calc(12 * var(--px)) var(--font-ui)', cursor: 'default' },
  '.cm-button:hover, .cm-button:active, .cm-panel.cm-search [name=close]:hover': { backgroundImage: 'none', backgroundColor: 'var(--hover)', color: 'var(--ink)' },
  '.cm-panel.cm-search [name=next], .cm-panel.cm-search [name=prev], .cm-panel.cm-search [name=close]': { position: 'relative', width: '24px', padding: '0', fontSize: '0' },
  '.cm-panel.cm-search [name=next]::before, .cm-panel.cm-search [name=prev]::before, .cm-panel.cm-search [name=close]::before': { content: '""', position: 'absolute', inset: '0', backgroundColor: 'currentColor' },
  '.cm-panel.cm-search [name=next]::before': { mask: ICONS.next, WebkitMask: ICONS.next },
  '.cm-panel.cm-search [name=prev]::before': { mask: ICONS.prev, WebkitMask: ICONS.prev },
  '.cm-panel.cm-search [name=close]::before': { mask: ICONS.close, WebkitMask: ICONS.close },
  '.cm-panel.cm-search [name=close]': { position: 'absolute', top: '7px', right: '8px' },
  '.cm-panel.cm-search label': { position: 'relative', height: '24px', display: 'inline-flex', alignItems: 'center', padding: '0 7px', borderRadius: '6px', color: 'var(--muted)', fontSize: 'calc(12 * var(--px))', cursor: 'default', userSelect: 'none' },
  '.cm-panel.cm-search label:hover': { backgroundColor: 'var(--hover)', color: 'var(--ink)' },
  '.cm-panel.cm-search label:has(:checked)': { backgroundColor: 'var(--accent-soft)', color: 'var(--accent)' },
  '.cm-panel.cm-search label:has(:focus-visible)': { outline: '2px solid var(--accent-line)', outlineOffset: '1px' },
  '.cm-panel.cm-search input[type=checkbox]': { position: 'absolute', width: '0', height: '0', margin: '0', opacity: '0' },
  '.cm-line.h-o, .cm-gutterElement.h-o': { backgroundColor: 'var(--accent-soft)' },
  '.cm-line.h-t, .cm-gutterElement.h-t': { backgroundColor: 'var(--info-soft)' },
  '.cm-line.h-b, .cm-gutterElement.h-b': { backgroundColor: 'var(--raised)' },
  '.cm-line.r-u, .cm-gutterElement.r-u': { backgroundColor: 'var(--warn-soft)' },
  '.cm-foldGutter .cm-gutterElement.r-o': { background: 'linear-gradient(var(--accent), var(--accent)) center / 3px 100% no-repeat' },
  '.cm-foldGutter .cm-gutterElement.r-t': { background: 'linear-gradient(var(--info), var(--info)) center / 3px 100% no-repeat' },
  '.cm-foldGutter .cm-gutterElement.r-b': { background: 'linear-gradient(var(--accent), var(--info)) center / 3px 100% no-repeat' },
  '.cm-foldGutter .cm-gutterElement.r-e': { background: 'linear-gradient(var(--good), var(--good)) center / 3px 100% no-repeat' },
  '.cm-foldGutter .cm-gutterElement.r-u': { background: 'linear-gradient(var(--warn), var(--warn)) center / 3px 100% no-repeat, var(--warn-soft)' },
  '.cm-fold-row': { display: 'flex', alignItems: 'center', gap: '6px', minHeight: '20px', padding: '0 20px 0 8px', backgroundColor: 'var(--raised)', boxShadow: '-64px 0 0 var(--raised)', color: 'var(--faint)', fontStyle: 'italic', cursor: 'default', userSelect: 'none' },
  '.cm-fold-row:hover': { color: 'var(--ink)' },
  '.cm-lens': { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '2px', minHeight: '26px', padding: '0 0 0 8px', font: 'calc(11.5 * var(--px)) var(--font-ui)', color: 'var(--faint)' },
  '.cm-lens-label': { marginRight: '6px', color: 'var(--muted)', fontWeight: '500' },
  '.cm-lens-label.is-open': { color: 'var(--warn)' },
  '.cm-lens button': { padding: '2px 6px', borderRadius: '4px', color: 'var(--muted)', cursor: 'default' },
  '.cm-lens button:hover': { color: 'var(--accent)', backgroundColor: 'var(--hover)' },
  '.cm-lens button[aria-pressed=true]': { color: 'var(--accent)', backgroundColor: 'var(--accent-soft)' },
  '.cm-lens i': { fontStyle: 'normal', color: 'var(--faint)' },
});

class LineClass extends GutterMarker {
  constructor(readonly cls: string) {
    super();
    this.elementClass = cls;
  }

  override eq(other: LineClass) {
    return other.cls === this.cls;
  }
}

class FoldRow extends WidgetType {
  constructor(
    readonly label: string,
    readonly expand: () => void,
  ) {
    super();
  }

  override eq(other: FoldRow) {
    return other.label === this.label;
  }

  override toDOM() {
    const row = document.createElement('div');
    row.className = 'cm-fold-row';
    row.setAttribute('role', 'button');
    row.tabIndex = 0;
    row.title = 'Déplier';
    row.innerHTML = `<svg class="ic" width="12" height="12" aria-hidden="true"><use href="#i-chev-right" /></svg>`;
    row.append(this.label);
    row.onclick = this.expand;
    row.onkeydown = (event) => {
      if (event.key === 'Enter' || event.key === ' ') this.expand();
    };
    return row;
  }
}

class LensBar extends WidgetType {
  constructor(
    readonly lens: Lens,
    readonly pick: (id: string, key: string) => void,
  ) {
    super();
  }

  override eq(other: LensBar) {
    const key = (l: Lens) => `${l.id}|${l.label}|${l.active}|${l.choices.map((c) => c.key).join()}`;
    return key(other.lens) === key(this.lens);
  }

  override toDOM() {
    const bar = document.createElement('div');
    bar.className = 'cm-lens';
    bar.setAttribute('role', 'group');
    bar.setAttribute('aria-label', 'Arbitrer ce conflit');
    if (this.lens.label) {
      const label = document.createElement('span');
      label.className = this.lens.active ? 'cm-lens-label' : 'cm-lens-label is-open';
      label.textContent = this.lens.label;
      bar.append(label);
    }
    this.lens.choices.forEach((choice, i) => {
      if (i) bar.append(Object.assign(document.createElement('i'), { textContent: '·' }));
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = choice.label;
      button.setAttribute('aria-pressed', String(this.lens.active === choice.key));
      button.onclick = () => this.pick(this.lens.id, choice.key);
      bar.append(button);
    });
    return bar;
  }
}

interface Layers {
  lines: DecorationSet;
  gutter: RangeSet<GutterMarker>;
}

const setLayers = StateEffect.define<Layers>();
const LAYERS = StateField.define<Layers>({
  create: () => ({ lines: Decoration.none, gutter: RangeSet.empty }),
  update(layers, tr) {
    for (const effect of tr.effects) if (effect.is(setLayers)) return effect.value;
    return tr.docChanged ? { lines: layers.lines.map(tr.changes), gutter: layers.gutter.map(tr.changes) } : layers;
  },
  provide: (field) => [EditorView.decorations.from(field, (l) => l.lines), gutterLineClass.from(field, (l) => l.gutter)],
});

const lineClasses = new Map<string, LineClass>();
const lineClass = (cls: string) => lineClasses.get(cls) ?? lineClasses.set(cls, new LineClass(cls)).get(cls)!;

function foldMarker(open: boolean): HTMLElement {
  const marker = document.createElement('span');
  marker.className = open ? 'cm-fold-marker' : 'cm-fold-marker is-folded';
  marker.title = open ? 'Replier' : 'Déplier';
  marker.innerHTML = `<svg class="ic" width="12" height="12" aria-hidden="true"><use href="#i-chev-${open ? 'down' : 'right'}" /></svg>`;
  return marker;
}

function foldPill(_view: EditorView, onclick: (event: Event) => void, hidden: number): HTMLElement {
  const pill = document.createElement('span');
  pill.className = 'cm-foldPlaceholder';
  pill.textContent = hidden > 1 ? `${hidden} lignes` : hidden === 1 ? '1 ligne' : '…';
  pill.title = 'Déplier';
  pill.onclick = onclick;
  return pill;
}

const BASE: Extension = [
  lineNumbers(),
  highlightActiveLine(),
  highlightActiveLineGutter(),
  foldGutter({ markerDOM: foldMarker }),
  codeFolding({ placeholderDOM: foldPill, preparePlaceholder: (state, range) => state.doc.lineAt(range.to).number - state.doc.lineAt(range.from).number - 1 }),
  highlightSpecialChars(),
  drawSelection(),
  indentOnInput(),
  bracketMatching(),
  closeBrackets(),
  search({ top: true }),
  syntaxHighlighting(HIGHLIGHTER),
  indentUnit.of('  '),
  EditorState.tabSize.of(2),
  EditorState.phrases.of(PHRASES),
  LAYERS,
  keymap.of([...closeBracketsKeymap, ...defaultKeymap.filter((b) => !APP_SHORTCUTS.has(b.key ?? '')), ...searchKeymap, ...historyKeymap, ...foldKeymap, indentWithTab]),
  THEME,
];

/** Éditeur CodeMirror 6 : JSON, XML ou texte, éditable ou en lecture seule, thème tiré des variables CSS. */
@Component({
  selector: 'app-code-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: '',
  styles: ':host { display: block; min-height: 0; overflow: hidden; background: var(--sunken); }',
})
export class CodeEditor {
  readonly value = input('');
  readonly language = input<CodeLanguage>('text');
  readonly readonly = input(false);
  readonly wrap = input(false);
  readonly label = input('');
  /** Lignes à teinter, par plage 1-based inclusive. */
  readonly marks = input<LineMark[]>([]);
  /** Plages de lignes à masquer derrière une ligne qui les résume ; un clic les déplie. */
  readonly folds = input<FoldRange[]>([]);
  /** Le schéma d'introspection (`{ __schema }`) que suivent l'autocomplétion et les diagnostics GraphQL. */
  readonly schema = input<unknown>(null);
  /** Barres de choix posées au-dessus d'une ligne. */
  readonly lenses = input<Lens[]>([]);
  /** Ligne à amener au centre de la vue ; un nouvel objet relance le défilement. */
  readonly reveal = input<{ line: number } | null>(null);
  readonly valueChange = output<string>();
  readonly lensPicked = output<{ id: string; key: string }>();

  private readonly lang = new Compartment();
  private readonly mode = new Compartment();
  private readonly wrapping = new Compartment();
  private current = '';
  private graphqlLoad = 0;
  private readonly expanded = signal<ReadonlySet<string>>(new Set());
  private readonly view = new EditorView({
    parent: inject<ElementRef<HTMLElement>>(ElementRef).nativeElement,
    state: EditorState.create({
      extensions: [BASE, this.lang.of([]), this.mode.of([]), this.wrapping.of([]), EditorView.updateListener.of((u) => this.changed(u))],
    }),
  });

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      this.graphqlLoad++;
      this.view.destroy();
    });
    effect(() => {
      this.setValue(this.value());
      this.decorate();
    });
    effect(() => this.scrollTo(this.reveal()));
    effect(() => {
      const language = this.language();
      if (language === 'graphql') void this.loadGraphql(this.schema());
      else this.configure(this.lang, LANGUAGES[language]);
    });
    effect(() => this.configure(this.mode, [this.readonly() ? EditorState.readOnly.of(true) : history(), EditorView.contentAttributes.of({ 'aria-label': this.label() })]));
    effect(() => this.configure(this.wrapping, this.wrap() ? EditorView.lineWrapping : []));
  }

  focus() {
    this.view.focus();
  }

  private async loadGraphql(schema: unknown) {
    const load = ++this.graphqlLoad;
    const { graphqlSupport } = await import('./graphql-language');
    const extension = await graphqlSupport(schema);
    if (load === this.graphqlLoad) this.configure(this.lang, extension);
  }

  private configure(compartment: Compartment, extension: Extension) {
    this.view.dispatch({ effects: compartment.reconfigure(extension) });
  }

  private setValue(value: string) {
    if (value === this.current) return;
    this.current = value;
    const doc = this.view.state.doc.toString();
    const max = Math.min(doc.length, value.length);
    let from = 0;
    while (from < max && doc.charCodeAt(from) === value.charCodeAt(from)) from++;
    let tail = 0;
    while (tail < max - from && doc.charCodeAt(doc.length - 1 - tail) === value.charCodeAt(value.length - 1 - tail)) tail++;
    if (from === doc.length && from === value.length) return;
    this.view.dispatch({ changes: { from, to: doc.length - tail, insert: value.slice(from, value.length - tail) }, annotations: external.of(true) });
  }

  private decorate() {
    const doc = this.view.state.doc;
    const line = (n: number) => doc.line(Math.min(Math.max(n, 1), doc.lines));
    const open = this.expanded();
    const lines: Range<Decoration>[] = [];
    const gutter: Range<GutterMarker>[] = [];
    for (const { from, to, cls } of this.marks()) {
      for (let n = Math.max(from, 1); n <= Math.min(to, doc.lines); n++) {
        lines.push(Decoration.line({ class: cls }).range(doc.line(n).from));
        gutter.push(lineClass(cls).range(doc.line(n).from));
      }
    }
    for (const fold of this.folds()) {
      const key = `${fold.from}-${fold.to}`;
      if (open.has(key) || fold.from > doc.lines) continue;
      const widget = new FoldRow(fold.label, () => this.expanded.update((set) => new Set([...set, key])));
      lines.push(Decoration.replace({ widget, block: true }).range(line(fold.from).from, line(fold.to).to));
    }
    for (const lens of this.lenses()) {
      const widget = new LensBar(lens, (id, key) => this.lensPicked.emit({ id, key }));
      lines.push(Decoration.widget({ widget, block: true, side: -1 }).range(line(lens.line).from));
    }
    this.view.dispatch({ effects: setLayers.of({ lines: Decoration.set(lines, true), gutter: RangeSet.of(gutter, true) }) });
  }

  private scrollTo(target: { line: number } | null) {
    if (!target) return;
    requestAnimationFrame(() => {
      const doc = this.view.state.doc;
      const at = doc.line(Math.min(Math.max(target.line, 1), doc.lines)).from;
      this.view.dispatch({ effects: EditorView.scrollIntoView(at, { y: 'center' }) });
    });
  }

  private changed(update: ViewUpdate) {
    if (!update.docChanged || update.transactions.some((tr) => tr.annotation(external))) return;
    this.current = update.state.doc.toString();
    this.valueChange.emit(this.current);
  }
}
