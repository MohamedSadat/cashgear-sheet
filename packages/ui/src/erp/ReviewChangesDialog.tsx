// The Review dialog (Phase 17, plan E.4): every ERP field that changed, is refused, is in conflict
// or holds a formula, as the engine lists them, in a `CgGrid` inside a `CgWindow`. Filters pick
// the invalid, the conflicting, those in rows a filter hides and the formulas; Go to cell selects
// one; Discard Selected writes the originals back as one step undo takes back; the footer commits.
// It shows what the engine answers and never reads cells itself.
import { useMemo, useState } from 'react';
import {
  CgButton,
  CgButtonGroup,
  CgGrid,
  CgWindow,
  type CgGridColumnDescriptor,
} from '@cashgear/ui';
import type { ErpReview, ErpReviewEntry } from '@cgs/engine-client';
import { count, isolate, reviewCommitButton, reviewMessage, statusText } from './messages';

export type ReviewFilter = 'all' | 'invalid' | 'conflicts' | 'hidden' | 'formulas';

const FILTERS: { name: ReviewFilter; text: string; keep: (entry: ErpReviewEntry) => boolean }[] = [
  { name: 'all', text: 'All', keep: () => true },
  { name: 'invalid', text: 'Invalid', keep: (entry) => entry.invalid },
  { name: 'conflicts', text: 'Conflicts', keep: (entry) => entry.conflict },
  { name: 'hidden', text: 'In hidden rows', keep: (entry) => entry.hidden },
  { name: 'formulas', text: 'Formulas', keep: (entry) => entry.state === 'formula' },
];

const keyOf = (entry: ErpReviewEntry) => `${entry.row}:${entry.col}`;

export interface ReviewChangesDialogProps {
  open: boolean;
  review: ErpReview | null;
  /** The entity's English noun, such as `vendors`. */
  noun: string;
  rightToLeft: boolean;
  busy: boolean;
  /** Whether a commit can be sent now: connected, with changes. */
  canCommit: boolean;
  onClose: () => void;
  onGoTo: (entry: ErpReviewEntry) => void;
  onDiscard: (entries: ErpReviewEntry[]) => void;
  onCommit: () => void;
}

export function ReviewChangesDialog(props: ReviewChangesDialogProps) {
  const { open, review, noun, rightToLeft, busy, canCommit, onClose, onGoTo, onDiscard } = props;
  const [filter, setFilter] = useState<ReviewFilter>('all');
  const [selected, setSelected] = useState<string[]>([]);
  const entries = useMemo(() => review?.entries ?? [], [review]);
  const shown = useMemo(() => {
    const keep = FILTERS.find((f) => f.name === filter)?.keep ?? (() => true);
    return entries.filter(keep);
  }, [entries, filter]);
  const chosen = useMemo(() => {
    const keys = new Set(selected);
    return shown.filter((entry) => keys.has(keyOf(entry)));
  }, [selected, shown]);

  const columns = useMemo<CgGridColumnDescriptor<ErpReviewEntry>[]>(
    () => [
      { type: 'selection', fieldId: 'selected', width: 36 },
      {
        type: 'text',
        fieldId: 'record',
        title: 'Record',
        width: 190,
        accessor: (entry) => (entry.name ? `${entry.key} · ${isolate(entry.name)}` : entry.key),
      },
      { type: 'text', fieldId: 'label', title: 'Field', width: 130, accessor: (e) => e.label },
      {
        type: 'text',
        fieldId: 'original',
        title: 'Original',
        width: 130,
        accessor: (entry) => (entry.original === undefined ? '' : isolate(entry.original)),
      },
      {
        type: 'text',
        fieldId: 'now',
        title: 'New',
        width: 130,
        accessor: (entry) => (entry.now === undefined ? '' : isolate(entry.now)),
      },
      { type: 'text', fieldId: 'status', title: 'Status', width: 110, accessor: statusText },
      {
        type: 'text',
        fieldId: 'message',
        title: 'Message',
        width: 300,
        accessor: (entry) => reviewMessage(entry, noun),
      },
    ],
    [noun],
  );

  const counts = review?.counts;
  const changed = counts?.changed ?? 0;
  const records = counts?.changedRecords ?? 0;
  const filterItems = FILTERS.map((f) => ({
    name: f.name,
    text: `${f.text} (${count(entries.filter(f.keep).length)})`,
  }));
  const first = chosen[0] ?? (shown.length === 1 ? shown[0] : undefined);

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      headerText="Review Changes"
      width={1080}
      allowDrag
      allowResize
      data-testid="erp-review"
      renderBody={() => (
        <div className="cgs-dialog-column cgs-erp-review" dir={rightToLeft ? 'rtl' : 'ltr'}>
          <CgButtonGroup
            data-testid="erp-review-filters"
            ariaLabel="Show"
            selectionMode="single"
            selectedName={filter}
            onSelectedNameChange={(name) => {
              if (name) {
                setFilter(name as ReviewFilter);
                setSelected([]);
              }
            }}
            items={filterItems}
          />
          <CgGrid<ErpReviewEntry>
            data-testid="erp-review-grid"
            data={shown}
            columns={columns}
            keySelector={keyOf}
            selectionMode="checkbox"
            onSelectionChange={(details) => setSelected([...details.selectedKeys])}
            onRowActivate={(entry) => onGoTo(entry)}
            height={360}
            stickyHeader
            direction={rightToLeft ? 'rtl' : 'ltr'}
            renderEmpty={() => <p className="cgs-dialog-message">Nothing to show here.</p>}
          />
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            data-testid="erp-review-go"
            disabled={!first}
            onClick={() => {
              if (first) onGoTo(first);
            }}
          >
            Go to Cell
          </CgButton>
          <CgButton
            data-testid="erp-review-discard"
            disabled={busy || chosen.length === 0}
            onClick={() => {
              onDiscard(chosen);
              setSelected([]);
            }}
          >
            Discard Selected
          </CgButton>
          <span className="cgs-dialog-spacer" />
          <CgButton
            intent="primary"
            data-testid="erp-review-commit"
            disabled={busy || !canCommit || changed === 0}
            onClick={props.onCommit}
          >
            {reviewCommitButton(changed, records, noun)}
          </CgButton>
          <CgButton data-testid="erp-review-close" onClick={onClose}>
            Close
          </CgButton>
        </div>
      )}
    />
  );
}
