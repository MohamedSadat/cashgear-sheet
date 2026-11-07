// Excel's Page Setup dialog: Page, Margins, Header/Footer and Sheet. OK sends what changed as
// engine commands; Print and Print Preview open File › Print with the changes made.
import { useEffect, useState } from 'react';
import { CgButton, CgTabs, CgWindow } from '@cashgear/ui';
import type { CellArea, PageSetupInfo, PageSpec, HeaderFooterTexts } from '@cgs/engine-client';
import type { PageSetupTab } from './ribbon-page';

export interface PageSetupChange {
  spec: PageSpec;
  /** The print area's areas, or empty to clear it; undefined when it didn't change. */
  printArea?: CellArea[];
  /** Rows and columns to repeat; undefined when they didn't change. */
  titles?: { rows: [number, number] | null; cols: [number, number] | null };
}

export interface PageSetupDialogProps {
  open: boolean;
  tab: PageSetupTab;
  setup: PageSetupInfo | null;
  sheetName: string;
  fileName: string;
  onClose: () => void;
  /** Applies the change, then opens File › Print when asked. */
  onApply: (change: PageSetupChange, then: 'close' | 'print' | 'preview') => Promise<void>;
}

const colNumber = (letters: string) =>
  [...letters.toUpperCase()].reduce((n, ch) => n * 26 + ch.charCodeAt(0) - 64, 0);

/** `$A$1:$D$20,$F$3` as areas, 1-based. Null if any piece can't be read. */
export function readAreas(text: string): CellArea[] | null {
  const pieces = text
    .split(',')
    .map((piece) => piece.trim().replace(/^.*!/, ''))
    .filter(Boolean);
  const areas: CellArea[] = [];
  for (const piece of pieces) {
    const match = /^\$?([A-Z]{1,3})\$?(\d+)(?::\$?([A-Z]{1,3})\$?(\d+))?$/i.exec(piece);
    if (!match) return null;
    const left = colNumber(match[1]!);
    const top = Number(match[2]);
    const right = match[3] ? colNumber(match[3]) : left;
    const bottom = match[4] ? Number(match[4]) : top;
    areas.push({
      top: Math.min(top, bottom),
      left: Math.min(left, right),
      bottom: Math.max(top, bottom),
      right: Math.max(left, right),
    });
  }
  return areas;
}

/** `$1:$3` as rows, or `$A:$B` as columns. Null if empty, undefined if it can't be read. */
export function readLines(text: string, rows: boolean): [number, number] | null | undefined {
  const clean = text.trim().replace(/^.*!/, '');
  if (!clean) return null;
  const pattern = rows ? /^\$?(\d+)(?::\$?(\d+))?$/ : /^\$?([A-Z]{1,3})(?::\$?([A-Z]{1,3}))?$/i;
  const match = pattern.exec(clean);
  if (!match) return undefined;
  const first = rows ? Number(match[1]) : colNumber(match[1]!);
  const last = match[2] ? (rows ? Number(match[2]) : colNumber(match[2])) : first;
  return [Math.min(first, last), Math.max(first, last)];
}

/** Excel's ready-made headers and footers. */
export function headerPresets(sheet: string, file: string): { text: string; code: string }[] {
  const book = file.replace(/\.[^.]*$/, '');
  return [
    { text: '(none)', code: '' },
    { text: 'Page 1', code: '&CPage &P' },
    { text: 'Page 1 of ?', code: '&CPage &P of &N' },
    { text: sheet, code: '&C&A' },
    {
      text: `Confidential, ${new Date().toLocaleDateString()}, Page 1`,
      code: '&LConfidential&C&D&RPage &P',
    },
    { text: book, code: '&C&F' },
    { text: `${sheet}, Page 1`, code: '&C&A&RPage &P' },
    { text: `${book}, Page 1`, code: '&C&F&RPage &P' },
    { text: `Page 1, ${sheet}`, code: '&CPage &P&R&A' },
  ];
}

/** Splits codes into the left, centre and right boxes of Custom Header. */
export function splitSections(text: string): [string, string, string] {
  const parts: [string, string, string] = ['', '', ''];
  let section = 1;
  for (let at = 0; at < text.length; at++) {
    const ch = text[at]!;
    if (ch === '&' && at + 1 < text.length) {
      const code = text[at + 1]!;
      if (code === 'L' || code === 'C' || code === 'R') {
        section = code === 'L' ? 0 : code === 'C' ? 1 : 2;
        at++;
        continue;
      }
      parts[section] += ch + code;
      at++;
      continue;
    }
    parts[section] += ch;
  }
  return parts;
}

export function joinSections([left, center, right]: [string, string, string]): string {
  return (left ? '&L' + left : '') + (center ? '&C' + center : '') + (right ? '&R' + right : '');
}

/** Plain text for a header in the dialog's preview. */
export function previewText(code: string, sheet: string, file: string): string {
  return code
    .replace(/&"[^"]*"/g, '')
    .replace(/&\d+/g, '')
    .replace(/&K[0-9A-F]{6}/gi, '')
    .replace(/&[LCRBIUESXY]/g, ' ')
    .replace(/&P([+-]\d+)?/g, '1')
    .replace(/&N/g, '1')
    .replace(/&D/g, new Date().toLocaleDateString())
    .replace(/&T/g, new Date().toLocaleTimeString())
    .replace(/&A/g, sheet)
    .replace(/&[FZ]/g, file)
    .replace(/&G/g, '[Picture]')
    .replace(/&&/g, '&')
    .replace(/\s+/g, ' ')
    .trim();
}

type HfKey = keyof Pick<
  HeaderFooterTexts,
  'oddHeader' | 'oddFooter' | 'evenHeader' | 'evenFooter' | 'firstHeader' | 'firstFooter'
>;

const FIELDS: [string, string][] = [
  ['Page Number', '&P'],
  ['Number of Pages', '&N'],
  ['Date', '&D'],
  ['Time', '&T'],
  ['File Path', '&Z&F'],
  ['File Name', '&F'],
  ['Sheet Name', '&A'],
];

function CustomHeader(props: {
  open: boolean;
  footer: boolean;
  texts: Record<HfKey, string>;
  oddEven: boolean;
  first: boolean;
  onClose: () => void;
  onOk: (texts: Partial<Record<HfKey, string>>) => void;
}) {
  const kinds: { key: HfKey; text: string }[] = [
    { key: props.footer ? 'oddFooter' : 'oddHeader', text: props.footer ? 'Footer' : 'Header' },
    ...(props.oddEven
      ? [
          {
            key: (props.footer ? 'evenFooter' : 'evenHeader') as HfKey,
            text: props.footer ? 'Even Page Footer' : 'Even Page Header',
          },
        ]
      : []),
    ...(props.first
      ? [
          {
            key: (props.footer ? 'firstFooter' : 'firstHeader') as HfKey,
            text: props.footer ? 'First Page Footer' : 'First Page Header',
          },
        ]
      : []),
  ];
  const [boxes, setBoxes] = useState<Record<string, [string, string, string]>>({});
  const [tab, setTab] = useState<string>(kinds[0]!.key);
  const [focus, setFocus] = useState(1);
  useEffect(() => {
    if (!props.open) return;
    setBoxes(
      Object.fromEntries(kinds.map((kind) => [kind.key, splitSections(props.texts[kind.key])])),
    );
    setTab(kinds[0]!.key);
    // Each time it opens.
  }, [props.open]);
  const insert = (code: string) => {
    const current = boxes[tab] ?? ['', '', ''];
    const next = [...current] as [string, string, string];
    next[focus] += code;
    setBoxes({ ...boxes, [tab]: next });
  };
  const section = (kind: HfKey) => (
    <div className="cgs-dialog-column">
      <div className="cgs-dialog-row" role="group" aria-label="Insert">
        {FIELDS.map(([text, code]) => (
          <CgButton key={code} data-testid={`hf-insert-${code}`} onClick={() => insert(code)}>
            {text}
          </CgButton>
        ))}
      </div>
      <div className="cgs-dialog-row">
        {(['Left section', 'Center section', 'Right section'] as const).map((label, index) => (
          <label key={label} className="cgs-dialog-field cgs-dialog-grow">
            {label}:
            <textarea
              rows={4}
              data-testid={`hf-section-${index}`}
              value={(boxes[kind] ?? ['', '', ''])[index]}
              onFocus={() => setFocus(index)}
              onChange={(event) => {
                const next = [...(boxes[kind] ?? ['', '', ''])] as [string, string, string];
                next[index] = event.target.value;
                setBoxes({ ...boxes, [kind]: next });
              }}
            />
          </label>
        ))}
      </div>
    </div>
  );
  return (
    <CgWindow
      open={props.open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText={props.footer ? 'Footer' : 'Header'}
      width={620}
      allowDrag
      data-testid="custom-header"
      renderBody={() =>
        kinds.length > 1 ? (
          <CgTabs
            tabs={kinds.map((kind) => ({
              key: kind.key,
              text: kind.text,
              content: section(kind.key),
            }))}
            activeKey={tab}
            onActiveKeyChange={(key) => key && setTab(key)}
            size="small"
            contentMode="active-only"
            ariaLabel="Header"
          />
        ) : (
          section(kinds[0]!.key)
        )
      }
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            intent="primary"
            data-testid="custom-header-ok"
            onClick={() =>
              props.onOk(
                Object.fromEntries(
                  Object.entries(boxes).map(([key, value]) => [key, joinSections(value)]),
                ),
              )
            }
          >
            OK
          </CgButton>
          <CgButton onClick={props.onClose}>Cancel</CgButton>
        </div>
      )}
    />
  );
}

interface Draft {
  orientation: 'portrait' | 'landscape';
  fit: boolean;
  scale: string;
  fitWidth: string;
  fitHeight: string;
  paper: number;
  firstPage: string;
  margins: Record<'top' | 'bottom' | 'left' | 'right' | 'header' | 'footer', string>;
  centerH: boolean;
  centerV: boolean;
  hf: Record<HfKey, string>;
  oddEven: boolean;
  first: boolean;
  scaleWithDoc: boolean;
  align: boolean;
  printArea: string;
  titleRows: string;
  titleCols: string;
  gridlines: boolean;
  blackAndWhite: boolean;
  draft: boolean;
  headings: boolean;
  comments: 'none' | 'atEnd' | 'asDisplayed';
  errors: 'displayed' | 'blank' | 'dash' | 'NA';
  pageOrder: 'downThenOver' | 'overThenDown';
}

function draftOf(setup: PageSetupInfo): Draft {
  const m = setup.margins;
  const show = (value: number) => String(Math.round(value * 100) / 100);
  return {
    orientation: setup.orientation === 'landscape' ? 'landscape' : 'portrait',
    fit: setup.fitToPage,
    scale: String(setup.scale),
    fitWidth: setup.fitWidth ? String(setup.fitWidth) : '',
    fitHeight: setup.fitHeight ? String(setup.fitHeight) : '',
    paper: setup.paper,
    firstPage: setup.firstPage === null ? 'Auto' : String(setup.firstPage),
    margins: {
      top: show(m.top),
      bottom: show(m.bottom),
      left: show(m.left),
      right: show(m.right),
      header: show(m.header),
      footer: show(m.footer),
    },
    centerH: setup.centerHorizontally,
    centerV: setup.centerVertically,
    hf: {
      oddHeader: setup.headerFooter.oddHeader,
      oddFooter: setup.headerFooter.oddFooter,
      evenHeader: setup.headerFooter.evenHeader,
      evenFooter: setup.headerFooter.evenFooter,
      firstHeader: setup.headerFooter.firstHeader,
      firstFooter: setup.headerFooter.firstFooter,
    },
    oddEven: setup.headerFooter.differentOddEven,
    first: setup.headerFooter.differentFirst,
    scaleWithDoc: setup.headerFooter.scaleWithDoc,
    align: setup.headerFooter.alignWithMargins,
    printArea: setup.printArea,
    titleRows: setup.titleRows ?? '',
    titleCols: setup.titleCols ?? '',
    gridlines: setup.gridlines,
    blackAndWhite: setup.blackAndWhite,
    draft: setup.draft,
    headings: setup.headings,
    comments: setup.comments,
    errors: setup.errors,
    pageOrder: setup.pageOrder,
  };
}

/** What the draft changes, refused in words where Excel would refuse it. */
export function changeOf(setup: PageSetupInfo, draft: Draft): PageSetupChange | string {
  const spec: PageSpec = {};
  if (draft.orientation !== (setup.orientation === 'landscape' ? 'landscape' : 'portrait')) {
    spec.orientation = draft.orientation;
  }
  if (draft.paper !== setup.paper) spec.paper = draft.paper;
  if (draft.fit) {
    const wide = draft.fitWidth.trim() ? Number(draft.fitWidth) : 0;
    const tall = draft.fitHeight.trim() ? Number(draft.fitHeight) : 0;
    if (![wide, tall].every((n) => Number.isInteger(n) && n >= 0 && n <= 32767)) {
      return 'Fit to a whole number of pages.';
    }
    if (!setup.fitToPage || wide !== setup.fitWidth || tall !== setup.fitHeight) {
      Object.assign(spec, { fitToPage: true, fitWidth: wide, fitHeight: tall });
    }
  } else {
    const scale = Number(draft.scale);
    if (!Number.isInteger(scale) || scale < 10 || scale > 400) {
      return 'The scale must be between 10% and 400%.';
    }
    if (setup.fitToPage || scale !== setup.scale) Object.assign(spec, { fitToPage: false, scale });
  }
  const first = draft.firstPage.trim();
  if (/^auto$/i.test(first) || !first) {
    if (setup.firstPage !== null) spec.firstPageAuto = true;
  } else {
    const number = Number(first);
    if (!Number.isInteger(number)) return 'The first page number must be a whole number.';
    if (number !== setup.firstPage) spec.firstPage = number;
  }
  const margins: Partial<Record<keyof Draft['margins'], number>> = {};
  for (const key of Object.keys(draft.margins) as (keyof Draft['margins'])[]) {
    const value = Number(draft.margins[key]);
    if (!Number.isFinite(value) || value < 0 || value > 49) {
      return 'Margins must be between 0 and 49 inches.';
    }
    if (Math.abs(value - setup.margins[key]) > 1e-9) margins[key] = value;
  }
  if (Object.keys(margins).length) spec.margins = margins;
  if (draft.centerH !== setup.centerHorizontally) spec.centerHorizontally = draft.centerH;
  if (draft.centerV !== setup.centerVertically) spec.centerVertically = draft.centerV;
  const hf: Partial<HeaderFooterTexts> = {};
  for (const key of Object.keys(draft.hf) as HfKey[]) {
    if (draft.hf[key] !== setup.headerFooter[key]) hf[key] = draft.hf[key];
  }
  if (draft.oddEven !== setup.headerFooter.differentOddEven) hf.differentOddEven = draft.oddEven;
  if (draft.first !== setup.headerFooter.differentFirst) hf.differentFirst = draft.first;
  if (draft.scaleWithDoc !== setup.headerFooter.scaleWithDoc) hf.scaleWithDoc = draft.scaleWithDoc;
  if (draft.align !== setup.headerFooter.alignWithMargins) hf.alignWithMargins = draft.align;
  if (Object.keys(hf).length) spec.headerFooter = hf;
  if (draft.gridlines !== setup.gridlines) spec.gridlines = draft.gridlines;
  if (draft.blackAndWhite !== setup.blackAndWhite) spec.blackAndWhite = draft.blackAndWhite;
  if (draft.draft !== setup.draft) spec.draft = draft.draft;
  if (draft.headings !== setup.headings) spec.headings = draft.headings;
  if (draft.comments !== setup.comments) spec.comments = draft.comments;
  if (draft.errors !== setup.errors) spec.errors = draft.errors;
  if (draft.pageOrder !== setup.pageOrder) spec.pageOrder = draft.pageOrder;
  const change: PageSetupChange = { spec };
  if (draft.printArea.trim() !== setup.printArea) {
    const areas = readAreas(draft.printArea);
    if (!areas) return 'The print area isn’t a reference this app can read.';
    change.printArea = areas;
  }
  if (
    draft.titleRows.trim() !== (setup.titleRows ?? '') ||
    draft.titleCols.trim() !== (setup.titleCols ?? '')
  ) {
    const rows = readLines(draft.titleRows, true);
    const cols = readLines(draft.titleCols, false);
    if (rows === undefined) return 'Rows to repeat at top must be whole rows, such as $1:$2.';
    if (cols === undefined)
      return 'Columns to repeat at left must be whole columns, such as $A:$B.';
    change.titles = { rows, cols };
  }
  return change;
}

export function PageSetupDialog(props: PageSetupDialogProps) {
  const { open, setup } = props;
  const [tab, setTab] = useState<PageSetupTab>(props.tab);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState('');
  const [custom, setCustom] = useState<'header' | 'footer' | null>(null);
  useEffect(() => {
    if (!open || !setup) return;
    setDraft(draftOf(setup));
    setTab(props.tab);
    setError('');
    // Each time it opens.
  }, [open]);
  if (!setup || !draft) return null;
  const set = (patch: Partial<Draft>) => setDraft({ ...draft, ...patch });
  const presets = headerPresets(props.sheetName, props.fileName);
  const apply = async (then: 'close' | 'print' | 'preview') => {
    const change = changeOf(setup, draft);
    if (typeof change === 'string') {
      setError(change);
      return;
    }
    try {
      await props.onApply(change, then);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };
  const check = (label: string, testId: string, on: boolean, change: (on: boolean) => void) => (
    <label className="cgs-dialog-check">
      <input
        type="checkbox"
        data-testid={testId}
        checked={on}
        onChange={(event) => change(event.target.checked)}
      />
      {label}
    </label>
  );
  const radio = (name: string, label: string, testId: string, on: boolean, choose: () => void) => (
    <label className="cgs-dialog-check">
      <input type="radio" name={name} data-testid={testId} checked={on} onChange={choose} />
      {label}
    </label>
  );
  const margin = (key: keyof Draft['margins'], label: string) => (
    <label className="cgs-dialog-field">
      {label}
      <input
        type="number"
        step={0.1}
        min={0}
        data-testid={`margin-${key}`}
        value={draft.margins[key]}
        onChange={(event) => set({ margins: { ...draft.margins, [key]: event.target.value } })}
      />
    </label>
  );
  const pageTab = (
    <div className="cgs-dialog-column">
      <fieldset className="cgs-dialog-column">
        <legend>Orientation</legend>
        <div className="cgs-dialog-row">
          {radio(
            'orientation',
            'Portrait',
            'setup-portrait',
            draft.orientation === 'portrait',
            () => set({ orientation: 'portrait' }),
          )}
          {radio(
            'orientation',
            'Landscape',
            'setup-landscape',
            draft.orientation === 'landscape',
            () => set({ orientation: 'landscape' }),
          )}
        </div>
      </fieldset>
      <fieldset className="cgs-dialog-column">
        <legend>Scaling</legend>
        <div className="cgs-dialog-row">
          {radio('scaling', 'Adjust to:', 'setup-adjust', !draft.fit, () => set({ fit: false }))}
          <input
            type="number"
            min={10}
            max={400}
            aria-label="% normal size"
            data-testid="setup-scale"
            disabled={draft.fit}
            value={draft.scale}
            onChange={(event) => set({ scale: event.target.value })}
          />
          <span>% normal size</span>
        </div>
        <div className="cgs-dialog-row">
          {radio('scaling', 'Fit to:', 'setup-fit', draft.fit, () => set({ fit: true }))}
          <input
            type="number"
            min={0}
            aria-label="pages wide"
            data-testid="setup-fit-width"
            disabled={!draft.fit}
            value={draft.fitWidth}
            onChange={(event) => set({ fitWidth: event.target.value })}
          />
          <span>page(s) wide by</span>
          <input
            type="number"
            min={0}
            aria-label="tall"
            data-testid="setup-fit-height"
            disabled={!draft.fit}
            value={draft.fitHeight}
            onChange={(event) => set({ fitHeight: event.target.value })}
          />
          <span>tall</span>
        </div>
      </fieldset>
      <label className="cgs-dialog-field">
        Paper size:
        <select
          data-testid="setup-paper"
          value={draft.paper}
          onChange={(event) => set({ paper: Number(event.target.value) })}
        >
          {setup.papers.some((p) => p.code === draft.paper) ? null : (
            <option value={draft.paper}>Paper {draft.paper}</option>
          )}
          {setup.papers.map((paper) => (
            <option key={paper.code} value={paper.code}>
              {paper.name}
            </option>
          ))}
        </select>
      </label>
      <label className="cgs-dialog-field">
        First page number:
        <input
          data-testid="setup-first-page"
          value={draft.firstPage}
          onChange={(event) => set({ firstPage: event.target.value })}
        />
      </label>
    </div>
  );
  const marginsTab = (
    <div className="cgs-dialog-column">
      <div className="cgs-dialog-row">
        {margin('top', 'Top:')}
        {margin('header', 'Header:')}
      </div>
      <div className="cgs-dialog-row">
        {margin('left', 'Left:')}
        {margin('right', 'Right:')}
      </div>
      <div className="cgs-dialog-row">
        {margin('bottom', 'Bottom:')}
        {margin('footer', 'Footer:')}
      </div>
      <fieldset className="cgs-dialog-row">
        <legend>Center on page</legend>
        {check('Horizontally', 'setup-center-h', draft.centerH, (on) => set({ centerH: on }))}
        {check('Vertically', 'setup-center-v', draft.centerV, (on) => set({ centerV: on }))}
      </fieldset>
    </div>
  );
  const presetSelect = (key: 'oddHeader' | 'oddFooter', label: string) => {
    const current = draft.hf[key];
    const known = presets.some((preset) => preset.code === current);
    return (
      <label className="cgs-dialog-field">
        {label}
        <select
          data-testid={`setup-${key}`}
          value={known ? current : '__custom'}
          onChange={(event) => {
            if (event.target.value === '__custom') return;
            set({ hf: { ...draft.hf, [key]: event.target.value } });
          }}
        >
          {!known && (
            <option value="__custom">
              {previewText(current, props.sheetName, props.fileName)}
            </option>
          )}
          {presets.map((preset) => (
            <option key={preset.text} value={preset.code}>
              {preset.text}
            </option>
          ))}
        </select>
      </label>
    );
  };
  const headerTab = (
    <div className="cgs-dialog-column">
      <div className="cgs-dialog-preview" data-testid="header-preview">
        {previewText(draft.hf.oddHeader, props.sheetName, props.fileName) || ' '}
      </div>
      {presetSelect('oddHeader', 'Header:')}
      <div className="cgs-dialog-row">
        <CgButton data-testid="setup-custom-header" onClick={() => setCustom('header')}>
          Custom Header…
        </CgButton>
        <CgButton data-testid="setup-custom-footer" onClick={() => setCustom('footer')}>
          Custom Footer…
        </CgButton>
      </div>
      {presetSelect('oddFooter', 'Footer:')}
      <div className="cgs-dialog-preview" data-testid="footer-preview">
        {previewText(draft.hf.oddFooter, props.sheetName, props.fileName) || ' '}
      </div>
      {check('Different odd and even pages', 'setup-odd-even', draft.oddEven, (on) =>
        set({ oddEven: on }),
      )}
      {check('Different first page', 'setup-first', draft.first, (on) => set({ first: on }))}
      {check('Scale with document', 'setup-scale-doc', draft.scaleWithDoc, (on) =>
        set({ scaleWithDoc: on }),
      )}
      {check('Align with page margins', 'setup-align', draft.align, (on) => set({ align: on }))}
    </div>
  );
  const sheetTab = (
    <div className="cgs-dialog-column">
      <label className="cgs-dialog-field">
        Print area:
        <input
          data-testid="setup-print-area"
          value={draft.printArea}
          onChange={(event) => set({ printArea: event.target.value })}
        />
      </label>
      <fieldset className="cgs-dialog-column">
        <legend>Print titles</legend>
        <label className="cgs-dialog-field">
          Rows to repeat at top:
          <input
            data-testid="setup-title-rows"
            value={draft.titleRows}
            placeholder="$1:$1"
            onChange={(event) => set({ titleRows: event.target.value })}
          />
        </label>
        <label className="cgs-dialog-field">
          Columns to repeat at left:
          <input
            data-testid="setup-title-cols"
            value={draft.titleCols}
            placeholder="$A:$A"
            onChange={(event) => set({ titleCols: event.target.value })}
          />
        </label>
      </fieldset>
      <fieldset className="cgs-dialog-column">
        <legend>Print</legend>
        <div className="cgs-dialog-row">
          {check('Gridlines', 'setup-gridlines', draft.gridlines, (on) => set({ gridlines: on }))}
          {check('Black and white', 'setup-bw', draft.blackAndWhite, (on) =>
            set({ blackAndWhite: on }),
          )}
        </div>
        <div className="cgs-dialog-row">
          {check('Draft quality', 'setup-draft', draft.draft, (on) => set({ draft: on }))}
          {check('Row and column headings', 'setup-headings', draft.headings, (on) =>
            set({ headings: on }),
          )}
        </div>
        <label className="cgs-dialog-field">
          Comments:
          <select
            data-testid="setup-comments"
            value={draft.comments}
            onChange={(event) => set({ comments: event.target.value as Draft['comments'] })}
          >
            <option value="none">(None)</option>
            <option value="atEnd">At end of sheet</option>
            <option value="asDisplayed">As displayed on sheet</option>
          </select>
        </label>
        <label className="cgs-dialog-field">
          Cell errors as:
          <select
            data-testid="setup-errors"
            value={draft.errors}
            onChange={(event) => set({ errors: event.target.value as Draft['errors'] })}
          >
            <option value="displayed">displayed</option>
            <option value="blank">&lt;blank&gt;</option>
            <option value="dash">--</option>
            <option value="NA">#N/A</option>
          </select>
        </label>
      </fieldset>
      <fieldset className="cgs-dialog-row">
        <legend>Page order</legend>
        {radio('order', 'Down, then over', 'setup-down', draft.pageOrder === 'downThenOver', () =>
          set({ pageOrder: 'downThenOver' }),
        )}
        {radio('order', 'Over, then down', 'setup-over', draft.pageOrder === 'overThenDown', () =>
          set({ pageOrder: 'overThenDown' }),
        )}
      </fieldset>
    </div>
  );
  return (
    <>
      <CgWindow
        open={open}
        onOpenChange={(next) => {
          if (!next) props.onClose();
        }}
        headerText="Page Setup"
        width={560}
        allowDrag
        data-testid="page-setup"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <CgTabs
              tabs={[
                { key: 'page', text: 'Page', content: pageTab },
                { key: 'margins', text: 'Margins', content: marginsTab },
                { key: 'header', text: 'Header/Footer', content: headerTab },
                { key: 'sheet', text: 'Sheet', content: sheetTab },
              ]}
              activeKey={tab}
              onActiveKeyChange={(key) => key && setTab(key as PageSetupTab)}
              size="small"
              contentMode="active-only"
              ariaLabel="Page Setup"
            />
            {error && (
              <p role="alert" data-testid="setup-error" className="cgs-dialog-message">
                {error}
              </p>
            )}
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton data-testid="setup-print" onClick={() => void apply('print')}>
              Print…
            </CgButton>
            <CgButton data-testid="setup-preview" onClick={() => void apply('preview')}>
              Print Preview
            </CgButton>
            <CgButton intent="primary" data-testid="setup-ok" onClick={() => void apply('close')}>
              OK
            </CgButton>
            <CgButton data-testid="setup-cancel" onClick={props.onClose}>
              Cancel
            </CgButton>
          </div>
        )}
      />
      <CustomHeader
        open={custom !== null}
        footer={custom === 'footer'}
        texts={draft.hf}
        oddEven={draft.oddEven}
        first={draft.first}
        onClose={() => setCustom(null)}
        onOk={(texts) => {
          set({ hf: { ...draft.hf, ...texts } });
          setCustom(null);
        }}
      />
    </>
  );
}
