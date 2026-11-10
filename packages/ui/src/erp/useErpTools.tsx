// ERP data in the shell (Phases 15 to 17): opening a launch from Cash Gear ERP with its load
// panel, the ERP Data tab with Refresh, Validate, Review Changes, Commit, Discard Changes, Next
// Error and Connection Info, the status badge with what changed, hover details and the window's
// title. The connector fetches and sends, the engine reads, binds, tracks changes and applies what
// the ERP answers, and this only shows what they report: it never reads or changes cells itself.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { CgButton, CgProgressBar, CgStatusBadge, CgWindow, type CgRibbonTab } from '@cashgear/ui';
import type {
  Applied,
  CellArea,
  Command,
  EngineClient,
  ErpBinding,
  ErpChangeset,
  ErpMarkSummary,
  ErpReview,
  ErpReviewEntry,
} from '@cgs/engine-client';
import {
  commitRecords,
  ErpError,
  ErpHttp,
  newCommitId,
  openRecords,
  payloadKey,
  refreshRecords,
  sendCommit,
  type CommitRequest,
  type ErpLaunch,
  type ErpSession,
  type RecordChange,
  type RecordSink,
  type SessionInfo,
} from '@cgs/erp-connector';
import {
  badgeText,
  batchErrorText,
  BUSY_FAILED,
  cellTip,
  commitButton,
  commitFailureText,
  committedText,
  committingText,
  COMMIT_TITLE,
  confirmLines,
  conflictsWaitText,
  conflictText409,
  CONNECTING,
  count,
  failureText,
  hasChanges,
  HIDDEN_ROW,
  loadingText,
  localErrorsText,
  NO_ERRORS,
  NOTHING_TO_COMMIT,
  NOTHING_TO_DISCARD,
  NOTHING_TO_VALIDATE,
  NOTHING_VISIBLE,
  openingTitle,
  REFRESH_ANYWAY,
  refreshedText,
  refreshReplaces,
  refusedText,
  SERVER_FAILED,
  tooManyText,
  UNKNOWN_OUTCOME,
  validatedText,
  validatingText,
  workbookTitle,
} from './messages';
import { ReviewChangesDialog } from './ReviewChangesDialog';
import { LOCK_PX } from '@cgs/engine-client';

export interface ErpToolsHost {
  engine: () => EngineClient | null;
  /** Runs a job in the shell's command queue, after whatever is queued. */
  queue: <T>(job: (engine: EngineClient) => Promise<T>) => Promise<T>;
  /** Opens a new blank workbook named `name`, resolving once its sheet can be written. */
  openBlank: (name: string, rightToLeft: boolean) => Promise<void>;
  /** The binding changed the workbook: its tabs, its sheet and its undo list. */
  afterBind: () => Promise<void>;
  notice: (message: string | null) => void;
  /** The sheet showing. */
  sheet: number;
  /** Runs a command as the ribbon's commands run, so undo and the grid follow it. */
  run: (command: Command) => Promise<Applied>;
  /** The selected areas and the active cell on the sheet showing, 1-based. */
  selection: () => { areas: CellArea[]; row: number; col: number } | null;
  /** Selects one cell on the sheet showing, 1-based. */
  select: (row: number, col: number) => void;
}

type Panel =
  | { phase: 'connecting'; noun: string | null }
  | { phase: 'loading'; noun: string; loaded: number; total: number | null }
  | { phase: 'binding'; noun: string; total: number }
  | { phase: 'failed'; noun: string | null; text: string; retry: boolean };

/** A commit on its way: what was sent, and how many changes and records it carries. */
interface InFlight {
  request: CommitRequest;
  changes: number;
  records: number;
}

/** The commit's panel: while it runs, when its outcome isn't known, and when it was refused. */
type CommitPanel =
  | { phase: 'committing'; text: string }
  | { phase: 'unknown'; text: string }
  | { phase: 'refused'; text: string; nextError: boolean };

/** Changes in a changeset, and those in rows a filter hides. */
function tally(set: ErpChangeset) {
  let changes = 0;
  let hidden = 0;
  for (const record of set.records) {
    const n = Object.keys(record.changes).length;
    changes += n;
    if (record.hidden) hidden += n;
  }
  return { changes, records: set.records.length, hidden, formulas: set.formulas.length };
}

/** A batch error's words, for errors no cell can show. */
const batchTexts = (summary: ErpMarkSummary, noun: string) => [
  ...new Set(summary.unplaced.map((error) => batchErrorText(error.code, error.message, noun))),
];

export interface ErpTools {
  binding: ErpBinding | null;
  /** The ERP Data tab, shown while a bound sheet is. */
  tabs: CgRibbonTab[];
  /** The status bar's badge, while a workbook is bound. */
  badge: ReactNode | null;
  overlays: ReactNode;
  /** Opens what the ERP's button launched. */
  launch: (launch: ErpLaunch) => void;
  /** Reads the binding again after a command, which may have moved its table. */
  sync: () => void;
  /**
   * Whether a cell is a bound table's heading or a read-only column's, where typing is refused at
   * the first key. Editable columns take typing.
   */
  readOnlyAt: (sheet: number, row: number, col: number) => boolean;
  /** What hovering over a bound cell shows: its field, what changed and why it's refused. */
  hoverDetails: (
    sheet: number,
    row: number,
    col: number,
  ) => Promise<{ title: string; text: string } | null>;
  /** For the bind bench: binds records already staged in the engine, as opening does. */
  bindStaged: (session: SessionInfo, label: string) => Promise<ErpBinding>;
}

function sinkOf(engine: EngineClient): RecordSink {
  return {
    begin: (schema) => engine.erpBegin(schema),
    beginRefresh: () => engine.erpBeginRefresh(),
    page: (bytes) => engine.erpPage(bytes),
    cancel: () => engine.erpCancel(),
  };
}

const asErpError = (error: unknown) =>
  error instanceof ErpError
    ? error
    : new ErpError('contract', error instanceof Error ? error.message : String(error));

const nounOf = (entity: string) => entity.toLowerCase();

/**
 * Room a heading's text leaves at 100%: a filter button's width, as the grid draws it
 * (`GridController.buttonBox`), and a read-only column's lock.
 */
const HEADING_ROOM_PX = 17 + LOCK_PX;

/** The longest texts by characters measured in each column: enough for its widest, and quick. */
const FIT_SAMPLE = 16;

export function useErpTools(host: ErpToolsHost): ErpTools {
  const hostRef = useRef(host);
  hostRef.current = host;
  const [binding, setBinding] = useState<ErpBinding | null>(null);
  const bindingRef = useRef<ErpBinding | null>(null);
  bindingRef.current = binding;
  const [panel, setPanel] = useState<Panel | null>(null);
  const [expired, setExpired] = useState(false);
  const [busy, setBusy] = useState(false);
  const [infoOpen, setInfoOpen] = useState(false);
  /** Refresh waiting on the user's word, with how many changes it would replace. */
  const [refreshAsk, setRefreshAsk] = useState<number | null>(null);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [review, setReview] = useState<ErpReview | null>(null);
  /** The commit's confirmation, with what it would send. */
  const [confirm, setConfirm] = useState<{
    visible: boolean;
    lines: string[];
    changes: number;
  } | null>(null);
  const [commitPanel, setCommitPanel] = useState<CommitPanel | null>(null);
  /** Why Commit sent nothing, in a message box. */
  const [refusal, setRefusal] = useState<{ text: string; nextError: boolean } | null>(null);
  /** A commit whose outcome isn't known, kept so the same payload goes again under its id. */
  const pendingRef = useRef<{ key: string; commitId: string } | null>(null);
  const inFlightRef = useRef<InFlight | null>(null);
  const sessionRef = useRef<ErpSession | null>(null);
  const launchRef = useRef<ErpLaunch | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const [apiBase, setApiBase] = useState('');

  /**
   * Binds what the engine has staged into a new workbook named for the data and company, fits its
   * columns and leaves nothing to undo. The last step of opening, and what the bind bench times.
   */
  const bindStaged = useCallback(
    async (session: SessionInfo, label: string): Promise<ErpBinding> => {
      const h = hostRef.current;
      // Marks for bench/measure-erp-bind.mjs, which reads where binding's time goes.
      const mark = (name: string) => performance.mark(`erp:${name}`);
      mark('begin');
      await h.openBlank(`${workbookTitle(label, session.company)}.xlsx`, session.language === 'ar');
      mark('opened');
      const bound = await h.queue((e) => e.erpBind(JSON.stringify(session)));
      mark('bound');
      // Fitted to their values and headings, with room for each heading's filter button.
      await h
        .queue((e) =>
          e.autoFitColumns(bound.sheet, bound.left, bound.right, {
            room: { row: bound.top, px: HEADING_ROOM_PX },
            keep: FIT_SAMPLE,
          }),
        )
        .catch(() => null);
      mark('fitted');
      await h.queue((e) => e.erpSettle());
      await h.afterBind();
      mark('shown');
      return bound;
    },
    [],
  );

  const open = useCallback(
    async (launch: ErpLaunch) => {
      const h = hostRef.current;
      const engine = h.engine();
      if (!engine) return;
      launchRef.current = launch;
      setApiBase(launch.apiBase);
      const controller = new AbortController();
      abortRef.current = controller;
      let noun: string | null = sessionRef.current ? nounOf(sessionRef.current.info.entity) : null;
      setPanel({ phase: 'connecting', noun });
      try {
        const opened = await openRecords({
          http: new ErpHttp({ baseUrl: launch.apiBase }),
          ticket: launch.ticket,
          session: sessionRef.current ?? undefined,
          onSession: (session) => {
            sessionRef.current = session;
            noun = nounOf(session.info.entity);
            setPanel({ phase: 'connecting', noun });
          },
          sink: sinkOf(engine),
          signal: controller.signal,
          onProgress: (progress) => {
            if (progress.stage !== 'loading') return;
            setPanel({
              phase: 'loading',
              noun: noun ?? 'records',
              loaded: progress.loaded,
              total: progress.total,
            });
          },
        });
        const { session, schema } = opened;
        noun = schema.label.en.toLowerCase();
        setPanel({ phase: 'binding', noun, total: opened.loaded });
        const bound = await bindStaged(session.info, schema.label[session.info.language]);
        setExpired(session.state === 'expired');
        session.onState((state) => setExpired(state === 'expired'));
        setBinding(bound);
        setPanel(null);
      } catch (error) {
        const failure = asErpError(error);
        if (failure.kind === 'ticket' || failure.kind === 'session') sessionRef.current = null;
        const { text, retry } = failureText(failure.kind, failure.message, noun ?? 'this data');
        setPanel({ phase: 'failed', noun, text, retry });
      } finally {
        if (abortRef.current === controller) abortRef.current = null;
      }
    },
    [bindStaged],
  );

  const launch = useCallback((given: ErpLaunch) => void open(given), [open]);

  const refresh = useCallback(async (anyway = false) => {
    const h = hostRef.current;
    const session = sessionRef.current;
    const bound = bindingRef.current;
    if (!session || !bound || session.state === 'expired') return;
    // Until Phase 18 merges them, a refresh replaces the sheet's changes, so it asks first.
    if (!anyway && hasChanges(bound.counts)) {
      setRefreshAsk(bound.counts.changed + bound.counts.formulas);
      return;
    }
    const noun = bound.labelEn.toLowerCase();
    setBusy(true);
    h.notice(`Refreshing ${noun}…`);
    try {
      const summary = await h.queue(async (engine) => {
        const ids = await engine.bindingIds();
        await refreshRecords({ session, sink: sinkOf(engine), ids, pageSize: bound.maxPageSize });
        return engine.erpRefresh();
      });
      await h.afterBind();
      const engine = h.engine();
      setBinding((await engine?.binding()) ?? null);
      h.notice(refreshedText(noun, summary));
    } catch (error) {
      const failure = asErpError(error);
      h.notice(failureText(failure.kind, failure.message, noun).text);
    } finally {
      setBusy(false);
    }
  }, []);

  const sync = useCallback(() => {
    if (!bindingRef.current) return;
    const engine = hostRef.current.engine();
    void engine
      ?.binding()
      .then((next) => setBinding(next))
      .catch(() => undefined);
  }, []);

  const readOnlyAt = useCallback((sheet: number, row: number, col: number) => {
    const b = bindingRef.current;
    if (!b || b.sheet !== sheet || row < b.top || row > b.bottom || col < b.left || col > b.right)
      return false;
    return row === b.top || !b.fields[col - b.left]?.editable;
  }, []);

  const hoverDetails = useCallback(async (sheet: number, row: number, col: number) => {
    const b = bindingRef.current;
    const engine = hostRef.current.engine();
    if (!b || !engine || b.sheet !== sheet) return null;
    if (row < b.top || row > b.bottom || col < b.left || col > b.right) return null;
    const info = await engine.query({ kind: 'erpCell', sheet, row, col });
    return info ? cellTip(info, b.labelEn.toLowerCase()) : null;
  }, []);

  /** Discard Changes: all of them, or those in the selected cells, as one step undo takes back. */
  const discard = useCallback(async (selected: boolean) => {
    const h = hostRef.current;
    const b = bindingRef.current;
    if (!b) return;
    const areas = selected ? h.selection()?.areas : undefined;
    if (selected && !areas?.length) return;
    const before = b.counts.changed + b.counts.formulas;
    await h.run({ kind: 'erpDiscard', ...(areas ? { areas } : {}) });
    const after = await h.engine()?.binding();
    if (after && after.counts.changed + after.counts.formulas === before)
      h.notice(NOTHING_TO_DISCARD);
  }, []);

  /** Next Error: the next cell a local rule refuses, in reading order, round the table. */
  const nextError = useCallback(async () => {
    const h = hostRef.current;
    const b = bindingRef.current;
    const engine = h.engine();
    if (!b || !engine) return;
    const at = h.selection();
    const found = await engine.query({
      kind: 'erpNextError',
      sheet: b.sheet,
      row: at?.row ?? b.top,
      col: at?.col ?? b.left,
    });
    if (found) h.select(found.row, found.col);
    else h.notice(NO_ERRORS);
  }, []);

  /** Reads the binding again after the engine took in what the ERP answered. */
  const reread = useCallback(async () => {
    const next = await hostRef.current.engine()?.binding();
    if (next !== undefined) setBinding(next);
  }, []);

  /**
   * Validate: the server's dry run of every change, local rules' refusals included, so one
   * Validate shows every problem, in batches as large as a commit may be. What comes back goes on
   * the cells (decisions, 2026-11-10, Phase 17).
   */
  const validate = useCallback(async () => {
    const h = hostRef.current;
    const b = bindingRef.current;
    const session = sessionRef.current;
    if (!b || !session || session.state === 'expired') return;
    const noun = b.labelEn.toLowerCase();
    const set = await h.queue((e) => e.query({ kind: 'erpChanges' }));
    if (!set || set.records.length === 0) {
      h.notice(NOTHING_TO_VALIDATE);
      return;
    }
    const sent = tally(set);
    setBusy(true);
    h.notice(validatingText(sent.changes));
    const found = { errors: 0, errorRecords: 0, conflicts: 0, batch: [] as string[] };
    try {
      const all = commitRecords(set.records);
      const size = Math.max(1, b.maxCommitRecords);
      for (let at = 0; at < all.length; at += size) {
        const batch = all.slice(at, at + size);
        const outcome = await sendCommit(session, {
          commitId: newCommitId(),
          company: b.company,
          schemaVersion: b.schemaVersion,
          dryRun: true,
          records: batch,
        });
        const states =
          outcome.kind === 'ok'
            ? { sent: batch, errors: [], conflicts: [] }
            : outcome.kind === 'invalid'
              ? { sent: batch, errors: outcome.errors, conflicts: [] }
              : { sent: batch, errors: [], conflicts: outcome.conflicts };
        const summary = await h.queue((e) => e.erpSetStates(JSON.stringify(states)));
        found.errors += summary.errors;
        found.errorRecords += summary.errorRecords;
        found.conflicts += summary.conflicts;
        found.batch.push(...batchTexts(summary, noun));
      }
      await reread();
      h.notice(validatedText(noun, sent, found));
    } catch (error) {
      const failure = asErpError(error);
      h.notice(failureText(failure.kind, failure.message, noun).text);
    } finally {
      setBusy(false);
    }
  }, [reread]);

  /** Why a commit of `set` can't be sent as it is, or null. */
  const commitRefusal = useCallback((set: ErpChangeset, visible: boolean) => {
    const b = bindingRef.current;
    if (!b) return null;
    const noun = b.labelEn.toLowerCase();
    if (set.errors.length > 0) return { text: localErrorsText(set.errors.length), nextError: true };
    if (set.counts.conflicts > 0)
      return { text: conflictsWaitText(set.counts.conflicts, noun), nextError: false };
    if (set.records.length > b.maxCommitRecords)
      return {
        text: tooManyText(set.records.length, b.maxCommitRecords, noun, visible),
        nextError: false,
      };
    return null;
  }, []);

  /**
   * Commit All Changes, or Commit Changes in Visible Rows: checks what would be sent, then asks,
   * stating the counts (decisions, 2026-11-10, Phase 17).
   */
  const commit = useCallback(
    async (visible: boolean) => {
      const h = hostRef.current;
      const b = bindingRef.current;
      const session = sessionRef.current;
      if (!b || !session || session.state === 'expired') return;
      setReviewOpen(false);
      const set = await h.queue((e) => e.query({ kind: 'erpChanges', visible }));
      if (!set || set.records.length === 0) {
        h.notice(visible ? NOTHING_VISIBLE : NOTHING_TO_COMMIT);
        return;
      }
      const refused = commitRefusal(set, visible);
      if (refused) {
        setRefusal(refused);
        return;
      }
      const counts = tally(set);
      setConfirm({
        visible,
        lines: confirmLines(b.labelEn.toLowerCase(), b.company, counts),
        changes: counts.changes,
      });
    },
    [commitRefusal],
  );

  /** Sends a commit, and applies or marks what comes back. Writes stay held until then. */
  const attempt = useCallback(
    async (flight: InFlight) => {
      const h = hostRef.current;
      const b = bindingRef.current;
      const session = sessionRef.current;
      if (!b || !session) {
        await h.queue((e) => e.erpFreeze(false)).catch(() => undefined);
        return;
      }
      const noun = b.labelEn.toLowerCase();
      inFlightRef.current = flight;
      setBusy(true);
      setCommitPanel({
        phase: 'committing',
        text: committingText(flight.changes, flight.records, noun),
      });
      const release = async () => {
        await h.queue((e) => e.erpFreeze(false)).catch(() => undefined);
        inFlightRef.current = null;
      };
      try {
        const outcome = await sendCommit(session, flight.request);
        const sent = flight.request.records;
        if (outcome.kind === 'ok') {
          const applied = await h.queue((e) =>
            e.erpApplyCommit(JSON.stringify({ sent, records: outcome.response.records })),
          );
          pendingRef.current = null;
          inFlightRef.current = null;
          await h.afterBind();
          await reread();
          setCommitPanel(null);
          h.notice(committedText(flight.changes, flight.records, noun, applied.kept));
          return;
        }
        // Refused, so nothing was written: a later commit takes a new id.
        pendingRef.current = null;
        const summary = await h.queue((e) =>
          e.erpSetStates(
            JSON.stringify(
              outcome.kind === 'invalid'
                ? { sent, errors: outcome.errors, conflicts: [] }
                : { sent, errors: [], conflicts: outcome.conflicts },
            ),
          ),
        );
        await release();
        await reread();
        setCommitPanel({
          phase: 'refused',
          text:
            outcome.kind === 'invalid'
              ? refusedText(noun, summary, batchTexts(summary, noun))
              : conflictText409(summary.conflicts, noun),
          nextError: outcome.kind === 'invalid' && summary.errors > 0,
        });
      } catch (error) {
        const failure = asErpError(error);
        if (failure.kind === 'network' || failure.kind === 'server') {
          // It may have been saved: writes stay held, and Try again sends the same commit.
          setCommitPanel({
            phase: 'unknown',
            text: failure.kind === 'network' ? UNKNOWN_OUTCOME : SERVER_FAILED,
          });
          return;
        }
        // An answer the sheet can't read may follow a save, so its id is kept for the same
        // payload; anything else was refused before a write.
        if (failure.kind !== 'contract') pendingRef.current = null;
        await release();
        await reread();
        setCommitPanel({
          phase: 'refused',
          text:
            failure.kind === 'busy'
              ? BUSY_FAILED
              : commitFailureText(failure.kind, failure.message, noun, b.maxCommitRecords),
          nextError: false,
        });
      } finally {
        setBusy(false);
      }
    },
    [reread],
  );

  /** The confirmation's Commit: writes are held, and what the cells hold now is sent. */
  const send = useCallback(
    async (visible: boolean) => {
      const h = hostRef.current;
      const b = bindingRef.current;
      setConfirm(null);
      if (!b) return;
      const set = await h.queue(async (e) => {
        const now = await e.query({ kind: 'erpChanges', visible });
        if (now && now.records.length > 0) await e.erpFreeze(true);
        return now;
      });
      if (!set || set.records.length === 0) return;
      const refused = commitRefusal(set, visible);
      if (refused) {
        await h.queue((e) => e.erpFreeze(false));
        setRefusal(refused);
        return;
      }
      const records: RecordChange[] = commitRecords(set.records);
      const base = {
        company: b.company,
        schemaVersion: b.schemaVersion,
        dryRun: false,
        records,
      };
      const key = payloadKey(base);
      // A commit whose outcome wasn't known goes again under its id, so a save that did land is
      // answered from the ERP's store rather than refused as a conflict.
      const commitId =
        pendingRef.current?.key === key ? pendingRef.current.commitId : newCommitId();
      pendingRef.current = { key, commitId };
      const counts = tally(set);
      await attempt({
        request: { commitId, ...base },
        changes: counts.changes,
        records: counts.records,
      });
    },
    [attempt, commitRefusal],
  );

  /** Gives up waiting on a commit whose outcome isn't known: writes go again, its id is kept. */
  const stopWaiting = useCallback(async () => {
    setCommitPanel(null);
    inFlightRef.current = null;
    await hostRef.current.queue((e) => e.erpFreeze(false)).catch(() => undefined);
  }, []);

  /** Review Changes: the engine's list of every changed, refused, conflicting or formula field. */
  const openReview = useCallback(async () => {
    const answer = await hostRef.current.queue((e) => e.query({ kind: 'erpReview' }));
    setReview(answer);
    setReviewOpen(true);
  }, []);

  // The list follows the sheet while it is open.
  useEffect(() => {
    if (!reviewOpen || !binding) return;
    let current = true;
    void hostRef.current
      .queue((e) => e.query({ kind: 'erpReview' }))
      .then((answer) => {
        if (current) setReview(answer);
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [binding, reviewOpen]);

  const goTo = useCallback((entry: ErpReviewEntry) => {
    const h = hostRef.current;
    setReviewOpen(false);
    h.select(entry.row, entry.col);
    if (entry.hidden) h.notice(HIDDEN_ROW);
  }, []);

  const discardEntries = useCallback(async (entries: ErpReviewEntry[]) => {
    if (entries.length === 0) return;
    const areas = entries.map((entry) => ({
      top: entry.row,
      left: entry.col,
      bottom: entry.row,
      right: entry.col,
    }));
    await hostRef.current.run({ kind: 'erpDiscard', areas });
  }, []);

  // Closing the tab with changes asks first: a bound workbook keeps no recovery copy until
  // Phase 18 (decisions, 2026-11-10, Phase 16).
  const pending = hasChanges(binding?.counts);
  useEffect(() => {
    if (!pending) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [pending]);

  // The window's title names the ERP data, as the workbook's does.
  useEffect(() => {
    if (!binding) return;
    const before = document.title;
    document.title = `${workbookTitle(binding.label, binding.company)} – CashGear Sheet`;
    return () => {
      document.title = before;
    };
  }, [binding]);

  const tabs = useMemo<CgRibbonTab[]>(() => {
    const shown = Boolean(binding) && binding?.sheet === host.sheet;
    const offline = expired || busy;
    const changed = hasChanges(binding?.counts);
    const values = Boolean(binding?.counts.changed);
    const editable = Boolean(binding?.fields.some((field) => field.editable));
    return [
      {
        key: 'erp-data',
        text: 'ERP Data',
        visible: shown,
        groups: [
          {
            key: 'erp-data-group',
            text: 'Data',
            icon: 'redo',
            items: [
              {
                name: 'erp-refresh',
                text: 'Refresh',
                icon: 'redo',
                size: 'large',
                disabled: offline,
                onClick: () => void refresh(),
              },
            ],
          },
          {
            key: 'erp-changes',
            text: 'Changes',
            icon: 'eraser',
            visible: editable,
            items: [
              {
                name: 'erp-validate',
                text: 'Validate',
                icon: 'check',
                size: 'large',
                disabled: !values || offline,
                onClick: () => void validate(),
              },
              {
                name: 'erp-review',
                text: 'Review Changes',
                icon: 'eye',
                size: 'large',
                disabled: !changed && !binding?.counts.invalid && !binding?.counts.conflicts,
                onClick: () => void openReview(),
              },
              {
                name: 'erp-commit',
                text: 'Commit',
                icon: 'save',
                size: 'large',
                disabled: !values || offline,
                children: [
                  {
                    name: 'erp-commit-all',
                    text: 'Commit All Changes…',
                    onClick: () => void commit(false),
                  },
                  {
                    name: 'erp-commit-visible',
                    text: 'Commit Changes in Visible Rows…',
                    onClick: () => void commit(true),
                  },
                ],
              },
              {
                name: 'erp-discard',
                text: 'Discard Changes',
                icon: 'eraser',
                size: 'large',
                disabled: !changed || busy,
                children: [
                  {
                    name: 'erp-discard-all',
                    text: 'Discard All Changes',
                    onClick: () => void discard(false),
                  },
                  {
                    name: 'erp-discard-selected',
                    text: 'Discard Changes in Selected Cells',
                    onClick: () => void discard(true),
                  },
                ],
              },
              {
                name: 'erp-next-error',
                text: 'Next Error',
                icon: 'error-check',
                size: 'large',
                disabled: !binding?.counts.invalid,
                onClick: () => void nextError(),
              },
            ],
          },
          {
            key: 'erp-connection',
            text: 'Connection',
            icon: 'link',
            items: [
              {
                name: 'erp-connection-info',
                text: 'Connection Info',
                icon: 'link',
                size: 'large',
                onClick: () => setInfoOpen(true),
              },
            ],
          },
        ],
      },
    ];
  }, [
    binding,
    busy,
    commit,
    discard,
    expired,
    host.sheet,
    nextError,
    openReview,
    refresh,
    validate,
  ]);

  const badge = binding ? (
    <CgStatusBadge
      data-testid="erp-status"
      type={expired || binding.counts.invalid > 0 ? 'warning' : 'info'}
      text={badgeText(
        binding.labelEn,
        binding.records,
        binding.missing,
        expired,
        binding.userName,
        binding.company,
        binding.counts,
        binding.fields.some((field) => field.editable),
      )}
    />
  ) : null;

  const cancel = () => abortRef.current?.abort();
  const panelText =
    panel?.phase === 'connecting'
      ? CONNECTING
      : panel?.phase === 'loading'
        ? loadingText(panel.noun, panel.loaded, panel.total)
        : panel?.phase === 'binding'
          ? loadingText(panel.noun, panel.total, panel.total)
          : (panel?.text ?? '');
  const working = panel !== null && panel.phase !== 'failed';
  const progress =
    panel?.phase === 'loading' && panel.total
      ? panel.loaded / panel.total
      : panel?.phase === 'binding'
        ? 1
        : undefined;

  const session = sessionRef.current;
  const infoRows: [string, string][] = binding
    ? [
        ['Data', binding.label],
        ['Company', binding.company],
        ['User', `${binding.userName} (${binding.userId})`],
        [
          'Records',
          binding.missing
            ? `${count(binding.records)} · ${count(binding.missing)} not in ERP`
            : count(binding.records),
        ],
        ['Read from the ERP', new Date(binding.asOf).toLocaleString()],
        [
          'Session',
          expired || !session
            ? 'Ended'
            : `Open until ${new Date(session.expiresAt).toLocaleTimeString()}`,
        ],
        ['Server', apiBase],
        [
          'Editing',
          (() => {
            const editable = binding.fields.filter((field) => field.editable).length;
            return editable === 0
              ? 'Every column is read-only for you.'
              : `${count(editable)} of ${count(binding.fields.length)} columns can be edited. ` +
                  'Changes stay in this sheet until you commit them.';
          })(),
        ],
      ]
    : [];

  const overlays = (
    <>
      <CgWindow
        open={panel !== null}
        onOpenChange={(next) => {
          if (!next && !working) setPanel(null);
        }}
        headerText={openingTitle(panel?.noun ?? null)}
        width={440}
        data-testid="erp-load-panel"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <p className="cgs-dialog-message" data-testid="erp-load-text" role="status">
              {panelText}
            </p>
            {working && (
              <CgProgressBar
                data-testid="erp-load-progress"
                value={progress === undefined ? undefined : Math.round(progress * 100)}
                max={100}
              />
            )}
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            {panel?.phase === 'failed' && panel.retry && launchRef.current && (
              <CgButton
                intent="primary"
                data-testid="erp-load-retry"
                onClick={() => {
                  if (launchRef.current) launch(launchRef.current);
                }}
              >
                Try again
              </CgButton>
            )}
            {working ? (
              <CgButton
                data-testid="erp-load-cancel"
                disabled={panel?.phase === 'binding'}
                onClick={cancel}
              >
                Cancel
              </CgButton>
            ) : (
              <CgButton data-testid="erp-load-close" onClick={() => setPanel(null)}>
                Close
              </CgButton>
            )}
          </div>
        )}
      />
      <CgWindow
        open={refreshAsk !== null}
        onOpenChange={(next) => {
          if (!next) setRefreshAsk(null);
        }}
        headerText="Refresh"
        width={420}
        data-testid="erp-refresh-ask"
        renderBody={() => <p className="cgs-dialog-message">{refreshReplaces(refreshAsk ?? 0)}</p>}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton
              intent="primary"
              data-testid="erp-refresh-anyway"
              onClick={() => {
                setRefreshAsk(null);
                void refresh(true);
              }}
            >
              {REFRESH_ANYWAY}
            </CgButton>
            <CgButton data-testid="erp-refresh-cancel" onClick={() => setRefreshAsk(null)}>
              Cancel
            </CgButton>
          </div>
        )}
      />
      <ReviewChangesDialog
        open={reviewOpen && Boolean(binding)}
        review={review}
        noun={binding?.labelEn.toLowerCase() ?? 'records'}
        rightToLeft={binding?.language === 'ar'}
        busy={busy}
        canCommit={!expired && Boolean(binding?.counts.changed)}
        onClose={() => setReviewOpen(false)}
        onGoTo={goTo}
        onDiscard={(entries) => void discardEntries(entries)}
        onCommit={() => void commit(false)}
      />
      <CgWindow
        open={confirm !== null}
        onOpenChange={(next) => {
          if (!next) setConfirm(null);
        }}
        headerText={COMMIT_TITLE}
        width={480}
        data-testid="erp-commit-confirm"
        renderBody={() => (
          <div className="cgs-erp-lines" data-testid="erp-commit-text">
            {confirm?.lines.map((line) => (
              <p key={line}>{line}</p>
            ))}
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton
              intent="primary"
              data-testid="erp-commit-send"
              onClick={() => {
                if (confirm) void send(confirm.visible);
              }}
            >
              {commitButton(confirm?.changes ?? 0)}
            </CgButton>
            <CgButton
              data-testid="erp-commit-review"
              onClick={() => {
                setConfirm(null);
                void openReview();
              }}
            >
              Review Changes
            </CgButton>
            <CgButton data-testid="erp-commit-cancel" onClick={() => setConfirm(null)}>
              Cancel
            </CgButton>
          </div>
        )}
      />
      <CgWindow
        open={commitPanel !== null}
        onOpenChange={(next) => {
          if (next || !commitPanel || commitPanel.phase === 'committing') return;
          if (commitPanel.phase === 'unknown') void stopWaiting();
          else setCommitPanel(null);
        }}
        headerText={COMMIT_TITLE}
        width={480}
        data-testid="erp-commit-panel"
        renderBody={() => (
          <div className="cgs-dialog-column">
            <p className="cgs-dialog-message" data-testid="erp-commit-status" role="status">
              {commitPanel?.text}
            </p>
            {commitPanel?.phase === 'committing' && (
              <CgProgressBar data-testid="erp-commit-progress" />
            )}
          </div>
        )}
        renderFooter={() =>
          commitPanel?.phase === 'committing' ? null : (
            <div className="cgs-dialog-actions">
              {commitPanel?.phase === 'unknown' && (
                <CgButton
                  intent="primary"
                  data-testid="erp-commit-retry"
                  onClick={() => {
                    const flight = inFlightRef.current;
                    if (flight) void attempt(flight);
                  }}
                >
                  Try again
                </CgButton>
              )}
              {commitPanel?.phase === 'refused' && commitPanel.nextError && (
                <CgButton
                  data-testid="erp-commit-next-error"
                  onClick={() => {
                    setCommitPanel(null);
                    void nextError();
                  }}
                >
                  Next Error
                </CgButton>
              )}
              <CgButton
                data-testid="erp-commit-close"
                onClick={() => {
                  if (commitPanel?.phase === 'unknown') void stopWaiting();
                  else setCommitPanel(null);
                }}
              >
                Close
              </CgButton>
            </div>
          )
        }
      />
      <CgWindow
        open={refusal !== null}
        onOpenChange={(next) => {
          if (!next) setRefusal(null);
        }}
        headerText={COMMIT_TITLE}
        width={440}
        data-testid="erp-commit-refused"
        renderBody={() => (
          <p className="cgs-dialog-message" data-testid="erp-commit-refused-text">
            {refusal?.text}
          </p>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            {refusal?.nextError && (
              <CgButton
                data-testid="erp-commit-refused-next"
                onClick={() => {
                  setRefusal(null);
                  void nextError();
                }}
              >
                Next Error
              </CgButton>
            )}
            <CgButton
              intent="primary"
              data-testid="erp-commit-refused-close"
              onClick={() => setRefusal(null)}
            >
              OK
            </CgButton>
          </div>
        )}
      />
      <CgWindow
        open={infoOpen && Boolean(binding)}
        onOpenChange={(next) => {
          if (!next) setInfoOpen(false);
        }}
        headerText="Connection Info"
        width={460}
        allowDrag
        data-testid="erp-info"
        renderBody={() => (
          <dl className="cgs-erp-info">
            {infoRows.map(([term, value]) => (
              <div key={term} className="cgs-erp-info-row">
                <dt>{term}</dt>
                <dd data-testid={`erp-info-${term.toLowerCase().replace(/\W+/g, '-')}`}>{value}</dd>
              </div>
            ))}
          </dl>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions">
            <CgButton
              intent="primary"
              data-testid="erp-info-close"
              onClick={() => setInfoOpen(false)}
            >
              Close
            </CgButton>
          </div>
        )}
      />
    </>
  );

  return {
    binding,
    tabs,
    badge,
    overlays,
    launch,
    sync,
    readOnlyAt,
    hoverDetails,
    bindStaged,
  };
}
