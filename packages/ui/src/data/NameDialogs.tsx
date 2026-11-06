// The Formulas tab's Defined Names dialogs: Name Manager, New Name and Edit Name, and Create Names
// from Selection. The engine holds the names; these only list them and send commands.
import { useCallback, useEffect, useState, type KeyboardEvent } from 'react';
import { CgButton, CgWindow } from '@cashgear/ui';
import type { Applied, CellArea, Command, DefinedNameInfo, EngineClient } from '@cgs/engine-client';

type Run = (command: Command) => Promise<Applied>;

/** Names in Excel's order: by name, ignoring case, then the workbook's before a sheet's. */
export function sortNames(names: DefinedNameInfo[]): DefinedNameInfo[] {
  return [...names].sort(
    (a, b) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }) ||
      (a.sheet ?? -1) - (b.sheet ?? -1),
  );
}

/** The names whose name or Refers to holds the filter's text, ignoring case. */
export function filterNames(names: DefinedNameInfo[], text: string): DefinedNameInfo[] {
  const wanted = text.trim().toLowerCase();
  if (!wanted) return names;
  return names.filter(
    (name) =>
      name.name.toLowerCase().includes(wanted) || name.formula.toLowerCase().includes(wanted),
  );
}

/** One name's key in the list: a name is only unique within its scope. */
export function nameKey(name: { name: string; sheet: number | null }): string {
  return (name.sheet ?? 'workbook') + ':' + name.name.toLowerCase();
}

/** Refers to as the engine takes it, with its `=`, or null when it's empty. */
export function refersTo(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed || trimmed === '=') return null;
  return trimmed.startsWith('=') ? trimmed : '=' + trimmed;
}

/** The command OK sends from New Name or Edit Name. */
export function setNameCommand(
  existing: DefinedNameInfo | undefined,
  name: string,
  formula: string,
  sheet: number | null,
): Command {
  return {
    kind: 'setName',
    ...(existing ? { old: { name: existing.name, sheet: existing.sheet } } : {}),
    name: name.trim(),
    formula,
    // A name keeps its scope when it's edited, as in Excel.
    sheet: existing ? existing.sheet : sheet,
  };
}

export interface Edges {
  top: boolean;
  left: boolean;
  bottom: boolean;
  right: boolean;
}

/** What Create Names from Selection ticks first: a row of labels on the left, a column of labels
 *  on top, or both for a block. */
export function defaultEdges(area: CellArea): Edges {
  const oneRow = area.top === area.bottom;
  const oneCol = area.left === area.right;
  return {
    top: !oneRow || oneCol,
    left: !oneCol || oneRow,
    bottom: false,
    right: false,
  };
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export interface NameManagerProps {
  open: boolean;
  engine: () => EngineClient | null;
  /** By sheet index. */
  sheetNames: string[];
  activeSheet: number;
  /** For a new name's Refers to, such as `=Sheet1!$A$1:$B$4`. */
  selectionText: string;
  run: Run;
  onClose: () => void;
}

export function NameManager(props: NameManagerProps) {
  if (!props.open) return null;
  return <NameManagerWindow {...props} />;
}

function NameManagerWindow(props: NameManagerProps) {
  const [names, setNames] = useState<DefinedNameInfo[]>([]);
  const [filter, setFilter] = useState('');
  const [selected, setSelected] = useState<string | null>(null);
  const [formula, setFormula] = useState('');
  const [editing, setEditing] = useState<'new' | 'edit' | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [message, setMessage] = useState('');
  const { engine } = props;

  const refresh = useCallback(async () => {
    const client = engine();
    if (!client) return [];
    const list = sortNames(await client.query({ kind: 'names' }));
    setNames(list);
    return list;
  }, [engine]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const shown = filterNames(names, filter);
  const current = names.find((name) => nameKey(name) === selected);

  const pick = (name: DefinedNameInfo | undefined) => {
    setSelected(name ? nameKey(name) : null);
    setFormula(name?.formula ?? '');
    setMessage('');
  };

  // A change from the dialogs on top lists the names again and keeps the one it made selected.
  const runAndRefresh: Run = async (command) => {
    const applied = await props.run(command);
    const list = await refresh();
    if (command.kind === 'setName') {
      const scope = command.sheet ?? null;
      pick(list.find((name) => nameKey(name) === nameKey({ name: command.name, sheet: scope })));
    }
    return applied;
  };

  const commitFormula = async () => {
    if (!current) return;
    const text = refersTo(formula);
    if (!text) {
      setMessage('The formula contains an error.');
      return;
    }
    try {
      await runAndRefresh(setNameCommand(current, current.name, text, current.sheet));
    } catch (error) {
      setMessage(errorText(error));
    }
  };

  const remove = async () => {
    setConfirming(false);
    if (!current) return;
    try {
      await props.run({ kind: 'deleteName', name: current.name, sheet: current.sheet });
      await refresh();
      pick(undefined);
    } catch (error) {
      setMessage(errorText(error));
    }
  };

  const changed = current !== undefined && formula !== current.formula;

  return (
    <>
      <CgWindow
        open
        onOpenChange={(next) => {
          if (!next) props.onClose();
        }}
        headerText="Name Manager"
        width={640}
        allowDrag
        data-testid="name-manager"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <div className="cgs-dialog-row">
              <CgButton data-testid="name-new" onClick={() => setEditing('new')}>
                New…
              </CgButton>
              <CgButton
                data-testid="name-edit"
                disabled={!current}
                onClick={() => setEditing('edit')}
              >
                Edit…
              </CgButton>
              <CgButton
                data-testid="name-delete"
                disabled={!current}
                onClick={() => setConfirming(true)}
              >
                Delete
              </CgButton>
              <label className="cgs-dialog-field cgs-dialog-grow">
                <span>Filter</span>
                <input
                  data-testid="name-filter"
                  value={filter}
                  onChange={(event) => setFilter(event.target.value)}
                />
              </label>
            </div>
            <div
              role="listbox"
              aria-label="Names"
              data-testid="name-list"
              tabIndex={0}
              style={{
                blockSize: '14rem',
                overflow: 'auto',
                border: 'var(--cg-border-style)',
              }}
              onKeyDown={(event) => {
                if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
                event.preventDefault();
                const at = shown.findIndex((name) => nameKey(name) === selected);
                const next = event.key === 'ArrowDown' ? at + 1 : Math.max(at - 1, 0);
                if (next < shown.length) pick(shown[next]);
              }}
            >
              <div style={{ ...rowStyle, fontWeight: 600, position: 'sticky', top: 0 }}>
                <span>Name</span>
                <span>Refers To</span>
                <span>Scope</span>
              </div>
              {shown.map((name) => {
                const key = nameKey(name);
                const on = key === selected;
                return (
                  <div
                    key={key}
                    role="option"
                    aria-selected={on}
                    data-testid="name-row"
                    style={{
                      ...rowStyle,
                      cursor: 'default',
                      background: on ? 'var(--cg-hover)' : undefined,
                    }}
                    onClick={() => pick(name)}
                    onDoubleClick={() => {
                      pick(name);
                      setEditing('edit');
                    }}
                  >
                    <span style={cellStyle}>{name.name}</span>
                    <span style={cellStyle}>{name.formula}</span>
                    <span style={cellStyle}>{name.scope}</span>
                  </div>
                );
              })}
            </div>
            <div className="cgs-dialog-field">
              <span id="cgs-name-refers">Refers to</span>
              <div className="cgs-dialog-row" style={{ alignItems: 'center', flexWrap: 'nowrap' }}>
                <input
                  data-testid="name-refers"
                  aria-labelledby="cgs-name-refers"
                  disabled={!current}
                  value={formula}
                  onChange={(event) => setFormula(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') {
                      event.preventDefault();
                      void commitFormula();
                    } else if (event.key === 'Escape' && changed) {
                      event.preventDefault();
                      event.stopPropagation();
                      setFormula(current.formula);
                    }
                  }}
                />
                <CgButton
                  data-testid="name-refers-cancel"
                  aria-label="Cancel"
                  disabled={!changed}
                  onClick={() => current && setFormula(current.formula)}
                >
                  ✕
                </CgButton>
                <CgButton
                  data-testid="name-refers-commit"
                  aria-label="Commit"
                  disabled={!changed}
                  onClick={() => void commitFormula()}
                >
                  ✓
                </CgButton>
              </div>
            </div>
            {message && (
              <p role="alert" data-testid="name-message" className="cgs-dialog-message">
                {message}
              </p>
            )}
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton data-testid="name-close" onClick={props.onClose}>
              Close
            </CgButton>
          </div>
        )}
      />
      <DefineNameDialog
        open={editing !== null}
        existing={editing === 'edit' ? current : undefined}
        sheetNames={props.sheetNames}
        activeSheet={props.activeSheet}
        selectionText={props.selectionText}
        run={runAndRefresh}
        onClose={() => setEditing(null)}
      />
      {confirming && current && (
        <CgWindow
          open
          onOpenChange={(next) => {
            if (!next) setConfirming(false);
          }}
          headerText="Microsoft Excel"
          width={420}
          data-testid="name-delete-confirm"
          renderBody={() => <p>Are you sure you want to delete the name {current.name}?</p>}
          renderFooter={() => (
            <div className="cgs-dialog-actions">
              <CgButton intent="primary" data-testid="name-delete-ok" onClick={() => void remove()}>
                OK
              </CgButton>
              <CgButton data-testid="name-delete-cancel" onClick={() => setConfirming(false)}>
                Cancel
              </CgButton>
            </div>
          )}
        />
      )}
    </>
  );
}

const rowStyle = {
  display: 'grid',
  gridTemplateColumns: '9rem minmax(0, 1fr) 7rem',
  gap: 'var(--cg-space-2)',
  padding: '2px var(--cg-space-1)',
  background: 'var(--cg-surface)',
} as const;

const cellStyle = { overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } as const;

export interface DefineNameDialogProps {
  open: boolean;
  /** The name Edit changes; none for a new one. */
  existing?: DefinedNameInfo;
  sheetNames: string[];
  activeSheet: number;
  selectionText: string;
  run: Run;
  onClose: () => void;
}

/** New Name, or Edit Name with `existing`. */
export function DefineNameDialog(props: DefineNameDialogProps) {
  if (!props.open) return null;
  return <DefineNameWindow {...props} />;
}

function DefineNameWindow(props: DefineNameDialogProps) {
  const { existing } = props;
  const [name, setName] = useState(existing?.name ?? '');
  const [scope, setScope] = useState<number | null>(existing ? existing.sheet : null);
  const [formula, setFormula] = useState(existing?.formula ?? props.selectionText);
  const [message, setMessage] = useState('');

  const ok = async () => {
    const text = refersTo(formula);
    if (!name.trim()) {
      setMessage('The name that you entered is not valid.');
      return;
    }
    if (!text) {
      setMessage('The formula contains an error.');
      return;
    }
    try {
      await props.run(setNameCommand(existing, name, text, scope));
      props.onClose();
    } catch (error) {
      setMessage(errorText(error));
    }
  };

  const enter = (event: KeyboardEvent) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      void ok();
    }
  };

  return (
    <CgWindow
      open
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText={existing ? 'Edit Name' : 'New Name'}
      width={420}
      allowDrag
      data-testid="define-name"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <label className="cgs-dialog-field">
            <span>Name</span>
            <input
              data-testid="define-name-name"
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={enter}
            />
          </label>
          <label className="cgs-dialog-field">
            <span>Scope</span>
            <select
              data-testid="define-name-scope"
              disabled={existing !== undefined}
              value={scope === null ? 'workbook' : String(scope)}
              onChange={(event) =>
                setScope(event.target.value === 'workbook' ? null : Number(event.target.value))
              }
            >
              <option value="workbook">Workbook</option>
              {props.sheetNames.map((sheet, index) => (
                <option key={index} value={String(index)}>
                  {sheet}
                </option>
              ))}
            </select>
          </label>
          <label className="cgs-dialog-field">
            <span>Refers to</span>
            <input
              data-testid="define-name-refers"
              value={formula}
              onChange={(event) => setFormula(event.target.value)}
              onKeyDown={enter}
            />
          </label>
          {message && (
            <p role="alert" data-testid="define-name-message" className="cgs-dialog-message">
              {message}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="define-name-ok" onClick={() => void ok()}>
            OK
          </CgButton>
          <CgButton data-testid="define-name-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

export interface CreateNamesDialogProps {
  open: boolean;
  sheet: number;
  /** 1-based. */
  area: CellArea;
  run: Run;
  onClose: () => void;
}

/** Create Names from Selection. */
export function CreateNamesDialog(props: CreateNamesDialogProps) {
  if (!props.open) return null;
  return <CreateNamesWindow {...props} />;
}

function CreateNamesWindow(props: CreateNamesDialogProps) {
  const [edges, setEdges] = useState<Edges>(() => defaultEdges(props.area));
  const [message, setMessage] = useState('');
  const any = edges.top || edges.left || edges.bottom || edges.right;

  const ok = async () => {
    if (!any) return;
    try {
      await props.run({
        kind: 'namesFromSelection',
        sheet: props.sheet,
        area: props.area,
        ...edges,
      });
      props.onClose();
    } catch (error) {
      setMessage(errorText(error));
    }
  };

  const boxes: [keyof Edges, string][] = [
    ['top', 'Top row'],
    ['left', 'Left column'],
    ['bottom', 'Bottom row'],
    ['right', 'Right column'],
  ];

  return (
    <CgWindow
      open
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Create Names from Selection"
      width={340}
      allowDrag
      data-testid="create-names"
      renderBody={() => (
        <div
          className="cgs-dialog-column"
          role="group"
          aria-label="Create names from values in the"
        >
          <span>Create names from values in the:</span>
          {boxes.map(([key, text]) => (
            <label key={key} className="cgs-dialog-check">
              <input
                type="checkbox"
                data-testid={'create-names-' + key}
                checked={edges[key]}
                onChange={(event) => setEdges({ ...edges, [key]: event.target.checked })}
              />
              {text}
            </label>
          ))}
          {message && (
            <p role="alert" data-testid="create-names-message" className="cgs-dialog-message">
              {message}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            intent="primary"
            data-testid="create-names-ok"
            disabled={!any}
            onClick={() => void ok()}
          >
            OK
          </CgButton>
          <CgButton data-testid="create-names-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}
