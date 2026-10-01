/** Correspondance floue : score (plus haut = meilleur) et positions des caractères trouvés. */
export interface Match {
  score: number;
  marks: number[];
}

/** Élément classé, avec l'indice du champ qui a donné la meilleure correspondance. */
export interface Ranked<T> extends Match {
  item: T;
  field: number;
}

/** Morceau de texte, surligné ou non. */
export interface Segment {
  text: string;
  hit: boolean;
}

const WORD = 3;
const PREFIX = 4;
const CONTIGUOUS = 4;
const GAP = 1;
const GAP_CHAR = 0.05;
const LENGTH = 0.01;
const FIELD = 5;
const SEPARATOR = /[\s/\\_\-.:?&=#@{}()[\],'"]/;

const fold = (ch: string) => ch.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().charAt(0) || ch;

function boundary(chars: string[], i: number): number {
  if (i === 0) return PREFIX + WORD;
  const prev = chars[i - 1];
  const cur = chars[i];
  if (SEPARATOR.test(prev)) return WORD;
  return prev === prev.toLowerCase() && cur !== cur.toLowerCase() ? WORD : 0;
}

/**
 * Cherche `query` comme sous-séquence de `text`, sans tenir compte de la casse, des accents ni des
 * espaces de la requête, en favorisant le préfixe, les débuts de mot et les caractères contigus.
 */
export function fuzzy(text: string, query: string): Match | null {
  const q = Array.from(query.replace(/\s+/g, ''), fold);
  if (!q.length) return { score: 0, marks: [] };
  const raw = Array.from(text);
  const t = raw.map(fold);
  const n = t.length;
  const m = q.length;
  let found = 0;
  for (let j = 0; j < n && found < m; j++) if (t[j] === q[found]) found++;
  if (found < m) return null;

  const score = Array.from({ length: m }, () => new Float64Array(n).fill(-Infinity));
  const from = Array.from({ length: m }, () => new Int32Array(n).fill(-1));
  for (let i = 0; i < m; i++) {
    let best = -Infinity;
    let bestAt = -1;
    for (let j = i; j < n; j++) {
      if (i > 0 && j >= 2 && score[i - 1][j - 2] + GAP_CHAR * (j - 2) > best) {
        best = score[i - 1][j - 2] + GAP_CHAR * (j - 2);
        bestAt = j - 2;
      }
      if (t[j] !== q[i]) continue;
      const gain = 1 + boundary(raw, j);
      if (i === 0) {
        score[0][j] = gain - GAP_CHAR * j;
        continue;
      }
      const near = score[i - 1][j - 1] + CONTIGUOUS;
      const far = best - GAP - GAP_CHAR * (j - 1);
      if (near >= far) {
        score[i][j] = near + gain;
        from[i][j] = j - 1;
      } else {
        score[i][j] = far + gain;
        from[i][j] = bestAt;
      }
    }
  }

  let end = -1;
  for (let j = m - 1; j < n; j++) if (end < 0 || score[m - 1][j] > score[m - 1][end]) end = j;
  if (score[m - 1][end] === -Infinity) return null;
  const marks = new Array<number>(m);
  for (let i = m - 1, j = end; i >= 0; j = from[i][j], i--) marks[i] = j;
  return { score: score[m - 1][end] - LENGTH * n, marks };
}

/** Classe les éléments selon leur meilleur champ ; chaque champ pèse un peu moins que le précédent. */
export function rank<T>(items: readonly T[], query: string, fields: (item: T) => readonly string[]): Ranked<T>[] {
  const ranked: Ranked<T>[] = [];
  for (const item of items) {
    const texts = fields(item);
    let best: Ranked<T> | null = null;
    for (let field = 0; field < texts.length; field++) {
      const match = fuzzy(texts[field], query);
      if (match && (!best || match.score - field * FIELD > best.score)) {
        best = { item, field, score: match.score - field * FIELD, marks: match.marks };
      }
    }
    if (best) ranked.push(best);
  }
  return ranked.sort((a, b) => b.score - a.score);
}

/** Découpe `text` en segments, surlignés aux positions `marks`. */
export function segments(text: string, marks: readonly number[] = []): Segment[] {
  const hits = new Set(marks);
  const out: Segment[] = [];
  Array.from(text).forEach((ch, i) => {
    const last = out.at(-1);
    if (last && last.hit === hits.has(i)) last.text += ch;
    else out.push({ text: ch, hit: hits.has(i) });
  });
  return out;
}
