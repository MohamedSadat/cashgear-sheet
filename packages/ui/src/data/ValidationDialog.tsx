// Data Validation: Settings, Input Message and Error Alert. It starts from the active cell's rule and
// puts what's set here on the whole selection, as in Excel.
import { useEffect, useState } from 'react';
import { CgButton, CgTabs, CgWindow } from '@cashgear/ui';
import type {
  Applied,
  CellArea,
  Command,
  EngineClient,
  ValidationInfo,
  ValidationKind,
  ValidationOp,
  ValidationRule,
} from '@cgs/engine-client';

export interface ValidationDialogProps {
  open: boolean;
  engine: () => EngineClient | null;
  sheet: number;
  /** The selection's areas, 1-based. */
  areas: CellArea[];
  /** The active cell, 1-based, whose rule the dialog starts from. */
  row: number;
  col: number;
  run: (command: Command) => Promise<Applied>;
  onClose: () => void;
}

/** Everything the dialog shows, with the formulas as typed. */
export interface ValidationForm {
  kind: ValidationKind;
  op: ValidationOp;
  formula1: string;
  formula2: string;
  allowBlank: boolean;
  dropdown: boolean;
  showInput: boolean;
  inputTitle: string;
  inputText: string;
  showError: boolean;
  errorTitle: string;
  errorText: string;
  style: 'stop' | 'warning' | 'information';
  /** A rule from newer Excel, shown but not edited. */
  extended: boolean;
}

export const KINDS: [ValidationKind, string][] = [
  ['any', 'Any value'],
  ['whole', 'Whole number'],
  ['decimal', 'Decimal'],
  ['list', 'List'],
  ['date', 'Date'],
  ['time', 'Time'],
  ['textLength', 'Text length'],
  ['custom', 'Custom'],
];

export const OPS: [ValidationOp, string][] = [
  ['between', 'between'],
  ['notBetween', 'not between'],
  ['equal', 'equal to'],
  ['notEqual', 'not equal to'],
  ['greaterThan', 'greater than'],
  ['lessThan', 'less than'],
  ['greaterThanOrEqual', 'greater than or equal to'],
  ['lessThanOrEqual', 'less than or equal to'],
];

const STYLES = [
  ['stop', 'Stop'],
  ['warning', 'Warning'],
  ['information', 'Information'],
] as const;

/** A new cell's settings, as Excel starts them. */
export const EMPTY_FORM: ValidationForm = {
  kind: 'any',
  op: 'between',
  formula1: '',
  formula2: '',
  allowBlank: true,
  dropdown: true,
  showInput: true,
  inputTitle: '',
  inputText: '',
  showError: true,
  errorTitle: '',
  errorText: '',
  style: 'stop',
  extended: false,
};

const usesOp = (kind: ValidationKind) => kind !== 'any' && kind !== 'list' && kind !== 'custom';

/** The boxes under Data, as Excel labels them for this choice: one or two, or none. */
export function fieldLabels(kind: ValidationKind, op: ValidationOp): string[] {
  if (kind === 'any') return [];
  if (kind === 'list') return ['Source'];
  if (kind === 'custom') return ['Formula'];
  const [low, high, one] =
    kind === 'date'
      ? ['Start date', 'End date', 'Date']
      : kind === 'time'
        ? ['Start time', 'End time', 'Time']
        : ['Minimum', 'Maximum', kind === 'textLength' ? 'Length' : 'Value'];
  switch (op) {
    case 'between':
    case 'notBetween':
      return [low, high];
    case 'equal':
    case 'notEqual':
      return [one];
    case 'greaterThan':
    case 'greaterThanOrEqual':
      return [low];
    case 'lessThan':
    case 'lessThanOrEqual':
      return [high];
  }
}

const DAY = 86_400_000;
const EPOCH = Date.UTC(1899, 11, 30);

/** A 1900-system serial for a date, with Excel's 29 February 1900 kept in. */
function serialOf(year: number, month: number, day: number): number | null {
  if (month < 1 || month > 12 || day < 1 || day > 31) return null;
  if (year === 1900 && month === 2 && day === 29) return 60;
  const time = Date.UTC(year, month - 1, day);
  const check = new Date(time);
  if (check.getUTCMonth() !== month - 1 || check.getUTCDate() !== day) return null;
  const serial = Math.round((time - EPOCH) / DAY);
  if (serial < 2) return null;
  return serial <= 60 ? serial - 1 : serial;
}

function dateOf(serial: number): string {
  if (serial === 60) return '2/29/1900';
  const date = new Date(EPOCH + (serial < 60 ? serial + 1 : serial) * DAY);
  return `${date.getUTCMonth() + 1}/${date.getUTCDate()}/${date.getUTCFullYear()}`;
}

/** A time of day typed as `h:mm`, `h:mm:ss`, either with AM or PM, as a fraction of a day. */
export function parseTime(text: string): number | null {
  const match = /^(\d{1,2}):(\d{2})(?::(\d{2}))?\s*([ap]m?)?$/i.exec(text.trim());
  if (!match) return null;
  let hours = Number(match[1]);
  const minutes = Number(match[2]);
  const seconds = Number(match[3] ?? 0);
  const half = match[4]?.[0]?.toLowerCase();
  if (half) {
    if (hours < 1 || hours > 12) return null;
    hours = (hours % 12) + (half === 'p' ? 12 : 0);
  }
  if (hours > 23 || minutes > 59 || seconds > 59) return null;
  return (hours * 3600 + minutes * 60 + seconds) / 86_400;
}

/** A date typed as `m/d/yyyy` or `yyyy-mm-dd`, perhaps with a time, as a serial. */
export function parseDate(text: string): number | null {
  const trimmed = text.trim();
  const [day, ...rest] = trimmed.split(/\s+/);
  const us = /^(\d{1,2})\/(\d{1,2})\/(\d{4})$/.exec(day ?? '');
  const iso = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(day ?? '');
  const serial = us
    ? serialOf(Number(us[3]), Number(us[1]), Number(us[2]))
    : iso
      ? serialOf(Number(iso[1]), Number(iso[2]), Number(iso[3]))
      : null;
  if (serial === null) return null;
  if (rest.length === 0) return serial;
  const time = parseTime(rest.join(' '));
  return time === null ? null : serial + time;
}

function timeOf(fraction: number): string {
  const total = Math.round(fraction * 86_400) % 86_400;
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const two = (value: number) => String(value).padStart(2, '0');
  return `${hours % 12 || 12}:${two(minutes)}:${two(seconds)} ${hours < 12 ? 'AM' : 'PM'}`;
}

const NUMBER = /^-?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?$/i;

/** A rule's formula as the dialog's box shows it: lists bare, numbers and dates as values, the rest after `=`. */
export function shownFormula(kind: ValidationKind, formula: string | null): string {
  if (formula === null || formula === '') return '';
  if (kind === 'list' && formula.length >= 2 && formula.startsWith('"') && formula.endsWith('"')) {
    return formula.slice(1, -1).replace(/""/g, '"');
  }
  if (NUMBER.test(formula)) {
    const value = Number(formula);
    if (kind === 'date' && value >= 1) {
      const day = Math.floor(value);
      return value === day ? dateOf(day) : `${dateOf(day)} ${timeOf(value - day)}`;
    }
    if (kind === 'time' && value >= 0 && value < 1) return timeOf(value);
    return formula;
  }
  return '=' + formula;
}

/** What a box's text goes to the engine as: a list's items in quotes, dates and times as numbers. */
export function storedFormula(kind: ValidationKind, text: string): string {
  const trimmed = text.trim();
  if (trimmed === '' || trimmed.startsWith('=')) return trimmed;
  if (kind === 'list') return '"' + trimmed.replace(/"/g, '""') + '"';
  if (kind === 'date') {
    const serial = parseDate(trimmed);
    if (serial !== null) return String(serial);
  }
  if (kind === 'time') {
    const time = parseTime(trimmed);
    if (time !== null) return String(time);
  }
  return trimmed;
}

/** The dialog's settings for a cell's rule, or a new cell's. */
export function formOf(info: ValidationInfo | null): ValidationForm {
  if (!info) return { ...EMPTY_FORM };
  return {
    kind: info.kind,
    op: info.op,
    formula1: shownFormula(info.kind, info.formula1),
    formula2: shownFormula(info.kind, info.formula2),
    allowBlank: info.allowBlank,
    dropdown: info.kind === 'list' ? info.dropdown : true,
    showInput: info.showInput,
    inputTitle: info.inputTitle,
    inputText: info.inputText,
    showError: info.showError,
    errorTitle: info.errorTitle,
    errorText: info.errorText,
    style: info.style,
    extended: info.extended,
  };
}

/**
 * The rule OK puts on the selection: none when it allows anything and has no input message,
 * which takes rules off, or what's missing, in Excel's words.
 */
export function buildRule(
  form: ValidationForm,
): { rule: ValidationRule | undefined } | { error: string } {
  const message = form.inputTitle.trim() !== '' || form.inputText.trim() !== '';
  if (form.kind === 'any' && !message) return { rule: undefined };
  const labels = fieldLabels(form.kind, form.op);
  const boxes = [form.formula1, form.formula2];
  for (const [index, label] of labels.entries()) {
    if ((boxes[index] ?? '').trim() === '') {
      return { error: `You must enter a value in the ${label} box.` };
    }
  }
  // A one-box choice fills formula1, whichever bound it is, as Excel saves it.
  const [formula1, formula2] = labels.map((_, index) =>
    storedFormula(form.kind, boxes[index] ?? ''),
  );
  const rule: ValidationRule = {
    kind: form.kind,
    ...(usesOp(form.kind) ? { op: form.op } : {}),
    ...(formula1 !== undefined ? { formula1 } : {}),
    ...(formula2 !== undefined ? { formula2 } : {}),
    allowBlank: form.allowBlank,
    ...(form.kind === 'list' ? { dropdown: form.dropdown } : {}),
    showInput: form.showInput,
    inputTitle: form.inputTitle,
    inputText: form.inputText,
    showError: form.showError,
    errorTitle: form.errorTitle,
    errorText: form.errorText,
    style: form.style,
  };
  return { rule };
}

export function ValidationDialog(props: ValidationDialogProps) {
  const { open, sheet, row, col } = props;
  const [tab, setTab] = useState('settings');
  const [form, setForm] = useState<ValidationForm>(EMPTY_FORM);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    let live = true;
    setTab('settings');
    setForm({ ...EMPTY_FORM });
    setError('');
    const engine = props.engine();
    if (engine) {
      engine
        .query({ kind: 'validationAt', sheet, row, col })
        // An ERP lookup's dropdown is no rule of the file's, so there is none to edit.
        .then((info) => live && setForm(formOf(info?.lookup ? null : info)))
        .catch((reason: unknown) => {
          if (live) setError(reason instanceof Error ? reason.message : String(reason));
        });
    }
    return () => {
      live = false;
    };
    // The rule is read once each time the dialog opens.
  }, [open, sheet, row, col]);

  const change = (part: Partial<ValidationForm>) => setForm((current) => ({ ...current, ...part }));
  const locked = form.extended;
  const labels = fieldLabels(form.kind, form.op);

  const apply = async () => {
    const built = buildRule(form);
    if ('error' in built) {
      setError(built.error);
      setTab('settings');
      return;
    }
    setBusy(true);
    try {
      await props.run({
        kind: 'setValidation',
        sheet,
        areas: props.areas,
        ...(built.rule ? { rule: built.rule } : {}),
      });
      props.onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  const settings = (
    <fieldset className="cgs-dialog-column" disabled={locked} style={{ border: 0, margin: 0 }}>
      <span>Validation criteria</span>
      <div className="cgs-dialog-row">
        <label className="cgs-dialog-field cgs-dialog-grow">
          <span>Allow</span>
          <select
            data-testid="validation-kind"
            autoFocus
            value={form.kind}
            onChange={(event) =>
              // Excel empties the boxes when the choice changes.
              change({ kind: event.target.value as ValidationKind, formula1: '', formula2: '' })
            }
          >
            {KINDS.map(([value, text]) => (
              <option key={value} value={value}>
                {text}
              </option>
            ))}
          </select>
        </label>
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="validation-blank"
            disabled={form.kind === 'any'}
            checked={form.allowBlank}
            onChange={(event) => change({ allowBlank: event.target.checked })}
          />
          Ignore blank
        </label>
      </div>
      <div className="cgs-dialog-row">
        <label className="cgs-dialog-field cgs-dialog-grow">
          <span>Data</span>
          <select
            data-testid="validation-op"
            disabled={!usesOp(form.kind)}
            value={form.op}
            onChange={(event) => change({ op: event.target.value as ValidationOp })}
          >
            {OPS.map(([value, text]) => (
              <option key={value} value={value}>
                {text}
              </option>
            ))}
          </select>
        </label>
        {form.kind === 'list' && (
          <label className="cgs-dialog-check">
            <input
              type="checkbox"
              data-testid="validation-dropdown"
              checked={form.dropdown}
              onChange={(event) => change({ dropdown: event.target.checked })}
            />
            In-cell dropdown
          </label>
        )}
      </div>
      {labels.map((label, index) => (
        <label key={label} className="cgs-dialog-field">
          <span>{label}</span>
          <input
            data-testid={index === 0 ? 'validation-formula1' : 'validation-formula2'}
            value={index === 0 ? form.formula1 : form.formula2}
            onChange={(event) =>
              change(
                index === 0 ? { formula1: event.target.value } : { formula2: event.target.value },
              )
            }
          />
        </label>
      ))}
    </fieldset>
  );

  const input = (
    <fieldset className="cgs-dialog-column" disabled={locked} style={{ border: 0, margin: 0 }}>
      <label className="cgs-dialog-check">
        <input
          type="checkbox"
          data-testid="validation-show-input"
          checked={form.showInput}
          onChange={(event) => change({ showInput: event.target.checked })}
        />
        Show input message when cell is selected
      </label>
      <span>When cell is selected, show this input message:</span>
      <label className="cgs-dialog-field">
        <span>Title</span>
        <input
          data-testid="validation-input-title"
          maxLength={32}
          value={form.inputTitle}
          onChange={(event) => change({ inputTitle: event.target.value })}
        />
      </label>
      <label className="cgs-dialog-field">
        <span>Input message</span>
        <textarea
          data-testid="validation-input-text"
          rows={5}
          maxLength={255}
          value={form.inputText}
          onChange={(event) => change({ inputText: event.target.value })}
        />
      </label>
    </fieldset>
  );

  const alert = (
    <fieldset className="cgs-dialog-column" disabled={locked} style={{ border: 0, margin: 0 }}>
      <label className="cgs-dialog-check">
        <input
          type="checkbox"
          data-testid="validation-show-error"
          checked={form.showError}
          onChange={(event) => change({ showError: event.target.checked })}
        />
        Show error alert after invalid data is entered
      </label>
      <span>When user enters invalid data, show this error alert:</span>
      <div className="cgs-dialog-row">
        <label className="cgs-dialog-field">
          <span>Style</span>
          <select
            data-testid="validation-style"
            value={form.style}
            onChange={(event) => change({ style: event.target.value as ValidationForm['style'] })}
          >
            {STYLES.map(([value, text]) => (
              <option key={value} value={value}>
                {text}
              </option>
            ))}
          </select>
        </label>
        <label className="cgs-dialog-field cgs-dialog-grow">
          <span>Title</span>
          <input
            data-testid="validation-error-title"
            maxLength={32}
            value={form.errorTitle}
            onChange={(event) => change({ errorTitle: event.target.value })}
          />
        </label>
      </div>
      <label className="cgs-dialog-field">
        <span>Error message</span>
        <textarea
          data-testid="validation-error-text"
          rows={5}
          maxLength={225}
          value={form.errorText}
          onChange={(event) => change({ errorText: event.target.value })}
        />
      </label>
    </fieldset>
  );

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Data Validation"
      width={460}
      allowDrag
      data-testid="validation-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <CgTabs
            tabs={[
              { key: 'settings', text: 'Settings', content: settings },
              { key: 'input', text: 'Input Message', content: input },
              { key: 'alert', text: 'Error Alert', content: alert },
            ]}
            activeKey={tab}
            onActiveKeyChange={(key) => key && setTab(key)}
            size="small"
            contentMode="active-only"
            ariaLabel="Data Validation"
          />
          {locked && (
            <p data-testid="validation-extended" className="cgs-dialog-message">
              This rule was made in a newer version of Excel. You can clear it, but not change it
              here.
            </p>
          )}
          {error && (
            <p role="alert" data-testid="validation-error" className="cgs-dialog-message">
              {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            data-testid="validation-clear"
            style={{ marginInlineEnd: 'auto' }}
            onClick={() => {
              setForm({ ...EMPTY_FORM });
              setError('');
            }}
          >
            Clear All
          </CgButton>
          <CgButton
            intent="primary"
            data-testid="validation-ok"
            disabled={locked || busy}
            onClick={() => void apply()}
          >
            OK
          </CgButton>
          <CgButton data-testid="validation-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}
