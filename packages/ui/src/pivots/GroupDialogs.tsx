// Excel 2016's Grouping dialog for a pivot table's date or number field, and Show Report Filter
// Pages. Both only gather what to send; the engine does the grouping and the copying.
import { useState } from 'react';
import { CgButton, CgWindow } from '@cashgear/ui';
import type { Command, PivotFieldInfo } from '@cgs/engine-client';

type GroupSpec = Extract<Command, { kind: 'groupPivot' }>['group'];

/** The periods offered, finest first, as Excel's By list orders them. */
export const DATE_PARTS = ['days', 'months', 'quarters', 'years'] as const;
const PART_LABELS: Record<(typeof DATE_PARTS)[number], string> = {
  days: 'Days',
  months: 'Months',
  quarters: 'Quarters',
  years: 'Years',
};

/** A date serial, from 1900's system, as m/d/yyyy. */
export function serialText(serial: number) {
  // Serial 60 is Excel's 29 February 1900, which never was.
  const days = Math.floor(serial) - (serial < 61 ? 1 : 2);
  const date = new Date(Date.UTC(1900, 0, 1) + days * 86_400_000);
  return `${date.getUTCMonth() + 1}/${date.getUTCDate()}/${date.getUTCFullYear()}`;
}

/** m/d/yyyy as a date serial, or null if it isn't a date. */
export function textSerial(text: string): number | null {
  const match = /^\s*(\d{1,2})\/(\d{1,2})\/(\d{4})\s*$/.exec(text);
  if (!match) return null;
  const time = Date.UTC(Number(match[3]), Number(match[1]) - 1, Number(match[2]));
  const days = Math.round((time - Date.UTC(1900, 0, 1)) / 86_400_000);
  return days + (days < 59 ? 1 : 2);
}

export interface GroupingDialogProps {
  field: PivotFieldInfo;
  onGroup: (group: GroupSpec) => void;
  onClose: () => void;
}

export function GroupingDialog({ field, onGroup, onClose }: GroupingDialogProps) {
  const dates = field.dates;
  const low = field.low ?? 0;
  // Excel ends a date range the day after the last date.
  const high = dates ? Math.floor(field.high ?? 0) + 1 : (field.high ?? 0);
  const shown = (n: number) => (dates ? serialText(n) : String(n));
  const [autoStart, setAutoStart] = useState(true);
  const [autoEnd, setAutoEnd] = useState(true);
  const [start, setStart] = useState(shown(dates ? Math.floor(low) : low));
  const [end, setEnd] = useState(shown(high));
  const [by, setBy] = useState('10');
  const [parts, setParts] = useState<string[]>(['months']);
  const [problem, setProblem] = useState<string | null>(null);

  const read = (text: string) =>
    dates
      ? textSerial(text)
      : Number.isFinite(Number(text)) && text.trim() !== ''
        ? Number(text)
        : null;
  const ok = () => {
    const from = autoStart ? undefined : read(start);
    const to = autoEnd ? undefined : read(end);
    if (from === null || to === null) {
      setProblem(dates ? 'Enter a date such as 1/1/2016.' : 'Enter a number.');
      return;
    }
    const range = {
      ...(from !== undefined ? { start: from } : {}),
      ...(to !== undefined ? { end: to } : {}),
    };
    if (dates) {
      if (!parts.length) {
        setProblem('Select at least one way to group.');
        return;
      }
      onGroup({ kind: 'dates', parts, ...range });
    } else {
      const step = Number(by);
      if (!(step > 0)) {
        setProblem('Enter a number greater than zero in By.');
        return;
      }
      onGroup({ kind: 'numbers', by: step, ...range });
    }
    onClose();
  };

  const bound = (
    label: string,
    auto: boolean,
    setAuto: (on: boolean) => void,
    value: string,
    setValue: (text: string) => void,
    key: string,
  ) => (
    <label className="cgs-dialog-row">
      <input
        type="checkbox"
        data-testid={`group-auto-${key}`}
        checked={auto}
        onChange={(event) => setAuto(event.target.checked)}
      />
      <span style={{ minWidth: 80 }}>{label}</span>
      <input
        data-testid={`group-${key}`}
        value={value}
        disabled={auto}
        onChange={(event) => setValue(event.target.value)}
      />
    </label>
  );

  return (
    <CgWindow
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      headerText="Grouping"
      width={320}
      allowDrag
      data-testid="grouping-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <span>Auto</span>
          {bound('Starting at:', autoStart, setAutoStart, start, setStart, 'start')}
          {bound('Ending at:', autoEnd, setAutoEnd, end, setEnd, 'end')}
          <span>By</span>
          {dates ? (
            <div role="listbox" aria-multiselectable="true" aria-label="By">
              {DATE_PARTS.map((part) => (
                <label key={part} className="cgs-dialog-check">
                  <input
                    type="checkbox"
                    data-testid={`group-by-${part}`}
                    checked={parts.includes(part)}
                    onChange={(event) =>
                      setParts((list) =>
                        event.target.checked
                          ? [...list, part]
                          : list.filter((entry) => entry !== part),
                      )
                    }
                  />
                  {PART_LABELS[part]}
                </label>
              ))}
            </div>
          ) : (
            <input
              data-testid="group-by"
              aria-label="By"
              value={by}
              onChange={(event) => setBy(event.target.value)}
            />
          )}
          {problem && <span role="alert">{problem}</span>}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="grouping-ok" onClick={ok}>
            OK
          </CgButton>
          <CgButton data-testid="grouping-cancel" onClick={onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

export interface FilterPagesDialogProps {
  /** The report filters, by field index, with their captions. */
  fields: { field: number; caption: string }[];
  onShow: (field: number) => void;
  onClose: () => void;
}

/** Show Report Filter Pages: which report filter to make a sheet for each item of. */
export function FilterPagesDialog({ fields, onShow, onClose }: FilterPagesDialogProps) {
  const [chosen, setChosen] = useState(fields[0]?.field ?? -1);
  const ok = () => {
    if (chosen < 0) return;
    onShow(chosen);
    onClose();
  };
  return (
    <CgWindow
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      headerText="Show Report Filter Pages"
      width={300}
      allowDrag
      data-testid="filter-pages-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <span>Show all report filter pages of:</span>
          <select
            size={Math.max(2, Math.min(8, fields.length))}
            data-testid="filter-pages-field"
            aria-label="Report filter"
            value={chosen}
            onChange={(event) => setChosen(Number(event.target.value))}
          >
            {fields.map(({ field, caption }) => (
              <option key={field} value={field}>
                {caption}
              </option>
            ))}
          </select>
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            intent="primary"
            data-testid="filter-pages-ok"
            disabled={chosen < 0}
            onClick={ok}
          >
            OK
          </CgButton>
          <CgButton data-testid="filter-pages-cancel" onClick={onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}
