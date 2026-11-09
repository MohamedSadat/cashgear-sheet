// Sheet and workbook protection as Excel 2016 has it: Review › Protect Sheet and Protect Workbook,
// their password prompts, and the message a locked cell gives.
import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { CgButton, CgMessageBox, CgWindow, type CgRibbonGroup } from '@cashgear/ui';
import type { Applied, BookSummary, Command, ProtectAllow } from '@cgs/engine-client';

/** Excel 2016's words when a change would touch a locked cell. */
export const PROTECTED_MESSAGE =
  "The cell or chart you're trying to change is on a protected sheet. To make a change, " +
  'unprotect the sheet. You might be requested to enter a password.';

/** Protect Sheet's list, in Excel's order and words. */
export const ALLOW_LIST: ReadonlyArray<[keyof ProtectAllow, string]> = [
  ['selectLocked', 'Select locked cells'],
  ['selectUnlocked', 'Select unlocked cells'],
  ['formatCells', 'Format cells'],
  ['formatColumns', 'Format columns'],
  ['formatRows', 'Format rows'],
  ['insertColumns', 'Insert columns'],
  ['insertRows', 'Insert rows'],
  ['insertHyperlinks', 'Insert hyperlinks'],
  ['deleteColumns', 'Delete columns'],
  ['deleteRows', 'Delete rows'],
  ['sort', 'Sort'],
  ['autoFilter', 'Use AutoFilter'],
  ['pivotTables', 'Use PivotTable & PivotChart'],
  ['editObjects', 'Edit objects'],
  ['editScenarios', 'Edit scenarios'],
];

const DEFAULT_ALLOW: ProtectAllow = {
  selectLocked: true,
  selectUnlocked: true,
  formatCells: false,
  formatColumns: false,
  formatRows: false,
  insertColumns: false,
  insertRows: false,
  insertHyperlinks: false,
  deleteColumns: false,
  deleteRows: false,
  sort: false,
  autoFilter: false,
  pivotTables: false,
  editObjects: false,
  editScenarios: false,
};

/** 16 random bytes for a new password's salt, as Excel uses. */
export function newSalt(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return btoa(String.fromCharCode(...bytes));
}

type ProtectDialog =
  | { kind: 'protectSheet'; allow: ProtectAllow; password: string; contents: boolean }
  | { kind: 'protectWorkbook'; password: string; structure: boolean }
  | { kind: 'confirm'; then: 'sheet' | 'workbook'; password: string; again: string }
  | { kind: 'unprotect'; what: 'sheet' | 'workbook'; password: string; error: string }
  | { kind: 'message'; text: string };

export interface ProtectToolsOptions {
  book: BookSummary | null;
  sheet: number;
  hasBook: boolean;
  run: (command: Command) => Promise<Applied>;
  focusGrid: () => void;
}

export interface ProtectTools {
  /** What the sheet showing allows, when it is protected. */
  protection: (ProtectAllow & { password: boolean }) | null;
  structureLocked: boolean;
  /** Protect Sheet, or Unprotect Sheet on a protected one. */
  toggleSheet: () => void;
  toggleWorkbook: () => void;
  /** Shows a refusal as Excel does, in a message box. */
  showMessage: (text: string) => void;
  /** Review's Protect group. */
  group: CgRibbonGroup;
  overlays: ReactNode;
}

export function useProtectTools(options: ProtectToolsOptions): ProtectTools {
  const { book, sheet, run, focusGrid } = options;
  const [dialog, setDialog] = useState<ProtectDialog | null>(null);
  const [pending, setPending] = useState<
    { what: 'sheet'; allow: ProtectAllow } | { what: 'workbook'; structure: boolean } | null
  >(null);
  const protection = book?.sheets[sheet]?.protection ?? null;
  const structureLocked = book?.structureLocked ?? false;

  const close = useCallback(() => {
    setDialog(null);
    focusGrid();
  }, [focusGrid]);

  const report = useCallback(
    (failure: unknown) =>
      setDialog({
        kind: 'message',
        text: failure instanceof Error ? failure.message : String(failure),
      }),
    [],
  );

  const protectSheet = useCallback(
    (allow: ProtectAllow, password: string) => {
      close();
      run({ kind: 'protectSheet', sheet, allow, password, salt: password ? newSalt() : '' }).catch(
        report,
      );
    },
    [close, report, run, sheet],
  );

  const protectWorkbook = useCallback(
    (structure: boolean, password: string) => {
      close();
      run({ kind: 'protectWorkbook', structure, password, salt: password ? newSalt() : '' }).catch(
        report,
      );
    },
    [close, report, run],
  );

  const toggleSheet = useCallback(() => {
    if (!protection) {
      setDialog({ kind: 'protectSheet', allow: DEFAULT_ALLOW, password: '', contents: true });
    } else if (protection.password) {
      setDialog({ kind: 'unprotect', what: 'sheet', password: '', error: '' });
    } else {
      run({ kind: 'unprotectSheet', sheet }).catch(report);
    }
  }, [protection, report, run, sheet]);

  const toggleWorkbook = useCallback(() => {
    if (!structureLocked) {
      setDialog({ kind: 'protectWorkbook', password: '', structure: true });
    } else if (book?.structurePassword) {
      setDialog({ kind: 'unprotect', what: 'workbook', password: '', error: '' });
    } else {
      run({ kind: 'unprotectWorkbook' }).catch(report);
    }
  }, [book?.structurePassword, report, run, structureLocked]);

  const showMessage = useCallback((text: string) => setDialog({ kind: 'message', text }), []);

  const group = useMemo<CgRibbonGroup>(
    () => ({
      key: 'protect',
      text: 'Protect',
      icon: 'table',
      items: [
        {
          name: 'protect-sheet',
          text: protection ? 'Unprotect Sheet' : 'Protect Sheet',
          icon: 'table',
          size: 'large',
          disabled: !options.hasBook || book?.sheets[sheet]?.kind !== 'worksheet',
          onClick: toggleSheet,
        },
        {
          name: 'protect-workbook',
          text: 'Protect Workbook',
          icon: 'workbook',
          size: 'large',
          checked: structureLocked,
          disabled: !options.hasBook,
          onClick: toggleWorkbook,
        },
      ],
    }),
    [book, options.hasBook, protection, sheet, structureLocked, toggleSheet, toggleWorkbook],
  );

  let overlays: ReactNode = null;
  if (dialog?.kind === 'protectSheet') {
    const ok = () => {
      if (!dialog.contents) return close();
      if (dialog.password) {
        setPending({ what: 'sheet', allow: dialog.allow });
        setDialog({ kind: 'confirm', then: 'sheet', password: dialog.password, again: '' });
      } else {
        protectSheet(dialog.allow, '');
      }
    };
    overlays = (
      <CgWindow
        open
        onOpenChange={(next) => {
          if (!next) close();
        }}
        headerText="Protect Sheet"
        width={340}
        allowDrag
        data-testid="protect-sheet-dialog"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <label className="cgs-dialog-check">
              <input
                type="checkbox"
                data-testid="protect-contents"
                checked={dialog.contents}
                onChange={(event) => setDialog({ ...dialog, contents: event.target.checked })}
              />
              Protect worksheet and contents of locked cells
            </label>
            <label className="cgs-dialog-field">
              <span>Password to unprotect sheet:</span>
              <input
                type="password"
                data-testid="protect-password"
                autoFocus
                maxLength={255}
                value={dialog.password}
                onChange={(event) => setDialog({ ...dialog, password: event.target.value })}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault();
                    ok();
                  }
                }}
              />
            </label>
            <span>Allow all users of this worksheet to:</span>
            <div className="cgs-dialog-list" data-testid="protect-allow">
              {ALLOW_LIST.map(([key, text]) => (
                <label key={key} className="cgs-dialog-check">
                  <input
                    type="checkbox"
                    data-testid={'allow-' + key}
                    checked={dialog.allow[key]}
                    onChange={(event) => {
                      const allow = { ...dialog.allow, [key]: event.target.checked };
                      // Selecting locked cells needs unlocked ones selectable too, as in Excel.
                      if (key === 'selectLocked' && allow.selectLocked) allow.selectUnlocked = true;
                      if (key === 'selectUnlocked' && !allow.selectUnlocked) {
                        allow.selectLocked = false;
                      }
                      setDialog({ ...dialog, allow });
                    }}
                  />
                  {text}
                </label>
              ))}
            </div>
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton intent="primary" data-testid="protect-ok" onClick={ok}>
              OK
            </CgButton>
            <CgButton data-testid="protect-cancel" onClick={close}>
              Cancel
            </CgButton>
          </div>
        )}
      />
    );
  } else if (dialog?.kind === 'protectWorkbook') {
    const ok = () => {
      if (!dialog.structure) return close();
      if (dialog.password) {
        setPending({ what: 'workbook', structure: dialog.structure });
        setDialog({ kind: 'confirm', then: 'workbook', password: dialog.password, again: '' });
      } else {
        protectWorkbook(true, '');
      }
    };
    overlays = (
      <CgWindow
        open
        onOpenChange={(next) => {
          if (!next) close();
        }}
        headerText="Protect Structure and Windows"
        width={320}
        allowDrag
        data-testid="protect-workbook-dialog"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <span>Protect workbook for</span>
            <label className="cgs-dialog-check">
              <input
                type="checkbox"
                data-testid="protect-structure"
                checked={dialog.structure}
                onChange={(event) => setDialog({ ...dialog, structure: event.target.checked })}
              />
              Structure
            </label>
            <label className="cgs-dialog-check">
              {/* Excel 2016 shows it greyed: each workbook has a window of its own. */}
              <input type="checkbox" data-testid="protect-windows" disabled checked={false} />
              Windows
            </label>
            <label className="cgs-dialog-field">
              <span>Password (optional):</span>
              <input
                type="password"
                data-testid="protect-workbook-password"
                autoFocus
                maxLength={255}
                value={dialog.password}
                onChange={(event) => setDialog({ ...dialog, password: event.target.value })}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault();
                    ok();
                  }
                }}
              />
            </label>
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton intent="primary" data-testid="protect-workbook-ok" onClick={ok}>
              OK
            </CgButton>
            <CgButton data-testid="protect-workbook-cancel" onClick={close}>
              Cancel
            </CgButton>
          </div>
        )}
      />
    );
  } else if (dialog?.kind === 'confirm') {
    const ok = () => {
      if (dialog.again !== dialog.password) {
        setPending(null);
        setDialog({ kind: 'message', text: 'Confirmation password is not identical.' });
        return;
      }
      if (pending?.what === 'sheet') protectSheet(pending.allow, dialog.password);
      if (pending?.what === 'workbook') protectWorkbook(pending.structure, dialog.password);
      setPending(null);
    };
    overlays = (
      <CgWindow
        open
        onOpenChange={(next) => {
          if (!next) close();
        }}
        headerText="Confirm Password"
        width={360}
        allowDrag
        data-testid="confirm-password-dialog"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <label className="cgs-dialog-field">
              <span>Reenter password to proceed.</span>
              <input
                type="password"
                data-testid="confirm-password"
                autoFocus
                value={dialog.again}
                onChange={(event) => setDialog({ ...dialog, again: event.target.value })}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault();
                    ok();
                  }
                }}
              />
            </label>
            <p className="cgs-dialog-message">
              Caution: If you lose or forget the password, it cannot be recovered. It is advisable
              to keep a list of passwords and their corresponding workbook and sheet names in a safe
              place. (Remember that passwords are case-sensitive.)
            </p>
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton intent="primary" data-testid="confirm-password-ok" onClick={ok}>
              OK
            </CgButton>
            <CgButton data-testid="confirm-password-cancel" onClick={close}>
              Cancel
            </CgButton>
          </div>
        )}
      />
    );
  } else if (dialog?.kind === 'unprotect') {
    const ok = () => {
      const command: Command =
        dialog.what === 'sheet'
          ? { kind: 'unprotectSheet', sheet, password: dialog.password }
          : { kind: 'unprotectWorkbook', password: dialog.password };
      // The dialog goes at once, so the keyboard isn't taken back once the user moves on.
      close();
      run(command).catch(report);
    };
    overlays = (
      <CgWindow
        open
        onOpenChange={(next) => {
          if (!next) close();
        }}
        headerText={dialog.what === 'sheet' ? 'Unprotect Sheet' : 'Unprotect Workbook'}
        width={300}
        allowDrag
        data-testid="unprotect-dialog"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <label className="cgs-dialog-field">
              <span>Password:</span>
              <input
                type="password"
                data-testid="unprotect-password"
                autoFocus
                value={dialog.password}
                onChange={(event) => setDialog({ ...dialog, password: event.target.value })}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') {
                    event.preventDefault();
                    ok();
                  }
                }}
              />
            </label>
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton intent="primary" data-testid="unprotect-ok" onClick={ok}>
              OK
            </CgButton>
            <CgButton data-testid="unprotect-cancel" onClick={close}>
              Cancel
            </CgButton>
          </div>
        )}
      />
    );
  } else if (dialog?.kind === 'message') {
    overlays = (
      <CgMessageBox
        defaultOpen
        title="CashGear Sheet"
        actionLabel="OK"
        data-testid="protect-message"
        onClosed={close}
      >
        <p data-testid="protect-message-text">{dialog.text}</p>
      </CgMessageBox>
    );
  }

  return {
    protection,
    structureLocked,
    toggleSheet,
    toggleWorkbook,
    showMessage,
    group,
    overlays,
  };
}
