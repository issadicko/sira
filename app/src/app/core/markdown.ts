export type Inline =
  | { kind: 'text'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'strong'; children: Inline[] }
  | { kind: 'em'; children: Inline[] }
  | { kind: 'del'; children: Inline[] }
  | { kind: 'link'; href: string; children: Inline[] }
  | { kind: 'break' };

export type Align = 'left' | 'center' | 'right' | null;

export type Block =
  | { kind: 'heading'; level: number; children: Inline[] }
  | { kind: 'paragraph'; children: Inline[] }
  | { kind: 'code'; language: string; text: string }
  | { kind: 'quote'; children: Block[] }
  | { kind: 'list'; ordered: boolean; start: number; items: Block[][] }
  | { kind: 'table'; head: Inline[][]; align: Align[]; rows: Inline[][][] }
  | { kind: 'rule' };

const SAFE_LINK = /^(https?:\/\/|mailto:)/i;
const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)[^`]*$/;
const HEADING = /^ {0,3}(#{1,6})(?:\s+(.*?))?\s*#*\s*$/;
const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const BULLET = /^( {0,3})([-*+])\s+(.*)$/;
const ORDERED = /^( {0,3})(\d{1,9})[.)]\s+(.*)$/;
const QUOTE = /^ {0,3}>\s?(.*)$/;
const DIVIDER = /^\s*\|?\s*:?-{1,}:?\s*(\|\s*:?-{1,}:?\s*)*\|?\s*$/;

/** Adresse de lien montrée comme lien : seuls http, https et mailto, jamais `javascript:` ni une adresse relative. */
export function safeHref(href: string): string | null {
  const clean = href.trim();
  return SAFE_LINK.test(clean) ? clean : null;
}

export function parseMarkdown(source: string): Block[] {
  return blocks(source.replace(/\r\n?/g, '\n').split('\n'));
}

function blocks(lines: string[]): Block[] {
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const [mark, language] = [fence[1], fence[2]];
      const body: string[] = [];
      i++;
      while (i < lines.length && !closesFence(lines[i], mark)) body.push(lines[i++]);
      i++;
      out.push({ kind: 'code', language, text: body.join('\n') });
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      out.push({ kind: 'heading', level: heading[1].length, children: inline(heading[2] ?? '') });
      i++;
      continue;
    }
    if (RULE.test(line)) {
      out.push({ kind: 'rule' });
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) body.push(QUOTE.exec(lines[i++])![1]);
      out.push({ kind: 'quote', children: blocks(body) });
      continue;
    }
    if (BULLET.test(line) || ORDERED.test(line)) {
      const list = readList(lines, i);
      out.push(list.block);
      i = list.next;
      continue;
    }
    if (i + 1 < lines.length && line.includes('|') && DIVIDER.test(lines[i + 1]) && lines[i + 1].includes('-')) {
      const table = readTable(lines, i);
      if (table) {
        out.push(table.block);
        i = table.next;
        continue;
      }
    }
    const text: string[] = [];
    while (i < lines.length && lines[i].trim() && !startsBlock(lines[i])) text.push(lines[i++].trim());
    if (!text.length) text.push(lines[i++].trim());
    out.push({ kind: 'paragraph', children: inline(text.join('\n')) });
  }
  return out;
}

function closesFence(line: string, mark: string): boolean {
  const trimmed = line.trim();
  return trimmed.length >= mark.length && trimmed[0] === mark[0] && [...trimmed].every((c) => c === mark[0]);
}

function startsBlock(line: string): boolean {
  return FENCE.test(line) || HEADING.test(line) || RULE.test(line) || QUOTE.test(line) || BULLET.test(line) || ORDERED.test(line);
}

function readList(lines: string[], from: number): { block: Block; next: number } {
  const first = BULLET.exec(lines[from]) ?? ORDERED.exec(lines[from])!;
  const ordered = ORDERED.test(lines[from]) && !BULLET.test(lines[from]);
  const start = ordered ? Number(first[2]) : 1;
  const items: Block[][] = [];
  let i = from;
  while (i < lines.length) {
    const m = ordered ? ORDERED.exec(lines[i]) : BULLET.exec(lines[i]);
    if (!m) break;
    const body = [m[3]];
    i++;
    while (i < lines.length) {
      const line = lines[i];
      if (!line.trim()) {
        const following = lines[i + 1];
        if (following !== undefined && /^( {2,}|\t)\S/.test(following)) {
          body.push('');
          i++;
          continue;
        }
        break;
      }
      if (/^( {2,}|\t)/.test(line)) {
        body.push(line.replace(/^( {1,4}|\t)/, ''));
        i++;
        continue;
      }
      if (startsBlock(line) && !(ordered ? ORDERED.test(line) : BULLET.test(line))) break;
      if (startsBlock(line)) break;
      body.push(line.trim());
      i++;
    }
    items.push(blocks(body));
  }
  return { block: { kind: 'list', ordered, start, items }, next: i };
}

function cells(line: string): string[] {
  let text = line.trim();
  if (text.startsWith('|')) text = text.slice(1);
  if (text.endsWith('|') && !text.endsWith('\\|')) text = text.slice(0, -1);
  const out: string[] = [];
  let current = '';
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '\\' && text[i + 1] === '|') {
      current += '|';
      i++;
    } else if (text[i] === '|') {
      out.push(current.trim());
      current = '';
    } else {
      current += text[i];
    }
  }
  out.push(current.trim());
  return out;
}

function readTable(lines: string[], from: number): { block: Block; next: number } | null {
  const head = cells(lines[from]);
  const marks = cells(lines[from + 1]);
  if (head.length !== marks.length) return null;
  const align = marks.map((m): Align => {
    const [left, right] = [m.startsWith(':'), m.endsWith(':')];
    return left && right ? 'center' : right ? 'right' : left ? 'left' : null;
  });
  const rows: Inline[][][] = [];
  let i = from + 2;
  while (i < lines.length && lines[i].trim() && lines[i].includes('|')) {
    const row = cells(lines[i++]);
    rows.push(head.map((_, c) => inline(row[c] ?? '')));
  }
  return { block: { kind: 'table', head: head.map(inline), align, rows }, next: i };
}

const ESCAPABLE = '\\`*_{}[]()#+-.!|~>';

export function inline(text: string): Inline[] {
  const out: Inline[] = [];
  let plain = '';
  const flush = () => {
    if (plain) out.push({ kind: 'text', text: plain });
    plain = '';
  };
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '\\' && i + 1 < text.length && ESCAPABLE.includes(text[i + 1])) {
      plain += text[i + 1];
      i += 2;
      continue;
    }
    if (c === '\n') {
      const hard = plain.endsWith('  ');
      plain = plain.replace(/ +$/, '');
      if (hard) {
        flush();
        out.push({ kind: 'break' });
      } else {
        plain += ' ';
      }
      i++;
      continue;
    }
    if (c === '`') {
      const run = /^`+/.exec(text.slice(i))![0];
      const end = text.indexOf(run, i + run.length);
      if (end > -1) {
        flush();
        out.push({ kind: 'code', text: text.slice(i + run.length, end).replace(/^ (.*\S.*) $/s, '$1') });
        i = end + run.length;
        continue;
      }
      plain += run;
      i += run.length;
      continue;
    }
    if (c === '[') {
      const link = readLink(text, i);
      if (link) {
        flush();
        out.push(link.node);
        i = link.next;
        continue;
      }
    }
    const marker = c === '*' || c === '_' || c === '~' ? emphasis(text, i) : null;
    if (marker) {
      flush();
      out.push(marker.node);
      i = marker.next;
      continue;
    }
    const auto = c === '<' ? /^<((?:https?:\/\/|mailto:)[^\s<>]+)>/i.exec(text.slice(i)) : null;
    if (auto) {
      flush();
      out.push({ kind: 'link', href: auto[1], children: [{ kind: 'text', text: auto[1] }] });
      i += auto[0].length;
      continue;
    }
    plain += c;
    i++;
  }
  flush();
  return out;
}

function readLink(text: string, from: number): { node: Inline; next: number } | null {
  let depth = 0;
  let close = -1;
  for (let i = from; i < text.length; i++) {
    if (text[i] === '\\') {
      i++;
    } else if (text[i] === '[') {
      depth++;
    } else if (text[i] === ']' && --depth === 0) {
      close = i;
      break;
    }
  }
  if (close < 0 || text[close + 1] !== '(') return null;
  const end = text.indexOf(')', close + 2);
  if (end < 0) return null;
  const target = text.slice(close + 2, end).trim().split(/\s+/)[0] ?? '';
  const label = text.slice(from + 1, close);
  return { node: { kind: 'link', href: target, children: inline(label) }, next: end + 1 };
}

function emphasis(text: string, from: number): { node: Inline; next: number } | null {
  const c = text[from];
  const run = /^(\*+|_+|~+)/.exec(text.slice(from))![0];
  if (c === '~' && run.length !== 2) return null;
  const size = c === '~' ? 2 : Math.min(run.length, 2);
  const mark = c.repeat(size);
  if (c === '_' && from > 0 && /\w/.test(text[from - 1])) return null;
  const inner = from + size;
  if (inner >= text.length || /\s/.test(text[inner])) return null;
  let at = inner;
  while (at < text.length) {
    const end = text.indexOf(mark, at);
    if (end < 0) return null;
    const closes = !/\s/.test(text[end - 1]) && (c !== '_' || !/\w/.test(text[end + size] ?? ''));
    if (closes && end > inner) {
      const children = inline(text.slice(inner, end));
      const kind = c === '~' ? 'del' : size === 2 ? 'strong' : 'em';
      return { node: { kind, children }, next: end + size };
    }
    at = end + 1;
  }
  return null;
}
