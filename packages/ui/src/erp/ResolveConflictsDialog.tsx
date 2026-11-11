// Resolve Conflicts (Phase 18, plan E.5; decisions, 2026-11-11): every field that you and Cash
// Gear ERP both changed, as the engine lists them, with the original, yours and the ERP's now, in
// a `CgGrid` inside a `CgWindow`. Keep Mine and Take ERP's act on the selected rows, so choosing a
// record's rows resolves that record; Keep All Mine and Take All ERP's act on every conflict. The
// engine merges each touched record again, and one with nothing left in conflict takes the ERP's
// version. It shows what the engine answers and never reads cells itself.
import { useMemo, useState } from 'react';
import { CgButton, CgGrid, CgWindow, type CgGridColumnDescriptor } from '@cashgear/ui';
import type { ErpConflictEntry, ErpConflicts } from '@cgs/engine-client';
import { isolate } from './messages';
import type { ErpTexts } from './texts';

const keyOf = (entry: ErpConflictEntry) => `${entry.row}:${entry.col}`;
/** `CgGrid` reports a selected row by its key's typed token, `string:3:4` for `3:4`. */
const fromToken = (token: string) => token.replace(/^string:/, '');

/** What to resolve: chosen fields, or every conflict. */
export type Resolution =
  { choices: { key: string; field: string; take: 'mine' | 'erp' }[] } | { all: 'mine' | 'erp' };

export interface ResolveConflictsDialogProps {
  open: boolean;
  list: ErpConflicts | null;
  t: ErpTexts;
  /** The entity's English noun, such as `vendors`. */
  noun: string;
  busy: boolean;
  onClose: () => void;
  onResolve: (resolution: Resolution) => void;
  onGoTo: (entry: ErpConflictEntry) => void;
}

export function ResolveConflictsDialog(props: ResolveConflictsDialogProps) {
  const { open, list, t, noun, busy, onClose, onResolve, onGoTo } = props;
  const [selected, setSelected] = useState<string[]>([]);
  const entries = useMemo(() => list?.entries ?? [], [list]);
  const chosen = useMemo(() => {
    const keys = new Set(selected);
    return entries.filter((entry) => keys.has(keyOf(entry)));
  }, [entries, selected]);
  const columns = useMemo<CgGridColumnDescriptor<ErpConflictEntry>[]>(() => {
    const shown = (value: string | undefined) => (value === undefined ? t.EMPTY : isolate(value));
    return [
      { type: 'selection', fieldId: 'selected', width: 36 },
      {
        type: 'text',
        fieldId: 'record',
        title: t.COLUMN_RECORD,
        width: 180,
        accessor: (entry) => (entry.name ? `${entry.key} · ${isolate(entry.name)}` : entry.key),
      },
      {
        type: 'text',
        fieldId: 'label',
        title: t.COLUMN_FIELD,
        width: 130,
        accessor: (e) => e.label,
      },
      {
        type: 'text',
        fieldId: 'original',
        title: t.COLUMN_ORIGINAL,
        width: 130,
        accessor: (entry) => shown(entry.original),
      },
      {
        type: 'text',
        fieldId: 'mine',
        title: t.COLUMN_MINE,
        width: 130,
        accessor: (entry) => shown(entry.mine),
      },
      {
        type: 'text',
        fieldId: 'erp',
        title: t.COLUMN_ERP,
        width: 150,
        accessor: (entry) => (entry.known ? shown(entry.erp) : t.UNKNOWN_VALUE),
      },
      {
        type: 'text',
        fieldId: 'by',
        title: t.COLUMN_BY,
        width: 190,
        accessor: (entry) => t.changedAt(entry.by, entry.at),
      },
    ];
  }, [t]);

  const resolveChosen = (take: 'mine' | 'erp') => {
    const choices = chosen
      .filter((entry) => entry.known)
      .map((entry) => ({ key: entry.key, field: entry.field, take }));
    if (choices.length === 0) return;
    onResolve({ choices });
    setSelected([]);
  };
  const first = chosen[0] ?? (entries.length === 1 ? entries[0] : undefined);
  const none = entries.length === 0;

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      headerText={t.RESOLVE_TITLE}
      width={1020}
      allowDrag
      allowResize
      data-testid="erp-resolve"
      renderBody={() => (
        <div className="cgs-dialog-column cgs-erp-review" dir={t.dir}>
          <p className="cgs-dialog-message" data-testid="erp-resolve-intro">
            {none ? t.NO_CONFLICTS_LEFT : t.resolveIntro(list?.records ?? 0, noun)}
          </p>
          <CgGrid<ErpConflictEntry>
            data-testid="erp-resolve-grid"
            data={entries}
            columns={columns}
            keySelector={keyOf}
            selectionMode="checkbox"
            onSelectionChange={(details) => setSelected(details.selectedKeys.map(fromToken))}
            onRowActivate={(entry) => onGoTo(entry)}
            height={320}
            stickyHeader
            direction={t.dir}
            renderEmpty={() => <p className="cgs-dialog-message">{t.NOTHING_HERE}</p>}
          />
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions" dir={t.dir}>
          <CgButton
            data-testid="erp-resolve-mine"
            disabled={busy || chosen.length === 0}
            onClick={() => resolveChosen('mine')}
          >
            {t.KEEP_MINE}
          </CgButton>
          <CgButton
            data-testid="erp-resolve-erp"
            disabled={busy || !chosen.some((entry) => entry.known)}
            onClick={() => resolveChosen('erp')}
          >
            {t.TAKE_ERP}
          </CgButton>
          <span className="cgs-dialog-spacer" />
          <CgButton
            data-testid="erp-resolve-all-mine"
            disabled={busy || none}
            onClick={() => onResolve({ all: 'mine' })}
          >
            {t.KEEP_ALL_MINE}
          </CgButton>
          <CgButton
            data-testid="erp-resolve-all-erp"
            disabled={busy || none}
            onClick={() => onResolve({ all: 'erp' })}
          >
            {t.TAKE_ALL_ERP}
          </CgButton>
          <CgButton
            data-testid="erp-resolve-go"
            disabled={!first}
            onClick={() => {
              if (first) onGoTo(first);
            }}
          >
            {t.GO_TO_CELL}
          </CgButton>
          <CgButton intent="primary" data-testid="erp-resolve-close" onClick={onClose}>
            {t.CLOSE}
          </CgButton>
        </div>
      )}
    />
  );
}
