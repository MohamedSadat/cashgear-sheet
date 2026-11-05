// What the cell editor needs to know about the text being typed: which references it holds, whether
// the caret is somewhere a clicked cell would make sense, what function name is being typed. It only
// reads the text in the editor, and has nothing to do with what any cell holds.

export const MAX_ROW = 1_048_576;
export const MAX_COL = 16_384;

export interface FoundReference {
  /** Where it stands in the text, sheet name included. */
  start: number;
  end: number;
  sheet: string | null;
  /** 0-based and inclusive. A whole column runs to the last row, and a whole row to the last column. */
  top: number;
  left: number;
  bottom: number;
  right: number;
  /** Which colour it gets: the same text always gets the same one. */
  color: number;
}

const WORD = /[\p{L}\p{N}_.$\\]/u;

function colNumber(letters: string) {
  let col = 0;
  for (const ch of letters.toUpperCase()) col = col * 26 + ch.charCodeAt(0) - 64;
  return col;
}

function readCell(word: string) {
  const match = /^\$?([A-Za-z]{1,3})\$?(\d{1,7})$/.exec(word);
  if (!match) return null;
  const col = colNumber(match[1]!);
  const row = Number(match[2]);
  if (col < 1 || col > MAX_COL || row < 1 || row > MAX_ROW) return null;
  return { row: row - 1, col: col - 1 };
}

function readColumn(word: string) {
  const match = /^\$?([A-Za-z]{1,3})$/.exec(word);
  if (!match) return null;
  const col = colNumber(match[1]!);
  return col >= 1 && col <= MAX_COL ? col - 1 : null;
}

function readRow(word: string) {
  const match = /^\$?(\d{1,7})$/.exec(word);
  if (!match) return null;
  const row = Number(match[1]);
  return row >= 1 && row <= MAX_ROW ? row - 1 : null;
}

/** Moves past text in quotes, whose quote doubles to stand for itself. */
function skipQuoted(text: string, at: number, quote: string) {
  let index = at + 1;
  while (index < text.length) {
    if (text[index] === quote) {
      if (text[index + 1] !== quote) return index + 1;
      index += 2;
    } else {
      index++;
    }
  }
  return index;
}

function wordEnd(text: string, at: number) {
  let index = at;
  while (index < text.length && WORD.test(text[index]!)) index++;
  return index;
}

/** Whether what's in the editor is a formula rather than text. */
export function isFormula(text: string) {
  return text.startsWith('=') || text.startsWith('+') || text.startsWith('-');
}

/** Every plain reference in a formula, in order. */
export function findReferences(text: string): FoundReference[] {
  const found: FoundReference[] = [];
  if (!isFormula(text)) return found;
  const colors = new Map<string, number>();
  let at = 1;
  while (at < text.length) {
    const ch = text[at]!;
    if (ch === '"') {
      at = skipQuoted(text, at, '"');
      continue;
    }
    if (ch === '[') {
      // A table's own syntax.
      let depth = 0;
      while (at < text.length) {
        if (text[at] === '[') depth++;
        if (text[at] === ']') {
          depth--;
          if (depth === 0) break;
        }
        at++;
      }
      at++;
      continue;
    }
    const start = at;
    let sheet: string | null = null;
    let wordStart = at;
    if (ch === "'") {
      const end = skipQuoted(text, at, "'");
      if (text[end] !== '!') {
        at = end;
        continue;
      }
      sheet = text.slice(at + 1, end - 1).replace(/''/g, "'");
      wordStart = end + 1;
    } else if (!WORD.test(ch)) {
      at++;
      continue;
    } else {
      const end = wordEnd(text, at);
      if (text[end] === '!') {
        sheet = text.slice(at, end);
        wordStart = end + 1;
      }
    }
    const firstEnd = wordEnd(text, wordStart);
    const first = text.slice(wordStart, firstEnd);
    if (!first || text[firstEnd] === '(' || text[firstEnd] === '[') {
      at = Math.max(firstEnd, at + 1);
      continue;
    }
    let area: { top: number; left: number; bottom: number; right: number } | null = null;
    let end = firstEnd;
    if (text[firstEnd] === ':') {
      const secondEnd = wordEnd(text, firstEnd + 1);
      const second = text.slice(firstEnd + 1, secondEnd);
      const a = readCell(first);
      const b = readCell(second);
      if (a && b) {
        area = {
          top: Math.min(a.row, b.row),
          left: Math.min(a.col, b.col),
          bottom: Math.max(a.row, b.row),
          right: Math.max(a.col, b.col),
        };
        end = secondEnd;
      } else {
        const colA = readColumn(first);
        const colB = readColumn(second);
        const rowA = readRow(first);
        const rowB = readRow(second);
        if (colA !== null && colB !== null) {
          area = {
            top: 0,
            bottom: MAX_ROW - 1,
            left: Math.min(colA, colB),
            right: Math.max(colA, colB),
          };
          end = secondEnd;
        } else if (rowA !== null && rowB !== null) {
          area = {
            left: 0,
            right: MAX_COL - 1,
            top: Math.min(rowA, rowB),
            bottom: Math.max(rowA, rowB),
          };
          end = secondEnd;
        }
      }
    }
    if (!area) {
      const cell = readCell(first);
      if (cell) area = { top: cell.row, left: cell.col, bottom: cell.row, right: cell.col };
    }
    if (!area) {
      at = Math.max(firstEnd, at + 1);
      continue;
    }
    const key = text.slice(start, end).replace(/\$/g, '').toUpperCase();
    let color = colors.get(key);
    if (color === undefined) {
      color = colors.size;
      colors.set(key, color);
    }
    found.push({ start, end, sheet, color, ...area });
    at = end;
  }
  return found;
}

/** Whether a clicked or arrowed-to cell would go in at the caret, as a reference. */
export function canPointAt(text: string, caret: number) {
  if (!isFormula(text)) return false;
  let at = caret - 1;
  while (at >= 0 && text[at] === ' ') at--;
  if (at < 0) return false;
  // In a string, what comes before isn't an operator.
  let quote = false;
  for (let index = 0; index <= at; index++) {
    if (text[index] === '"') quote = !quote;
  }
  if (quote) return false;
  const next = text[caret];
  if (next !== undefined && WORD.test(next)) return false;
  return '=+-*/^&<>(,:%'.includes(text[at]!);
}

/** The function name being typed at the caret, if there is one. */
export function nameAtCaret(text: string, caret: number): { start: number; prefix: string } | null {
  if (!isFormula(text)) return null;
  let quote = false;
  for (let index = 0; index < caret; index++) {
    if (text[index] === '"') quote = !quote;
  }
  if (quote) return null;
  let start = caret;
  while (start > 1 && /[\p{L}\p{N}_.]/u.test(text[start - 1]!)) start--;
  const prefix = text.slice(start, caret);
  if (!prefix || /^\d/.test(prefix)) return null;
  // Part of a sheet name or a table isn't a function.
  if (text[start - 1] === "'" || text[start - 1] === '!' || text[start - 1] === '[') return null;
  return { start, prefix };
}

/** The next of `A1`, `$A$1`, `A$1` and `$A1`, for F4. */
function nextAbsolute(cell: string) {
  const match = /^(\$?)([A-Za-z]{1,3})(\$?)(\d{1,7})$/.exec(cell);
  if (!match) return cell;
  const [, a, col, b, row] = match;
  if (!a && !b) return `$${col}$${row}`;
  if (a && b) return `${col}$${row}`;
  if (!a && b) return `$${col}${row}`;
  return `${col}${row}`;
}

/** F4: cycles the reference at the caret through its absolute forms. Returns the new text and caret. */
export function cycleAbsolute(text: string, caret: number): { text: string; caret: number } {
  const found = findReferences(text).find((ref) => caret >= ref.start && caret <= ref.end);
  if (!found) return { text, caret };
  const original = text.slice(found.start, found.end);
  const bang = original.lastIndexOf('!');
  const sheet = bang >= 0 ? original.slice(0, bang + 1) : '';
  const body = original.slice(bang + 1);
  const next = body.split(':').map(nextAbsolute).join(':');
  const replaced = sheet + next;
  return {
    text: text.slice(0, found.start) + replaced + text.slice(found.end),
    caret: found.start + replaced.length,
  };
}

/** A cell's or an area's address for putting in a formula, 0-based going in. */
export function referenceText(
  top: number,
  left: number,
  bottom: number,
  right: number,
  sheet: string | null,
): string {
  const name = (col: number) => {
    let out = '';
    for (let at = col + 1; at > 0; at = Math.floor((at - 1) / 26)) {
      out = String.fromCharCode(65 + ((at - 1) % 26)) + out;
    }
    return out;
  };
  const prefix = sheet === null ? '' : quoteSheet(sheet) + '!';
  if (left === 0 && right === MAX_COL - 1) return `${prefix}${top + 1}:${bottom + 1}`;
  if (top === 0 && bottom === MAX_ROW - 1) return `${prefix}${name(left)}:${name(right)}`;
  const first = name(left) + (top + 1);
  if (top === bottom && left === right) return prefix + first;
  return prefix + first + ':' + name(right) + (bottom + 1);
}

export function quoteSheet(name: string) {
  const plain =
    /^[\p{L}_][\p{L}\p{N}_.]*$/u.test(name) && !readCell(name) && !/^[rc]\d*$/i.test(name);
  return plain ? name : "'" + name.replace(/'/g, "''") + "'";
}
