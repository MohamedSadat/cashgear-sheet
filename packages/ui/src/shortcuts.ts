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
  | 'cancel'
  | 'bold'
  | 'italic'
  | 'underline'
  | 'strike'
  | 'formatCells'
  | 'find'
  | 'replace'
  | 'goTo'
  | 'hideRows'
  | 'hideCols'
  | 'unhideRows'
  | 'unhideCols'
  | 'formatGeneral'
  | 'formatNumber'
  | 'formatCurrency'
  | 'formatPercent'
  | 'formatScientific'
  | 'formatDate'
  | 'formatTime'
  | 'borderOutline'
  | 'borderNone'
  | 'contextMenu'
  | 'hyperlink'
  | 'note'
  | 'filter';

export interface ShortcutKey {
  key: string;
  /** Where the key is, for a layout whose letters aren't Latin, such as Arabic. */
  code?: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
}

function latinKey(event: ShortcutKey, ctrl: boolean) {
  const { key, code } = event;
  if (key.length !== 1) return key;
  const lower = key.toLowerCase();
  if (!ctrl || !code || lower.charCodeAt(0) < 128) return lower;
  if (code.startsWith('Key')) return code.slice(3).toLowerCase();
  if (code.startsWith('Digit')) return code.slice(5);
  return lower;
}

/** The action a key stands for on the grid, if any. */
export function actionFor(event: ShortcutKey): GridAction | null {
  const ctrl = event.ctrlKey || event.metaKey;
  const shift = !!event.shiftKey;
  const alt = !!event.altKey;
  // With Ctrl, a key means what it means on a Latin layout, so that Ctrl+B works in Arabic too.
  const key = latinKey(event, Boolean(ctrl));
  if (key === 'ContextMenu' || (key === 'F10' && shift && !ctrl && !alt)) return 'contextMenu';
  if (key === 'F2' && !ctrl && !alt) return shift ? 'note' : 'edit';
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
  if (shift) {
    switch (key) {
      case '~':
      case '`':
        return 'formatGeneral';
      case '!':
      case '1':
        return 'formatNumber';
      case '@':
      case '2':
        return 'formatTime';
      case '#':
      case '3':
        return 'formatDate';
      case '$':
      case '4':
        return 'formatCurrency';
      case '%':
      case '5':
        return 'formatPercent';
      case '^':
      case '6':
        return 'formatScientific';
      case '&':
      case '7':
        return 'borderOutline';
      case '_':
      case '-':
        return 'borderNone';
      case 'l':
        return 'filter';
      case '(':
      case '9':
        return 'unhideRows';
      case ')':
      case '0':
        return 'unhideCols';
      default:
        break;
    }
  } else {
    switch (key) {
      case 'b':
      case '2':
        return 'bold';
      case 'i':
      case '3':
        return 'italic';
      case 'u':
      case '4':
        return 'underline';
      case '5':
        return 'strike';
      case '1':
        return 'formatCells';
      case 'f':
        return 'find';
      case 'h':
        return 'replace';
      case 'g':
        return 'goTo';
      case 'k':
        return 'hyperlink';
      case '9':
        return 'hideRows';
      case '0':
        return 'hideCols';
      default:
        break;
    }
  }
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
