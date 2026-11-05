// The shortcut map: what a key does on the grid, away from the editor. Excel's keys where a browser
// lets a page have them. Browsers keep Ctrl+N, Ctrl+T and Ctrl+W for themselves.

export type GridAction =
  | 'undo'
  | 'redo'
  | 'copy'
  | 'cut'
  | 'paste'
  | 'pasteValues'
  | 'clear'
  | 'fillDown'
  | 'fillRight'
  | 'insertCells'
  | 'deleteCells'
  | 'insertSheet'
  | 'previousSheet'
  | 'nextSheet'
  | 'today'
  | 'now'
  | 'autoSum'
  | 'selectColumn'
  | 'selectRow'
  | 'selectAll'
  | 'calculate'
  | 'calculateSheet'
  | 'calculateAll'
  | 'edit'
  | 'cancel';

export interface ShortcutKey {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
}

/** The action a key stands for on the grid, if any. */
export function actionFor(event: ShortcutKey): GridAction | null {
  const ctrl = event.ctrlKey || event.metaKey;
  const shift = !!event.shiftKey;
  const alt = !!event.altKey;
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (key === 'F2' && !ctrl && !alt) return shift ? null : 'edit';
  if (key === 'F9') {
    if (ctrl && alt) return 'calculateAll';
    if (shift) return 'calculateSheet';
    return ctrl ? null : 'calculate';
  }
  if (key === 'F11' && shift) return 'insertSheet';
  if (key === 'F4' && !ctrl && !alt && !shift) return 'redo';
  if (key === 'Delete' && !ctrl && !alt) return 'clear';
  if (key === 'Backspace' && !ctrl && !alt) return null;
  if (key === 'Escape') return 'cancel';
  if (key === ' ' && ctrl && shift) return 'selectAll';
  if (key === ' ' && ctrl) return 'selectColumn';
  if (key === ' ' && shift && !alt) return 'selectRow';
  if (key === '=' && alt && !ctrl) return 'autoSum';
  if (!ctrl || alt) return null;
  switch (key) {
    case 'z':
      return shift ? 'redo' : 'undo';
    case 'y':
      return 'redo';
    case 'c':
      return shift ? null : 'copy';
    case 'x':
      return shift ? null : 'cut';
    case 'v':
      return shift ? 'pasteValues' : 'paste';
    case 'd':
      return 'fillDown';
    case 'r':
      return 'fillRight';
    case 'a':
      return 'selectAll';
    case '-':
      return 'deleteCells';
    case '+':
    case '=':
      return shift || key === '+' ? 'insertCells' : null;
    case 'PageUp':
      return 'previousSheet';
    case 'PageDown':
      return 'nextSheet';
    case ';':
    case ':':
      return shift || key === ':' ? 'now' : 'today';
    default:
      return null;
  }
}

/** Whether a key on the grid starts typing into the selected cell. */
export function startsTyping(event: ShortcutKey) {
  if (event.ctrlKey || event.metaKey) return false;
  // Alt alone is a menu key, and AltGr reports both.
  if (event.altKey) return false;
  return event.key.length === 1 || event.key === 'Process' || event.key === 'Dead';
}
