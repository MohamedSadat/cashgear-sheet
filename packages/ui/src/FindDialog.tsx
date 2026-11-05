// Find and Replace. A search reads through the engine, and a replace is a command like any other.
import { useRef, useState } from 'react';
import { CgButton, CgTabs, CgWindow } from '@cashgear/ui';
import type { Command, EngineClient, FindQuery, FoundCell } from '@cgs/engine-client';
import type { Applied } from '@cgs/engine-client';
import type { CellRange } from './grid/controller';
import { cellName, MAX_COLS, MAX_ROWS } from './grid/geometry';
import { toArea } from './sheet-editing';

export interface FindDialogProps {
  open: boolean;
  tab: 'find' | 'replace';
  engine: () => EngineClient | null;
  /** The selection as it stands: the sheet showing, the active cell and the area, 0-based. */
  selection: () => { sheet: number; row: number; col: number; range: CellRange } | null;
  sheetName: (index: number) => string;
  settled: () => Promise<unknown>;
  run: (command: Command) => Promise<Applied>;
  goTo: (found: FoundCell) => Promise<void>;
  onClose: () => void;
}

export function FindDialog(props: FindDialogProps) {
  const [tab, setTab] = useState<string>(props.tab);
  const [text, setText] = useState('');
  const [replacement, setReplacement] = useState('');
  const [workbook, setWorkbook] = useState(false);
  const [byColumns, setByColumns] = useState(false);
  const [lookIn, setLookIn] = useState<'formulas' | 'values'>('formulas');
  const [matchCase, setMatchCase] = useState(false);
  const [wholeCell, setWholeCell] = useState(false);
  const [message, setMessage] = useState('');
  const [list, setList] = useState<FoundCell[]>([]);
  const last = useRef<FoundCell | null>(null);
  const replacing = tab === 'replace';

  const query = (): FindQuery | null => {
    const selection = props.selection();
    if (!selection) return null;
    const { range } = selection;
    const one = range.top === range.bottom && range.left === range.right;
    const everything = range.bottom === MAX_ROWS - 1 && range.right === MAX_COLS - 1;
    return {
      text,
      sheet: selection.sheet,
      workbook,
      byColumns,
      // Replacing always works on what the formula bar shows, as in Excel.
      formulas: replacing || lookIn === 'formulas',
      matchCase,
      wholeCell,
      ...(!workbook && !one && !everything ? { within: toArea(range) } : {}),
    };
  };

  const findNext = async (back: boolean) => {
    const engine = props.engine();
    const selection = props.selection();
    const search = query();
    if (!engine || !selection || !search || !text) return null;
    await props.settled();
    const found = await engine.findNext(search, selection.row + 1, selection.col + 1, back);
    if (found) {
      last.current = found;
      setMessage('');
      await props.goTo(found);
    } else {
      last.current = null;
      setMessage('We couldn’t find what you were looking for.');
    }
    return found;
  };

  const findAll = async () => {
    const engine = props.engine();
    const search = query();
    if (!engine || !search || !text) return;
    await props.settled();
    const found = await engine.findAll(search);
    setList(found);
    setMessage(found.length === 1 ? '1 cell found' : `${found.length} cells found`);
  };

  const replaceOne = async () => {
    const search = query();
    const selection = props.selection();
    if (!search || !selection || !text) return;
    const here = last.current;
    if (
      here &&
      here.sheet === selection.sheet &&
      here.row === selection.row + 1 &&
      here.col === selection.col + 1
    ) {
      try {
        await props.run({
          kind: 'replaceOne',
          query: search,
          with: replacement,
          sheet: here.sheet,
          row: here.row,
          col: here.col,
        });
      } catch (error) {
        setMessage(error instanceof Error ? error.message : String(error));
        return;
      }
    }
    await findNext(false);
  };

  const replaceAll = async () => {
    const search = query();
    if (!search || !text) return;
    try {
      const applied = await props.run({ kind: 'replaceAll', query: search, with: replacement });
      const count = applied.replaced ?? 0;
      setMessage(
        count === 0
          ? 'We couldn’t find anything to replace.'
          : `All done. We made ${count} ${count === 1 ? 'replacement' : 'replacements'}.`,
      );
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  const form = (
    <div className="cgs-dialog-column">
      <label className="cgs-dialog-field">
        <span>Find what</span>
        <input
          data-testid="find-text"
          autoFocus
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              void findNext(event.shiftKey);
            }
          }}
        />
      </label>
      {replacing && (
        <label className="cgs-dialog-field">
          <span>Replace with</span>
          <input
            data-testid="replace-text"
            value={replacement}
            onChange={(event) => setReplacement(event.target.value)}
          />
        </label>
      )}
      <div className="cgs-dialog-row">
        <label className="cgs-dialog-field">
          <span>Within</span>
          <select
            data-testid="find-within"
            value={workbook ? 'workbook' : 'sheet'}
            onChange={(event) => setWorkbook(event.target.value === 'workbook')}
          >
            <option value="sheet">Sheet</option>
            <option value="workbook">Workbook</option>
          </select>
        </label>
        <label className="cgs-dialog-field">
          <span>Search</span>
          <select
            data-testid="find-order"
            value={byColumns ? 'columns' : 'rows'}
            onChange={(event) => setByColumns(event.target.value === 'columns')}
          >
            <option value="rows">By Rows</option>
            <option value="columns">By Columns</option>
          </select>
        </label>
        {!replacing && (
          <label className="cgs-dialog-field">
            <span>Look in</span>
            <select
              data-testid="find-look-in"
              value={lookIn}
              onChange={(event) => setLookIn(event.target.value as 'formulas' | 'values')}
            >
              <option value="formulas">Formulas</option>
              <option value="values">Values</option>
            </select>
          </label>
        )}
      </div>
      <div className="cgs-dialog-row">
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="find-case"
            checked={matchCase}
            onChange={(event) => setMatchCase(event.target.checked)}
          />
          Match case
        </label>
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="find-whole"
            checked={wholeCell}
            onChange={(event) => setWholeCell(event.target.checked)}
          />
          Match entire cell contents
        </label>
      </div>
      {message && (
        <p role="status" data-testid="find-message" className="cgs-dialog-message">
          {message}
        </p>
      )}
      {list.length > 0 && (
        <ul className="cgs-find-list" data-testid="find-list">
          {list.map((found) => (
            <li key={found.sheet + ':' + found.row + ':' + found.col}>
              <button type="button" onClick={() => void props.goTo(found)}>
                <span>{props.sheetName(found.sheet)}</span>
                <span>{cellName(found.row - 1, found.col - 1)}</span>
                <span>{found.text}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );

  return (
    <CgWindow
      open={props.open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Find and Replace"
      width={520}
      allowDrag
      data-testid="find-dialog"
      renderBody={() => (
        <CgTabs
          tabs={[
            { key: 'find', text: 'Find', content: form },
            { key: 'replace', text: 'Replace', content: form },
          ]}
          activeKey={tab}
          onActiveKeyChange={(key) => key && setTab(key)}
          size="small"
          contentMode="active-only"
          ariaLabel="Find and Replace"
        />
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          {replacing && (
            <>
              <CgButton data-testid="replace-all" onClick={() => void replaceAll()}>
                Replace All
              </CgButton>
              <CgButton data-testid="replace-one" onClick={() => void replaceOne()}>
                Replace
              </CgButton>
            </>
          )}
          <CgButton data-testid="find-all" onClick={() => void findAll()}>
            Find All
          </CgButton>
          <CgButton intent="primary" data-testid="find-next" onClick={() => void findNext(false)}>
            Find Next
          </CgButton>
          <CgButton data-testid="find-close" onClick={props.onClose}>
            Close
          </CgButton>
        </div>
      )}
    />
  );
}
