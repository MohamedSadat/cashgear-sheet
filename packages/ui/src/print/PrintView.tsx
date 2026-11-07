// File › Print: what prints, the paper and the scaling, with a preview of the pages as they will
// print. Print sends the same pages to the browser's or the system's dialog, and Save as PDF
// writes them as a PDF.
import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { CgButton } from '@cashgear/ui';
import type { PageSetupInfo, PageSpec } from '@cgs/engine-client';
import { MARGIN_PRESETS, marginPreset, type PageSetupTab } from './ribbon-page';
import { nextFrame, type PrintChoice, type PrintJob } from './printJob';

export interface PrintViewProps {
  open: boolean;
  /** Export › PDF opens it with Save as PDF first. */
  mode: 'print' | 'pdf';
  setup: PageSetupInfo | null;
  hasSelection: boolean;
  /** Lays the pages out again for a choice. */
  makeJob: (choice: PrintChoice) => Promise<PrintJob>;
  svgs: (job: PrintJob) => Promise<{ svgs: string[]; release: () => void }>;
  print: (job: PrintJob, svgs: string[]) => Promise<void>;
  savePdf: (job: PrintJob) => Promise<void>;
  change: (spec: PageSpec) => Promise<void>;
  openPageSetup: (tab: PageSetupTab) => void;
  /** Web: a button that lets PDFs use the fonts on this computer. */
  fontAccess: (() => void) | null;
  onClose: () => void;
}

type Scaling = 'none' | 'sheet' | 'columns' | 'rows' | 'custom';

function scalingOf(setup: PageSetupInfo | null): Scaling {
  if (!setup) return 'none';
  if (!setup.fitToPage) return setup.scale === 100 ? 'none' : 'custom';
  if (setup.fitWidth === 1 && setup.fitHeight === 1) return 'sheet';
  if (setup.fitWidth === 1 && setup.fitHeight === 0) return 'columns';
  if (setup.fitWidth === 0 && setup.fitHeight === 1) return 'rows';
  return 'custom';
}

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function PrintView(props: PrintViewProps) {
  const { open, setup } = props;
  const [choice, setChoice] = useState<PrintChoice>({ what: 'active', ignorePrintArea: false });
  const [job, setJob] = useState<PrintJob | null>(null);
  const [svgs, setSvgs] = useState<string[]>([]);
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [printing, setPrinting] = useState<string[] | null>(null);
  const release = useRef<() => void>(() => {});
  const generation = useRef(0);

  // Lays out again whenever what prints or how it is set up changes.
  useEffect(() => {
    if (!open) return;
    const mine = ++generation.current;
    setBusy('Laying out the pages…');
    setMessage(null);
    void (async () => {
      try {
        const made = await props.makeJob(choice);
        const shown = await props.svgs(made);
        if (mine !== generation.current) {
          shown.release();
          return;
        }
        release.current();
        release.current = shown.release;
        setJob(made);
        setSvgs(shown.svgs);
        setPage((at) => Math.min(at, Math.max(0, shown.svgs.length - 1)));
      } catch (error) {
        if (mine === generation.current) setMessage(errorText(error));
      } finally {
        if (mine === generation.current) setBusy(null);
      }
    })();
  }, [open, choice, setup]);

  useEffect(() => {
    if (open) return;
    release.current();
    release.current = () => {};
    setJob(null);
    setSvgs([]);
    setPage(0);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') props.onClose();
      else if (event.key === 'PageDown') setPage((at) => Math.min(at + 1, svgs.length - 1));
      else if (event.key === 'PageUp') setPage((at) => Math.max(at - 1, 0));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, svgs.length]);

  const sizes = useMemo(() => {
    const seen = new Map<string, string>();
    for (const recorded of job?.pages ?? []) {
      const key = `${recorded.width}x${recorded.height}`;
      if (!seen.has(key)) seen.set(key, 'cgs-paper-' + seen.size);
    }
    return seen;
  }, [job]);

  if (!open) return null;
  const scaling = scalingOf(setup);
  const preset = marginPreset(setup?.margins) ?? 'custom';
  const count = svgs.length;

  const run = async (label: string, work: () => Promise<void>) => {
    setBusy(label);
    setMessage(null);
    try {
      await work();
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setBusy(null);
    }
  };

  const doPrint = () =>
    run('Sending the pages to the printer…', async () => {
      if (!job || !count) throw new Error('There is nothing to print.');
      setPrinting(svgs);
      // The pages must be in the document before the dialog takes them.
      await nextFrame();
      await nextFrame();
      try {
        await props.print(job, svgs);
      } finally {
        setPrinting(null);
      }
    });

  const doPdf = () =>
    run('Writing the PDF…', async () => {
      if (!job || !count) throw new Error('There is nothing to save.');
      await props.savePdf(job);
    });

  const select = (
    label: string,
    testId: string,
    value: string,
    options: [string, string][],
    onChange: (value: string) => void,
  ) => (
    <label className="cgs-print-field">
      <span>{label}</span>
      <select
        data-testid={testId}
        value={value}
        disabled={!setup}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map(([key, text]) => (
          <option key={key} value={key}>
            {text}
          </option>
        ))}
      </select>
    </label>
  );

  const printStyles = [...sizes.entries()]
    .map(([key, name]) => {
      const [w, h] = key.split('x');
      return `@page ${name} { size: ${w}pt ${h}pt; margin: 0; }`;
    })
    .join('\n');

  return (
    <div className="cgs-print-view" data-testid="print-view" role="dialog" aria-label="Print">
      <div className="cgs-print-settings">
        <div className="cgs-print-title">
          <CgButton data-testid="print-back" onClick={props.onClose} aria-label="Back">
            ←
          </CgButton>
          <h2>{props.mode === 'pdf' ? 'Export as PDF' : 'Print'}</h2>
        </div>
        <div className="cgs-print-actions">
          {props.mode === 'print' && (
            <CgButton
              intent="primary"
              data-testid="print-go"
              disabled={!count || busy !== null}
              onClick={() => void doPrint()}
            >
              Print
            </CgButton>
          )}
          <CgButton
            intent={props.mode === 'pdf' ? 'primary' : undefined}
            data-testid="print-pdf"
            disabled={!count || busy !== null}
            onClick={() => void doPdf()}
          >
            Save as PDF
          </CgButton>
        </div>
        {select(
          'Settings',
          'print-what',
          choice.what,
          [
            ['active', 'Print Active Sheets'],
            ['workbook', 'Print Entire Workbook'],
            ...(props.hasSelection
              ? ([['selection', 'Print Selection']] as [string, string][])
              : []),
          ],
          (what) => setChoice({ ...choice, what: what as PrintChoice['what'] }),
        )}
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="print-ignore-area"
            checked={choice.ignorePrintArea}
            onChange={(event) => setChoice({ ...choice, ignorePrintArea: event.target.checked })}
          />
          Ignore Print Area
        </label>
        {select(
          'Orientation',
          'print-orientation',
          setup?.orientation === 'landscape' ? 'landscape' : 'portrait',
          [
            ['portrait', 'Portrait Orientation'],
            ['landscape', 'Landscape Orientation'],
          ],
          (orientation) =>
            void props.change({ orientation: orientation as 'portrait' | 'landscape' }),
        )}
        {select(
          'Paper',
          'print-paper',
          String(setup?.paper ?? 1),
          (setup?.papers ?? []).map((paper) => [String(paper.code), paper.name]),
          (paper) => void props.change({ paper: Number(paper) }),
        )}
        {select(
          'Margins',
          'print-margins',
          preset,
          [
            ...MARGIN_PRESETS.map((entry) => [entry.name, entry.text] as [string, string]),
            ['custom', 'Custom Margins…'],
          ],
          (name) => {
            const entry = MARGIN_PRESETS.find((p) => p.name === name);
            if (entry) void props.change({ margins: entry.margins });
            else props.openPageSetup('margins');
          },
        )}
        {select(
          'Scaling',
          'print-scaling',
          scaling,
          [
            ['none', 'No Scaling'],
            ['sheet', 'Fit Sheet on One Page'],
            ['columns', 'Fit All Columns on One Page'],
            ['rows', 'Fit All Rows on One Page'],
            ['custom', 'Custom Scaling Options…'],
          ],
          (value) => {
            if (value === 'none') void props.change({ fitToPage: false, scale: 100 });
            else if (value === 'sheet')
              void props.change({ fitToPage: true, fitWidth: 1, fitHeight: 1 });
            else if (value === 'columns')
              void props.change({ fitToPage: true, fitWidth: 1, fitHeight: 0 });
            else if (value === 'rows')
              void props.change({ fitToPage: true, fitWidth: 0, fitHeight: 1 });
            else props.openPageSetup('page');
          },
        )}
        <CgButton data-testid="print-page-setup" onClick={() => props.openPageSetup('page')}>
          Page Setup
        </CgButton>
        {props.fontAccess && (
          <CgButton data-testid="print-font-access" onClick={props.fontAccess}>
            Use this computer’s fonts in PDFs
          </CgButton>
        )}
        {busy && (
          <p className="cgs-print-status" role="status" data-testid="print-status">
            {busy}
          </p>
        )}
        {message && (
          <p className="cgs-print-status" role="alert" data-testid="print-message">
            {message}
          </p>
        )}
      </div>
      <div className="cgs-print-preview">
        <div className="cgs-print-sheet" data-testid="print-preview">
          {count ? (
            <div
              className="cgs-print-paper"
              data-testid="print-page"
              data-page={page + 1}
              // The page is the app's own SVG, made from the recording.
              dangerouslySetInnerHTML={{ __html: svgs[page] ?? '' }}
            />
          ) : (
            !busy && <p data-testid="print-empty">There’s nothing to print.</p>
          )}
        </div>
        <div className="cgs-print-pager">
          <CgButton
            data-testid="print-prev"
            disabled={page <= 0}
            onClick={() => setPage(page - 1)}
            aria-label="Previous page"
          >
            ◀
          </CgButton>
          <span data-testid="print-count">
            {count ? page + 1 : 0} of {count}
          </span>
          <CgButton
            data-testid="print-next"
            disabled={page >= count - 1}
            onClick={() => setPage(page + 1)}
            aria-label="Next page"
          >
            ▶
          </CgButton>
        </div>
      </div>
      {printing &&
        createPortal(
          <div className="cgs-print-pages" data-testid="print-pages">
            <style>{printStyles}</style>
            {printing.map((svg, index) => {
              const recorded = job?.pages[index];
              const name = recorded ? sizes.get(`${recorded.width}x${recorded.height}`) : undefined;
              return (
                <div
                  key={index}
                  className="cgs-print-sheet-page"
                  style={{
                    page: name,
                    width: (recorded?.width ?? 612) + 'pt',
                    height: (recorded?.height ?? 792) + 'pt',
                  }}
                  dangerouslySetInnerHTML={{ __html: svg }}
                />
              );
            })}
          </div>,
          document.body,
        )}
    </div>
  );
}
