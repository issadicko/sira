import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, effect, inject, input, output } from '@angular/core';
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { json } from '@codemirror/lang-json';
import { xml } from '@codemirror/lang-xml';
import { bracketMatching, codeFolding, foldGutter, foldKeymap, indentOnInput, indentUnit, syntaxHighlighting } from '@codemirror/language';
import { search, searchKeymap } from '@codemirror/search';
import { Annotation, Compartment, EditorState, Extension } from '@codemirror/state';
import { EditorView, ViewUpdate, drawSelection, highlightSpecialChars, keymap, lineNumbers } from '@codemirror/view';
import { tagHighlighter, tags as t } from '@lezer/highlight';

export type CodeLanguage = 'json' | 'xml' | 'text';

const LANGUAGES: Record<CodeLanguage, Extension> = { json: json(), xml: xml(), text: [] };
/** Raccourcis globaux de l'application (⌘↵ envoie) que l'éditeur ne doit pas capturer. */
const APP_SHORTCUTS = new Set(['Mod-Enter']);
const external = Annotation.define<boolean>();

const HIGHLIGHTER = tagHighlighter([
  { tag: t.propertyName, class: 't-key' },
  { tag: [t.string, t.attributeValue], class: 't-str' },
  { tag: [t.number, t.character], class: 't-num' },
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
  '.cm-scroller': { font: '12.5px/20px var(--font-mono)', fontVariantLigatures: 'none', fontFeatureSettings: '"calt" 0, "liga" 0' },
  '.cm-content': { padding: '6px 0 16px', caretColor: 'var(--accent)' },
  '.cm-line': { padding: '0 20px 0 2px' },
  '.cm-gutters': { backgroundColor: 'transparent', color: 'var(--faint)', border: 'none' },
  '.cm-lineNumbers .cm-gutterElement': { minWidth: '44px', padding: '0 0 0 8px', fontVariantNumeric: 'tabular-nums' },
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
  '.cm-panels': { backgroundColor: 'var(--raised)', color: 'var(--ink)' },
  '.cm-panels-top': { borderBottom: '1px solid var(--line)' },
  '.cm-panel.cm-search': { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '4px', padding: '6px 40px 6px 8px', font: '12px var(--font-ui)' },
  '.cm-panel.cm-search:has([name=replace])::after': { content: '""', flexBasis: '100%', order: '1' },
  '.cm-panel.cm-search [name=replace], .cm-panel.cm-search [name=replaceAll]': { order: '2' },
  '.cm-panel.cm-search br': { display: 'none' },
  '.cm-panel.cm-search input, .cm-panel.cm-search button, .cm-panel.cm-search label': { margin: '0' },
  '.cm-textfield': { flex: '1 1 140px', maxWidth: '280px', height: '26px', padding: '0 8px', border: '1px solid var(--line)', borderRadius: '6px', backgroundColor: 'var(--sunken)', color: 'var(--ink)', font: '12px var(--font-mono)', fontVariantLigatures: 'none', outline: 'none' },
  '.cm-textfield:focus': { borderColor: 'var(--accent-line)', boxShadow: '0 0 0 3px var(--accent-soft)' },
  '.cm-button, .cm-panel.cm-search [name=close]': { height: '24px', padding: '0 8px', border: 'none', borderRadius: '6px', backgroundImage: 'none', backgroundColor: 'transparent', color: 'var(--muted)', font: '500 12px var(--font-ui)', cursor: 'pointer' },
  '.cm-button:hover, .cm-button:active, .cm-panel.cm-search [name=close]:hover': { backgroundImage: 'none', backgroundColor: 'var(--hover)', color: 'var(--ink)' },
  '.cm-panel.cm-search [name=next], .cm-panel.cm-search [name=prev], .cm-panel.cm-search [name=close]': { position: 'relative', width: '24px', padding: '0', fontSize: '0' },
  '.cm-panel.cm-search [name=next]::before, .cm-panel.cm-search [name=prev]::before, .cm-panel.cm-search [name=close]::before': { content: '""', position: 'absolute', inset: '0', backgroundColor: 'currentColor' },
  '.cm-panel.cm-search [name=next]::before': { mask: ICONS.next, WebkitMask: ICONS.next },
  '.cm-panel.cm-search [name=prev]::before': { mask: ICONS.prev, WebkitMask: ICONS.prev },
  '.cm-panel.cm-search [name=close]::before': { mask: ICONS.close, WebkitMask: ICONS.close },
  '.cm-panel.cm-search [name=close]': { position: 'absolute', top: '7px', right: '8px' },
  '.cm-panel.cm-search label': { position: 'relative', height: '24px', display: 'inline-flex', alignItems: 'center', padding: '0 7px', borderRadius: '6px', color: 'var(--muted)', fontSize: '12px', cursor: 'pointer' },
  '.cm-panel.cm-search label:hover': { backgroundColor: 'var(--hover)', color: 'var(--ink)' },
  '.cm-panel.cm-search label:has(:checked)': { backgroundColor: 'var(--accent-soft)', color: 'var(--accent)' },
  '.cm-panel.cm-search label:has(:focus-visible)': { outline: '2px solid var(--accent-line)', outlineOffset: '1px' },
  '.cm-panel.cm-search input[type=checkbox]': { position: 'absolute', width: '0', height: '0', margin: '0', opacity: '0' },
});

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
  keymap.of([...closeBracketsKeymap, ...defaultKeymap.filter((b) => !APP_SHORTCUTS.has(b.key ?? '')), ...searchKeymap, ...historyKeymap, ...foldKeymap, indentWithTab]),
  THEME,
];

/** Éditeur CodeMirror 6 : JSON, XML ou texte, éditable ou en lecture seule, thème tiré des variables CSS. */
@Component({
  selector: 'app-code-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: '',
  styles: ':host { display: block; min-height: 0; overflow: hidden; }',
})
export class CodeEditor {
  readonly value = input('');
  readonly language = input<CodeLanguage>('text');
  readonly readonly = input(false);
  readonly wrap = input(false);
  readonly label = input('');
  readonly valueChange = output<string>();

  private readonly lang = new Compartment();
  private readonly mode = new Compartment();
  private readonly wrapping = new Compartment();
  private current = '';
  private readonly view = new EditorView({
    parent: inject<ElementRef<HTMLElement>>(ElementRef).nativeElement,
    state: EditorState.create({
      extensions: [BASE, this.lang.of([]), this.mode.of([]), this.wrapping.of([]), EditorView.updateListener.of((u) => this.changed(u))],
    }),
  });

  constructor() {
    inject(DestroyRef).onDestroy(() => this.view.destroy());
    effect(() => this.setValue(this.value()));
    effect(() => this.configure(this.lang, LANGUAGES[this.language()]));
    effect(() => this.configure(this.mode, [this.readonly() ? EditorState.readOnly.of(true) : history(), EditorView.contentAttributes.of({ 'aria-label': this.label() })]));
    effect(() => this.configure(this.wrapping, this.wrap() ? EditorView.lineWrapping : []));
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

  private changed(update: ViewUpdate) {
    if (!update.docChanged || update.transactions.some((tr) => tr.annotation(external))) return;
    this.current = update.state.doc.toString();
    this.valueChange.emit(this.current);
  }
}
