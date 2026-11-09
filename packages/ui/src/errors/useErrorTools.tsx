// Excel 2016's error checking in the shell: the smart tag beside a cell with a green triangle,
// the Error Checking dialog that goes from one such cell to the next, and the rules' settings.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { CgButton, CgWindow, type CgRibbonGroup } from '@cashgear/ui';
import type { Applied, Command, EngineClient } from '@cgs/engine-client';
import type { GridController, GridSelection } from '../grid/controller';
import { errorFix, errorText, type CellError, type ErrorRule } from './error-text';

const storageKey = 'cgs-error-rules';

/** The rules' bits, as `crates/cgs-model/src/checks.rs` numbers them. */
export const RULE_BITS: Record<ErrorRule, number> = {
  evaluateToError: 1,
  textDate: 2,
  numberAsText: 4,
  inconsistentFormula: 8,
  omittedCells: 16,
  unlockedFormula: 32,
  emptyReference: 64,
};

/** Excel 2016's defaults: every rule but empty references, with background checking on. */
export const DEFAULT_RULES = 63;

export interface ErrorSettings {
  background: boolean;
  rules: number;
}

/** Error checking's settings, kept on the device as Excel keeps them in the application. */
export function readErrorSettings(storage: () => Storage = () => localStorage): ErrorSettings {
  try {
    const saved = JSON.parse(storage().getItem(storageKey) ?? 'null') as ErrorSettings | null;
    if (saved && typeof saved.rules === 'number') return saved;
  } catch {
    // Missing or unreadable: Excel's defaults.
  }
  return { background: true, rules: DEFAULT_RULES };
}

export function writeErrorSettings(
  settings: ErrorSettings,
  storage: () => Storage = () => localStorage,
) {
  try {
    storage().setItem(storageKey, JSON.stringify(settings));
  } catch {
    // Then it lasts until the page closes.
  }
}

/** The bits the engine checks for. */
export const ruleBits = (settings: ErrorSettings) => (settings.background ? settings.rules : 0);

export interface ErrorToolsHost {
  engine: () => EngineClient | null;
  grid: () => GridController | null;
  sheet: () => number;
  selection: () => GridSelection | null;
  run: (command: Command) => Promise<Applied>;
  notice: (message: string | null) => void;
  /** Starts editing the active cell in the formula bar. */
  editInBar: () => void;
  /** Selects a cell, 1-based. */
  goTo: (row: number, col: number) => void;
  openOptions: () => void;
}

interface Checking {
  row: number;
  col: number;
  errors: CellError[];
  formula: string;
  done: boolean;
}

const cellName = (row: number, col: number) => {
  let letters = '';
  for (let n = col; n > 0; n = Math.floor((n - 1) / 26)) {
    letters = String.fromCharCode(65 + ((n - 1) % 26)) + letters;
  }
  return '$' + letters + '$' + row;
};

export function useErrorTools(host: ErrorToolsHost) {
  const hostRef = useRef(host);
  hostRef.current = host;
  const [errors, setErrors] = useState<CellError[]>([]);
  const [tagMenu, setTagMenu] = useState(false);
  const [checking, setChecking] = useState<Checking | null>(null);
  const [place, setPlace] = useState<{ left: number; top: number; size: number } | null>(null);
  const token = useRef(0);

  /** What the active cell shows, after each command and move. */
  const refresh = useCallback(() => {
    const { engine: getEngine, selection: getSelection, sheet: getSheet, grid } = hostRef.current;
    const engine = getEngine();
    const selection = getSelection();
    if (!engine || !selection) return;
    const mine = ++token.current;
    const sheet = getSheet();
    void engine
      .query({ kind: 'cellErrors', sheet, row: selection.row + 1, col: selection.col + 1 })
      .then((found) => {
        if (mine !== token.current) return;
        const list = (found ?? []) as CellError[];
        setErrors((old) => (JSON.stringify(old) === JSON.stringify(list) ? old : list));
        const box = grid()?.cellBox(selection.row, selection.col);
        // Beside the cell, before it: to its right on a right-to-left sheet.
        const leftward = !(grid()?.rightToLeft ?? false);
        setPlace(
          list.length && box
            ? {
                left: leftward ? box.left - 20 : box.left + box.width + 2,
                top: box.top,
                size: Math.min(18, Math.max(14, box.height)),
              }
            : null,
        );
        if (!list.length) setTagMenu(false);
      })
      .catch(() => setErrors([]));
  }, []);

  const send = useCallback((command: Command) => {
    hostRef.current.notice(null);
    return hostRef.current.run(command).catch((error: unknown) => {
      hostRef.current.notice(error instanceof Error ? error.message : String(error));
    });
  }, []);

  /** The fix a rule offers, for the active cell or the cell Error Checking is on. */
  const fix = useCallback(
    async (error: CellError, row: number, col: number) => {
      const sheet = hostRef.current.sheet();
      const cell = { top: row, left: col, bottom: row, right: col };
      switch (error.rule) {
        case 'numberAsText':
          await send({ kind: 'convertToNumber', sheet, areas: [cell] });
          break;
        case 'omittedCells':
          await send({ kind: 'includeCells', sheet, row, col });
          break;
        case 'unlockedFormula':
          await send({ kind: 'format', sheet, areas: [cell], change: { locked: true } });
          break;
        case 'inconsistentFormula': {
          const from =
            error.from === 'left'
              ? { top: row, left: col - 1, bottom: row, right: col - 1 }
              : { top: row - 1, left: col, bottom: row - 1, right: col };
          const to =
            error.from === 'left'
              ? { top: row, left: col - 1, bottom: row, right: col }
              : { top: row - 1, left: col, bottom: row, right: col };
          await send({ kind: 'fill', sheet, from, to });
          break;
        }
        case 'textDate': {
          const engine = hostRef.current.engine();
          const text = engine ? (await engine.cellText(sheet, row, col)).edit : '';
          const written = text
            .replace(/^'/, '')
            .replace(/(\d{1,2}[/ -](?:\d{1,2}|[A-Za-z]{3,})[/ -])(\d{2})$/, '$120$2');
          if (written !== text) await send({ kind: 'input', sheet, row, col, text: written });
          break;
        }
        default:
          break;
      }
    },
    [send],
  );

  const ignore = useCallback(
    (row: number, col: number) =>
      send({ kind: 'ignoreError', sheet: hostRef.current.sheet(), row, col }),
    [send],
  );

  /** Error Checking: the next cell with an error from here, or the end of the check. */
  const step = useCallback(async (from: { row: number; col: number }, back = false) => {
    const engine = hostRef.current.engine();
    if (!engine) return;
    const sheet = hostRef.current.sheet();
    const next = await engine.query({
      kind: 'nextError',
      sheet,
      row: from.row,
      col: from.col,
      back,
    });
    if (!next) {
      setChecking({ row: from.row, col: from.col, errors: [], formula: '', done: true });
      return;
    }
    const [row, col] = next;
    hostRef.current.goTo(row, col);
    const [found, text] = await Promise.all([
      engine.query({ kind: 'cellErrors', sheet, row, col }),
      engine.cellText(sheet, row, col),
    ]);
    setChecking({
      row,
      col,
      errors: (found ?? []) as CellError[],
      formula: text.edit,
      done: false,
    });
  }, []);

  const startChecking = useCallback(() => {
    const selection = hostRef.current.selection();
    const row = selection ? selection.row + 1 : 1;
    const col = selection ? selection.col + 1 : 1;
    // A cell with an error is the first one shown, as in Excel.
    if (errors.length && selection) {
      const engine = hostRef.current.engine();
      void engine
        ?.cellText(hostRef.current.sheet(), row, col)
        .then((text) => setChecking({ row, col, errors, formula: text.edit, done: false }));
      return;
    }
    void step({ row, col });
  }, [errors, step]);

  // Moves and edits close the smart tag's menu.
  useEffect(() => setTagMenu(false), [errors]);

  /** The Formulas tab's Formula Auditing group. */
  const group = useMemo<CgRibbonGroup>(
    () => ({
      key: 'auditing',
      text: 'Formula Auditing',
      icon: 'error-check',
      items: [
        {
          name: 'error-checking',
          text: 'Error Checking',
          icon: 'error-check',
          size: 'large',
          onClick: startChecking,
        },
      ],
    }),
    [startChecking],
  );

  const first = errors[0];
  const tagTitle = first ? errorText(first).title : '';
  const overlays: ReactNode = (
    <>
      {place && first && (
        <button
          type="button"
          data-testid="error-tag"
          aria-label={tagTitle}
          title={tagTitle}
          onClick={() => setTagMenu((open) => !open)}
          style={{
            position: 'fixed',
            left: place.left,
            top: place.top,
            inlineSize: place.size,
            blockSize: place.size,
            padding: 0,
            border: '1px solid #a6a6a6',
            background: '#fff',
            color: '#c47800',
            font: 'bold 12px "Segoe UI", sans-serif',
            lineHeight: 1,
            zIndex: 36,
            cursor: 'default',
          }}
        >
          !
        </button>
      )}
      {tagMenu && place && first && (
        <div
          role="menu"
          data-testid="error-tag-menu"
          style={{
            position: 'fixed',
            left: place.left,
            top: place.top + place.size + 2,
            minInlineSize: 220,
            padding: '3px 0',
            background: 'var(--cg-surface, #fff)',
            color: 'inherit',
            border: '1px solid #a6a6a6',
            boxShadow: '1px 2px 6px rgba(0,0,0,0.25)',
            zIndex: 1000,
          }}
        >
          <div style={{ padding: '4px 10px', fontWeight: 600 }}>{tagTitle}</div>
          {[
            ...(errorFix(first)
              ? [[errorFix(first)!, () => fix(first, rowOf(), colOf())] as const]
              : []),
            ['Ignore Error', () => ignore(rowOf(), colOf())] as const,
            ['Edit in Formula Bar', () => hostRef.current.editInBar()] as const,
            ['Error Checking Options…', () => hostRef.current.openOptions()] as const,
          ].map(([label, act]) => (
            <button
              key={label}
              type="button"
              role="menuitem"
              data-testid={'error-tag-' + label.replace(/\W+/g, '-').toLowerCase()}
              onClick={() => {
                setTagMenu(false);
                void act();
              }}
              style={{
                display: 'block',
                inlineSize: '100%',
                padding: '4px 10px',
                textAlign: 'start',
                font: 'inherit',
                color: 'inherit',
                background: 'none',
                border: 0,
              }}
            >
              {label}
            </button>
          ))}
        </div>
      )}
      {checking && (
        <CgWindow
          open
          onOpenChange={(next) => {
            if (!next) setChecking(null);
          }}
          headerText="Error Checking"
          width={460}
          allowDrag
          data-testid="error-checking-dialog"
          renderBody={() =>
            checking.done ? (
              <p data-testid="error-checking-done">
                The error check is complete for the entire sheet.
              </p>
            ) : (
              <div className="cgs-dialog-column">
                <strong data-testid="error-checking-cell">
                  Error in cell {cellName(checking.row, checking.col).replace(/\$/g, '')}
                </strong>
                <code data-testid="error-checking-formula">{checking.formula}</code>
                {checking.errors[0] && (
                  <>
                    <strong data-testid="error-checking-title">
                      {errorText(checking.errors[0]).title}
                    </strong>
                    <span>{errorText(checking.errors[0]).text}</span>
                  </>
                )}
              </div>
            )
          }
          renderFooter={() => (
            <div className="cgs-dialog-actions" style={{ flexWrap: 'wrap' }}>
              {!checking.done && checking.errors[0] && errorFix(checking.errors[0]) && (
                <CgButton
                  data-testid="error-checking-fix"
                  onClick={() =>
                    void fix(checking.errors[0]!, checking.row, checking.col).then(() =>
                      step(checking),
                    )
                  }
                >
                  {errorFix(checking.errors[0])}
                </CgButton>
              )}
              {!checking.done && (
                <CgButton
                  data-testid="error-checking-ignore"
                  onClick={() => void ignore(checking.row, checking.col).then(() => step(checking))}
                >
                  Ignore Error
                </CgButton>
              )}
              {!checking.done && (
                <CgButton
                  data-testid="error-checking-edit"
                  onClick={() => {
                    setChecking(null);
                    hostRef.current.editInBar();
                  }}
                >
                  Edit in Formula Bar
                </CgButton>
              )}
              <CgButton data-testid="error-checking-options" onClick={hostRef.current.openOptions}>
                Options…
              </CgButton>
              {!checking.done && (
                <CgButton
                  data-testid="error-checking-previous"
                  onClick={() => void step(checking, true)}
                >
                  Previous
                </CgButton>
              )}
              <CgButton
                intent={checking.done ? 'primary' : undefined}
                data-testid={checking.done ? 'error-checking-ok' : 'error-checking-next'}
                onClick={() => (checking.done ? setChecking(null) : void step(checking))}
              >
                {checking.done ? 'OK' : 'Next'}
              </CgButton>
            </div>
          )}
        />
      )}
    </>
  );

  function rowOf() {
    return (hostRef.current.selection()?.row ?? 0) + 1;
  }
  function colOf() {
    return (hostRef.current.selection()?.col ?? 0) + 1;
  }

  return { errors, refresh, group, overlays, startChecking };
}
