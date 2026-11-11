// ERP data in the shell (Phases 15 to 18): opening a launch from Cash Gear ERP with its load
// panel, the ERP Data tab with Refresh, Validate, Review Changes, Commit, Discard Changes, Next
// Error, Resolve Conflicts, Next Conflict, Connection Info and Reconnect, the status badge, hover
// details and the window's title, in the ERP user's language. The session renews while the user
// works and ends with the tab; Reconnect takes a session another tab hands over; a commit whose
// outcome isn't known is looked up with CommitStatus before anything sends it again; a recovered
// draft opens disconnected and never commits by itself. The connector fetches and sends, the
// engine reads, binds, merges and applies what the ERP answers, and this only shows what they
// report: it never reads or changes cells itself.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { CgButton, CgProgressBar, CgStatusBadge, CgWindow, type CgRibbonTab } from '@cashgear/ui';
import type {
  Applied,
  CellArea,
  Command,
  EngineClient,
  ErpBinding,
  ErpChangeset,
  ErpConflictEntry,
  ErpConflicts,
  ErpDraftState,
  ErpMarkSummary,
  ErpPendingCommit,
  ErpReview,
  ErpReviewEntry,
  Recoverable,
} from '@cgs/engine-client';
import {
  commitRecords,
  commitStatus,
  ErpError,
  ErpHttp,
  ErpSession,
  newCommitId,
  offerHandover,
  openRecords,
  payloadKey,
  refreshRecords,
  sameScope,
  scopeOf,
  sendCommit,
  waitForHandover,
  type CommitRequest,
  type EntitySchema,
  type ErpLaunch,
  type RecordChange,
  type RecordSink,
  type SessionInfo,
} from '@cgs/erp-connector';
import { hasChanges } from './messages';
import { ResolveConflictsDialog, type Resolution } from './ResolveConflictsDialog';
import { ReviewChangesDialog } from './ReviewChangesDialog';
import { textsFor, type ErpTexts } from './texts';
import { LOCK_PX } from '@cgs/engine-client';

/** The new workbook a binding goes into, named for the data and company. */
function openBlankFor(h: ErpToolsHost, session: SessionInfo, label: string): Promise<void> {
  return h.openBlank(
    `${textsFor(session.language).workbookTitle(label, session.company)}.xlsx`,
    session.language === 'ar',
  );
}

/** Text in both scripts and digits, beside the headings, for the tile worker to ready its fonts. */
const WARM_SAMPLES = ['شركة النيل للتجارة 0123456789', 'Al Noor Trading 0123456789'];

/**
 * What a binding needs ready before its records arrive, made while they download: the new
 * workbook, and the tile worker's fonts for its headings and text (Phase 19, plan B.6).
 */
function prepareFor(h: ErpToolsHost, session: SessionInfo, schema: EntitySchema): Promise<void> {
  const headings = schema.fields.map((field) => field.label[session.language] ?? field.name);
  void h
    .engine()
    ?.warmText([...headings, ...WARM_SAMPLES])
    .catch(() => undefined);
  return openBlankFor(h, session, schema.label[session.language]);
}

export interface ErpToolsHost {
  engine: () => EngineClient | null;
  /** Runs a job in the shell's command queue, after whatever is queued. */
  queue: <T>(job: (engine: EngineClient) => Promise<T>) => Promise<T>;
  /** Opens a new blank workbook named `name`, resolving once its sheet can be written. */
  openBlank: (name: string, rightToLeft: boolean) => Promise<void>;
  /** Puts away the blank workbook a launch opened, when its records then failed to load. */
  closeBlank: () => void;
  /**
   * Holds the grid's redrawing while a binding writes, fits and settles the new workbook, so its
   * tiles are made once, at their fitted widths; letting go draws the sheet afresh.
   */
  holdView: (held: boolean) => void;
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
  /** What earlier pages left to recover. */
  recoverable: () => Promise<Recoverable[]>;
  /** Opens a recovered draft as the workbook, resolving once its replay is done. */
  recover: (entry: Recoverable) => Promise<void>;
  /** The ERP API's origin for this page, for a recovered draft that has no launch. */
  apiBase: () => string;
}

type Panel =
  | { phase: 'connecting'; noun: string | null }
  | { phase: 'loading'; noun: string; loaded: number; total: number | null }
  | { phase: 'binding'; noun: string; total: number }
  | { phase: 'failed'; noun: string | null; text: string; retry: boolean }
  | { phase: 'done'; noun: string | null; text: string };

/** A commit on its way: what was sent, and how many changes and records it carries. */
interface InFlight {
  request: CommitRequest;
  changes: number;
  records: number;
}

/** The commit's panel: while it runs, when its outcome isn't known, and when it was refused. */
type CommitPanel =
  | { phase: 'committing'; text: string }
  | { phase: 'unknown'; text: string; reconnect: boolean }
  | { phase: 'refused'; text: string; nextError: boolean; resolve: boolean };

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
const batchTexts = (t: ErpTexts, summary: ErpMarkSummary, noun: string) => [
  ...new Set(summary.unplaced.map((error) => t.batchErrorText(error.code, error.message, noun))),
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
  /** Whether a launch is being opened, when the shell leaves ERP drafts to it. */
  launching: () => boolean;
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
  /** The engine's English refusal or limit in the session's language. */
  localize: (message: string) => string;
  /**
   * Takes up a recovered draft once the shell has opened it: disconnected, waiting for a launch to
   * reconnect it. Resolves false when its ERP connection couldn't be recovered.
   */
  recovered: () => Promise<boolean>;
  /** The texts of an ERP draft offered back, in its own language. */
  textsFor: (language: string | null | undefined) => ErpTexts;
  /** For the bind bench: binds records already staged in the engine, as opening does. */
  bindStaged: (session: SessionInfo, label: string, opened?: Promise<void>) => Promise<ErpBinding>;
  /** Readies what a binding needs, as a launch does while its records download. */
  prepare: (session: SessionInfo, schema: EntitySchema) => Promise<void>;
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

/** The browser's own language, until a session says the ERP user's. */
const browserLanguage = () =>
  typeof navigator !== 'undefined' && navigator.language?.toLowerCase().startsWith('ar')
    ? 'ar'
    : 'en';

/**
 * Room a heading's text leaves at 100%: a filter button's width, as the grid draws it
 * (`GridController.buttonBox`), and a read-only column's lock.
 */
const HEADING_ROOM_PX = 17 + LOCK_PX;

/** The longest texts by characters measured in each column: enough for its widest, and quick. */
const FIT_SAMPLE = 16;
/** How long after a binding shows its recovery draft starts. */
const DRAFT_START_MS = 300;

export function useErpTools(host: ErpToolsHost): ErpTools {
  const hostRef = useRef(host);
  hostRef.current = host;
  const [binding, setBinding] = useState<ErpBinding | null>(null);
  const bindingRef = useRef<ErpBinding | null>(null);
  bindingRef.current = binding;
  const [language, setLanguage] = useState<string>(browserLanguage);
  const t = useMemo(() => textsFor(binding?.language ?? language), [binding?.language, language]);
  const tRef = useRef(t);
  tRef.current = t;
  const [panel, setPanel] = useState<Panel | null>(null);
  const [expired, setExpired] = useState(false);
  /** A recovered draft, which has no session until a launch hands it one. */
  const [disconnected, setDisconnected] = useState(false);
  const [busy, setBusy] = useState(false);
  const [infoOpen, setInfoOpen] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [review, setReview] = useState<ErpReview | null>(null);
  const [resolveOpen, setResolveOpen] = useState(false);
  const [conflicts, setConflicts] = useState<ErpConflicts | null>(null);
  const [reconnectOpen, setReconnectOpen] = useState(false);
  const [draft, setDraft] = useState<ErpDraftState>('none');
  /** A launch whose company, user and entity match a draft an earlier page left. */
  const [draftOffer, setDraftOffer] = useState<{
    entry: Recoverable;
    session: ErpSession;
    launch: ErpLaunch;
  } | null>(null);
  /** The commit's confirmation, with what it would send. */
  const [confirm, setConfirm] = useState<{
    visible: boolean;
    lines: string[];
    changes: number;
  } | null>(null);
  const [commitPanel, setCommitPanel] = useState<CommitPanel | null>(null);
  /** Why Commit sent nothing, in a message box. */
  const [refusal, setRefusal] = useState<{
    text: string;
    nextError: boolean;
    resolve: boolean;
  } | null>(null);
  /**
   * A commit whose outcome isn't known: kept so CommitStatus is asked before anything sends it
   * again, and so the same payload goes under the same id.
   */
  const pendingRef = useRef<ErpPendingCommit | null>(null);
  const inFlightRef = useRef<InFlight | null>(null);
  const sessionRef = useRef<ErpSession | null>(null);
  const unwatchRef = useRef<(() => void) | null>(null);
  const waitingRef = useRef<(() => void) | null>(null);
  const launchRef = useRef<ErpLaunch | null>(null);
  const launchingRef = useRef(false);
  const abortRef = useRef<AbortController | null>(null);
  const [apiBase, setApiBase] = useState('');
  const apiBaseRef = useRef('');
  apiBaseRef.current = apiBase;

  const noun = binding?.labelEn.toLowerCase() ?? 'records';

  /** Remembers a commit whose outcome isn't known, in the draft too, or forgets it. */
  const notePending = useCallback(async (pending: ErpPendingCommit | null) => {
    pendingRef.current = pending;
    await hostRef.current.queue((e) => e.erpNoteCommit(pending)).catch(() => undefined);
  }, []);

  /** Uses a session from here on: the badge follows it, and its end shows Reconnect. */
  const attach = useCallback((session: ErpSession) => {
    unwatchRef.current?.();
    sessionRef.current = session;
    setExpired(session.state === 'expired');
    setDisconnected(false);
    setLanguage(session.info.language);
    unwatchRef.current = session.onState((state) => {
      if (state !== 'expired' || sessionRef.current !== session) return;
      setExpired(true);
      if (bindingRef.current) hostRef.current.notice(tRef.current.SESSION_ENDED);
    });
  }, []);

  /**
   * Binds what the engine has staged into a new workbook named for the data and company, fits its
   * columns and leaves nothing to undo. The last step of opening, and what the bind bench times.
   */
  const bindStaged = useCallback(
    async (session: SessionInfo, label: string, opened?: Promise<void>): Promise<ErpBinding> => {
      const h = hostRef.current;
      // Marks for bench/measure-erp-bind.mjs, which reads where binding's time goes.
      const mark = (name: string) => performance.mark(`erp:${name}`);
      mark('begin');
      await (opened ?? openBlankFor(h, session, label));
      mark('opened');
      h.holdView(true);
      let bound: ErpBinding;
      try {
        bound = await h.queue((e) => e.erpBind(JSON.stringify(session)));
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
      } finally {
        h.holdView(false);
      }
      await h.afterBind();
      mark('shown');
      // The recovery draft starts once the sheet is drawn, out of the way of its first tiles.
      setTimeout(() => {
        void h
          .queue((e) => e.erpStartDraft())
          .then(setDraft)
          .catch(() => undefined);
      }, DRAFT_START_MS);
      return bound;
    },
    [],
  );

  /** Reads the binding again after the engine took in what the ERP answered. */
  const reread = useCallback(async () => {
    const engine = hostRef.current.engine();
    const next = await engine?.binding();
    if (next !== undefined) setBinding(next);
    if (engine) setDraft(await engine.erpDraftState().catch(() => 'none' as const));
  }, []);

  /** Applies a commit the ERP saved: the cells that still hold what was sent take its values. */
  const applySaved = useCallback(
    async (sent: unknown[], saved: unknown[], changes: number, records: number) => {
      const h = hostRef.current;
      const applied = await h.queue((e) =>
        e.erpApplyCommit(JSON.stringify({ sent, records: saved })),
      );
      await notePending(null);
      inFlightRef.current = null;
      await h.afterBind();
      await reread();
      setCommitPanel(null);
      h.notice(
        tRef.current.committedText(
          changes,
          records,
          bindingRef.current?.labelEn.toLowerCase() ?? 'records',
          applied.kept,
        ),
      );
    },
    [notePending, reread],
  );

  /**
   * Asks Cash Gear ERP whether the commit whose outcome isn't known was saved, before anything
   * sends it again (plan B.4): applied if it was, forgotten if it wasn't.
   */
  const askStatus = useCallback(async (): Promise<'committed' | 'unknown' | 'failed'> => {
    const h = hostRef.current;
    const pending = pendingRef.current;
    const session = sessionRef.current;
    if (!pending || !session || session.state === 'expired') return 'failed';
    h.notice(tRef.current.ASKING_STATUS);
    try {
      const status = await commitStatus(session, pending.commitId);
      if (status.status === 'committed') {
        await applySaved(pending.sent, [...status.records], pending.changes, pending.records);
        return 'committed';
      }
      // Nothing was written under that id: the changes wait, and no commit is held.
      await notePending(null);
      inFlightRef.current = null;
      await h.queue((e) => e.erpFreeze(false)).catch(() => undefined);
      return 'unknown';
    } catch (error) {
      const failure = asErpError(error);
      h.notice(
        failure.kind === 'session'
          ? tRef.current.SESSION_ENDED
          : tRef.current.failureText(failure.kind, failure.message, 'records').text,
      );
      return 'failed';
    }
  }, [applySaved, notePending]);

  const refresh = useCallback(async () => {
    const h = hostRef.current;
    const session = sessionRef.current;
    const bound = bindingRef.current;
    if (!session || !bound || session.state === 'expired') return;
    const n = bound.labelEn.toLowerCase();
    setBusy(true);
    h.notice(tRef.current.refreshingText(n));
    try {
      const summary = await h.queue(async (engine) => {
        const ids = await engine.bindingIds();
        await refreshRecords({ session, sink: sinkOf(engine), ids, pageSize: bound.maxPageSize });
        return engine.erpRefresh();
      });
      await h.afterBind();
      await reread();
      h.notice(tRef.current.refreshedText(n, summary));
    } catch (error) {
      const failure = asErpError(error);
      h.notice(
        failure.kind === 'session'
          ? tRef.current.SESSION_ENDED
          : tRef.current.failureText(failure.kind, failure.message, n).text,
      );
    } finally {
      setBusy(false);
    }
  }, [reread]);

  /**
   * Once a session is attached to a binding that had none or lost its own: a commit whose outcome
   * wasn't known is looked up first, then the records are merged with what the ERP has now.
   */
  const afterConnect = useCallback(async () => {
    if (pendingRef.current) {
      const found = await askStatus();
      if (found === 'unknown') hostRef.current.notice(tRef.current.STATUS_UNKNOWN);
      if (found === 'failed') return;
      setCommitPanel(null);
    }
    await refresh();
  }, [askStatus, refresh]);

  /** Stops waiting for a launch to hand its session over. */
  const stopWaiting = useCallback(() => {
    waitingRef.current?.();
    waitingRef.current = null;
  }, []);

  /**
   * Waits for the user's next launch from Cash Gear ERP to hand its session over, for the same
   * company, user and entity only (decisions, 2026-11-11, Phase 18).
   */
  const waitForLaunch = useCallback(() => {
    const b = bindingRef.current;
    if (!b || waitingRef.current) return;
    const scope = { company: b.company, userId: b.userId, entity: b.entity };
    waitingRef.current = waitForHandover({
      scope,
      take: async (given) => {
        const current = bindingRef.current;
        const base = apiBaseRef.current || hostRef.current.apiBase();
        const session = ErpSession.adopt(new ErpHttp({ baseUrl: base }), given);
        if (!current || !sameScope(scopeOf(session.info), scope)) {
          session.end();
          return false;
        }
        setApiBase(base);
        attach(session);
        // The channel stops by itself once it has said the session was taken.
        waitingRef.current = null;
        setReconnectOpen(false);
        hostRef.current.notice(tRef.current.RECONNECTED);
        void afterConnect();
        return true;
      },
      onRefused: (_why, given) => {
        // A session for another company, user or entity is ended here, so nothing can use it.
        try {
          ErpSession.adopt(
            new ErpHttp({ baseUrl: apiBaseRef.current || hostRef.current.apiBase() }),
            given,
          ).end();
        } catch {
          // Not a session at all.
        }
        const current = bindingRef.current;
        if (current) {
          hostRef.current.notice(
            tRef.current.refusedHandoverText(
              current.company,
              current.userName,
              current.labelEn.toLowerCase(),
            ),
          );
        }
      },
    });
  }, [afterConnect, attach, stopWaiting]);

  /** Opens records into a new workbook with a session already redeemed. */
  const load = useCallback(
    async (session: ErpSession, launch: ErpLaunch, controller: AbortController) => {
      const h = hostRef.current;
      const engine = h.engine();
      if (!engine) return;
      let n: string = nounOf(session.info.entity);
      // The new workbook opens while the records download, so binding starts as they arrive.
      let blank: Promise<void> | undefined;
      let opened: Awaited<ReturnType<typeof openRecords>>;
      try {
        opened = await openRecords({
          http: new ErpHttp({ baseUrl: launch.apiBase }),
          ticket: launch.ticket,
          session,
          sink: sinkOf(engine),
          signal: controller.signal,
          onSchema: (schema, given) => {
            blank = prepareFor(h, given.info, schema);
            blank.catch(() => undefined);
          },
          onProgress: (progress) => {
            if (progress.stage !== 'loading') return;
            setPanel({
              phase: 'loading',
              noun: n,
              loaded: progress.loaded,
              total: progress.total,
            });
          },
        });
      } catch (error) {
        // Nothing was opened, so the blank workbook goes too.
        if (blank) {
          await blank.catch(() => undefined);
          h.closeBlank();
        }
        throw error;
      }
      n = opened.schema.label.en.toLowerCase();
      setPanel({ phase: 'binding', noun: n, total: opened.loaded });
      const bound = await bindStaged(
        session.info,
        opened.schema.label[session.info.language],
        blank,
      );
      attach(session);
      setBinding(bound);
      setPanel(null);
    },
    [attach, bindStaged],
  );

  const open = useCallback(
    async (launch: ErpLaunch) => {
      const h = hostRef.current;
      if (!h.engine()) return;
      launchRef.current = launch;
      launchingRef.current = true;
      setApiBase(launch.apiBase);
      const controller = new AbortController();
      abortRef.current = controller;
      let n: string | null = sessionRef.current ? nounOf(sessionRef.current.info.entity) : null;
      setPanel({ phase: 'connecting', noun: n });
      try {
        let session = sessionRef.current;
        if (!session || session.state === 'expired') {
          session = await ErpSession.redeem(
            new ErpHttp({ baseUrl: launch.apiBase }),
            launch.ticket,
            {
              signal: controller.signal,
            },
          );
          sessionRef.current = session;
          setLanguage(session.info.language);
          n = nounOf(session.info.entity);
          setPanel({ phase: 'connecting', noun: n });
          // A tab waiting to reconnect for the same company, user and entity takes it.
          const handed = await offerHandover(session.handover());
          if (handed === 'taken') {
            sessionRef.current = null;
            setPanel({ phase: 'done', noun: n, text: textsFor(session.info.language).HANDED_OVER });
            window.close();
            return;
          }
          if (handed === 'refused') {
            session.end();
            sessionRef.current = null;
            setPanel({
              phase: 'failed',
              noun: n,
              text: textsFor(session.info.language).HANDOVER_REFUSED,
              retry: false,
            });
            return;
          }
          // With no tab waiting, a draft an earlier page left for the same scope is offered.
          const found = (await h.recoverable().catch(() => [] as Recoverable[])).find(
            (entry) => entry.erp && sameScope(entry.erp, session!.info),
          );
          if (found) {
            setPanel(null);
            setDraftOffer({ entry: found, session, launch });
            return;
          }
        }
        await load(session, launch, controller);
      } catch (error) {
        const failure = asErpError(error);
        if (failure.kind === 'ticket' || failure.kind === 'session') sessionRef.current = null;
        const { text, retry } = tRef.current.failureText(
          failure.kind,
          failure.message,
          n ?? 'this data',
        );
        setPanel({ phase: 'failed', noun: n, text, retry });
      } finally {
        if (abortRef.current === controller) abortRef.current = null;
        launchingRef.current = false;
      }
    },
    [load],
  );

  const launch = useCallback((given: ErpLaunch) => void open(given), [open]);

  /** The launch's draft offer: recover it into this session, or open the data afresh. */
  const answerDraft = useCallback(
    async (recover: boolean) => {
      const offer = draftOffer;
      setDraftOffer(null);
      if (!offer) return;
      const h = hostRef.current;
      const controller = new AbortController();
      abortRef.current = controller;
      try {
        if (!recover) {
          setPanel({ phase: 'connecting', noun: nounOf(offer.session.info.entity) });
          await load(offer.session, offer.launch, controller);
          return;
        }
        await h.recover(offer.entry);
        const result = await h.queue((e) => e.erpRecovered());
        const recovered = await h.engine()?.binding();
        if (!result || result.failed || !recovered) {
          offer.session.end();
          sessionRef.current = null;
          setPanel({
            phase: 'failed',
            noun: null,
            text: tRef.current.recoverFailedText(nounOf(offer.session.info.entity)),
            retry: false,
          });
          return;
        }
        pendingRef.current = result.pendingCommit;
        setBinding(recovered);
        attach(offer.session);
        await reread();
        await afterConnect();
      } catch (error) {
        const failure = asErpError(error);
        setPanel({
          phase: 'failed',
          noun: null,
          text: tRef.current.failureText(failure.kind, failure.message, 'this data').text,
          retry: false,
        });
      } finally {
        if (abortRef.current === controller) abortRef.current = null;
      }
    },
    [afterConnect, attach, draftOffer, load, reread],
  );

  /** A draft the shell recovered at start: disconnected, waiting for a launch to reconnect it. */
  const recovered = useCallback(async () => {
    const h = hostRef.current;
    const result = await h.queue((e) => e.erpRecovered()).catch(() => null);
    if (!result) return true;
    if (result.failed) return false;
    const next = await h.engine()?.binding();
    if (!next) return true;
    sessionRef.current = null;
    pendingRef.current = result.pendingCommit;
    setBinding(next);
    bindingRef.current = next;
    setLanguage(next.language);
    setApiBase(h.apiBase());
    setDisconnected(true);
    setExpired(false);
    await reread();
    waitForLaunch();
    h.notice(textsFor(next.language).RECOVERED);
    return true;
  }, [reread, waitForLaunch]);

  const sync = useCallback(() => {
    if (!bindingRef.current) return;
    const engine = hostRef.current.engine();
    void engine
      ?.binding()
      .then((next) => {
        setBinding(next);
        if (!next) return;
        return engine.erpDraftState().then(setDraft);
      })
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
    return info ? tRef.current.cellTip(info, b.labelEn.toLowerCase()) : null;
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
      h.notice(tRef.current.NOTHING_TO_DISCARD);
  }, []);

  /** Next Error or Next Conflict: the next such cell in reading order, round the table. */
  const next = useCallback(async (conflict: boolean) => {
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
      conflict,
    });
    if (found) h.select(found.row, found.col);
    else h.notice(conflict ? tRef.current.NO_CONFLICTS : tRef.current.NO_ERRORS);
  }, []);
  const nextError = useCallback(() => next(false), [next]);

  /**
   * Validate: the server's dry run of every change, local rules' refusals included, so one
   * Validate shows every problem, in batches as large as a commit may be. What comes back goes on
   * the cells (decisions, 2026-11-10, Phase 17); a 409 is merged three ways (Phase 18).
   */
  const validate = useCallback(async () => {
    const h = hostRef.current;
    const b = bindingRef.current;
    const session = sessionRef.current;
    if (!b || !session || session.state === 'expired') return;
    const n = b.labelEn.toLowerCase();
    const set = await h.queue((e) => e.query({ kind: 'erpChanges' }));
    if (!set || set.records.length === 0) {
      h.notice(tRef.current.NOTHING_TO_VALIDATE);
      return;
    }
    const sent = tally(set);
    setBusy(true);
    h.notice(tRef.current.validatingText(sent.changes));
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
        found.batch.push(...batchTexts(tRef.current, summary, n));
      }
      await h.afterBind();
      await reread();
      h.notice(tRef.current.validatedText(n, sent, found));
    } catch (error) {
      const failure = asErpError(error);
      h.notice(
        failure.kind === 'session'
          ? tRef.current.SESSION_ENDED
          : tRef.current.failureText(failure.kind, failure.message, n).text,
      );
    } finally {
      setBusy(false);
    }
  }, [reread]);

  /** Why a commit of `set` can't be sent as it is, or null. */
  const commitRefusal = useCallback((set: ErpChangeset, visible: boolean) => {
    const b = bindingRef.current;
    if (!b) return null;
    const n = b.labelEn.toLowerCase();
    const texts = tRef.current;
    if (set.errors.length > 0)
      return { text: texts.localErrorsText(set.errors.length), nextError: true, resolve: false };
    if (set.counts.conflicts > 0)
      return {
        text: texts.conflictsWaitText(set.counts.conflicts, n),
        nextError: false,
        resolve: true,
      };
    if (set.records.length > b.maxCommitRecords)
      return {
        text: texts.tooManyText(set.records.length, b.maxCommitRecords, n, visible),
        nextError: false,
        resolve: false,
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
        h.notice(visible ? tRef.current.NOTHING_VISIBLE : tRef.current.NOTHING_TO_COMMIT);
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
        lines: tRef.current.confirmLines(b.labelEn.toLowerCase(), b.company, counts),
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
      const n = b.labelEn.toLowerCase();
      inFlightRef.current = flight;
      setBusy(true);
      setCommitPanel({
        phase: 'committing',
        text: tRef.current.committingText(flight.changes, flight.records, n),
      });
      const release = async () => {
        await h.queue((e) => e.erpFreeze(false)).catch(() => undefined);
        inFlightRef.current = null;
      };
      try {
        const outcome = await sendCommit(session, flight.request);
        const sent = flight.request.records;
        if (outcome.kind === 'ok') {
          await applySaved(
            [...sent],
            [...outcome.response.records],
            flight.changes,
            flight.records,
          );
          return;
        }
        // Refused, so nothing was written: a later commit takes a new id.
        await notePending(null);
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
        await h.afterBind();
        await reread();
        setCommitPanel({
          phase: 'refused',
          text:
            outcome.kind === 'invalid'
              ? tRef.current.refusedText(n, summary, batchTexts(tRef.current, summary, n))
              : tRef.current.conflictText409(summary, n),
          nextError: outcome.kind === 'invalid' && summary.errors > 0,
          resolve: outcome.kind === 'conflict' && summary.conflicts > 0,
        });
      } catch (error) {
        const failure = asErpError(error);
        if (failure.kind === 'network' || failure.kind === 'server' || failure.kind === 'session') {
          // It may have been saved, a 401 too, since a browser can resend a POST by itself:
          // writes stay held, and CommitStatus is asked before anything sends it again.
          const pending = pendingRef.current;
          if (pending) await notePending(pending);
          setCommitPanel({
            phase: 'unknown',
            text:
              failure.kind === 'session'
                ? tRef.current.commitFailureText('session', failure.message, n, b.maxCommitRecords)
                : failure.kind === 'network'
                  ? tRef.current.UNKNOWN_OUTCOME
                  : tRef.current.SERVER_FAILED,
            reconnect: failure.kind === 'session',
          });
          return;
        }
        // An answer the sheet can't read may follow a save, so its id is kept for the same
        // payload; anything else was refused before a write.
        if (failure.kind !== 'contract') await notePending(null);
        await release();
        await reread();
        setCommitPanel({
          phase: 'refused',
          text:
            failure.kind === 'busy'
              ? tRef.current.BUSY_FAILED
              : tRef.current.commitFailureText(
                  failure.kind,
                  failure.message,
                  n,
                  b.maxCommitRecords,
                ),
          nextError: false,
          resolve: false,
        });
      } finally {
        setBusy(false);
      }
    },
    [applySaved, notePending, reread],
  );

  /** Try again after an unknown outcome: CommitStatus first, then the same commit if unsaved. */
  const retry = useCallback(async () => {
    const flight = inFlightRef.current;
    const session = sessionRef.current;
    if (!flight || !session || session.state === 'expired') {
      setReconnectOpen(true);
      waitForLaunch();
      return;
    }
    setBusy(true);
    try {
      const found = await askStatus();
      if (found === 'committed' || found === 'failed') return;
      // Nothing was written under that id, so the same commit goes again.
      await hostRef.current.queue((e) => e.erpFreeze(true));
      await notePending({
        commitId: flight.request.commitId,
        key: payloadKey(flight.request),
        sent: [...flight.request.records],
        changes: flight.changes,
        records: flight.records,
      });
      await attempt(flight);
    } finally {
      setBusy(false);
    }
  }, [askStatus, attempt, notePending, waitForLaunch]);

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
      const counts = tally(set);
      await notePending({
        commitId,
        key,
        sent: [...records],
        changes: counts.changes,
        records: counts.records,
      });
      await attempt({
        request: { commitId, ...base },
        changes: counts.changes,
        records: counts.records,
      });
    },
    [attempt, commitRefusal, notePending],
  );

  /** Gives up waiting on a commit whose outcome isn't known: writes go again, its id is kept. */
  const stopCommitWait = useCallback(async () => {
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

  /** Resolve Conflicts: the engine's list of every field you and the ERP both changed. */
  const openResolve = useCallback(async () => {
    setRefusal(null);
    setCommitPanel(null);
    const answer = await hostRef.current.queue((e) => e.query({ kind: 'erpConflicts' }));
    setConflicts(answer);
    setResolveOpen(true);
  }, []);

  // The lists follow the sheet while they're open.
  useEffect(() => {
    if (!binding || (!reviewOpen && !resolveOpen)) return;
    let current = true;
    const h = hostRef.current;
    if (reviewOpen)
      void h
        .queue((e) => e.query({ kind: 'erpReview' }))
        .then((answer) => {
          if (current) setReview(answer);
        })
        .catch(() => undefined);
    if (resolveOpen)
      void h
        .queue((e) => e.query({ kind: 'erpConflicts' }))
        .then((answer) => {
          if (current) setConflicts(answer);
        })
        .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [binding, resolveOpen, reviewOpen]);

  const resolve = useCallback(
    async (resolution: Resolution) => {
      const h = hostRef.current;
      setBusy(true);
      try {
        const resolved = await h.queue((e) => e.erpResolve(JSON.stringify(resolution)));
        await h.afterBind();
        await reread();
        h.notice(
          tRef.current.resolvedText(
            resolved,
            bindingRef.current?.labelEn.toLowerCase() ?? 'records',
          ),
        );
      } finally {
        setBusy(false);
      }
    },
    [reread],
  );

  const goTo = useCallback((entry: { row: number; col: number; hidden: boolean }) => {
    const h = hostRef.current;
    setReviewOpen(false);
    setResolveOpen(false);
    h.select(entry.row, entry.col);
    if (entry.hidden) h.notice(tRef.current.HIDDEN_ROW);
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

  // The session renews while the user works in this tab (decisions, 2026-11-11, Phase 18).
  useEffect(() => {
    if (!binding) return;
    const touch = () => sessionRef.current?.touch();
    const options = { capture: true, passive: true };
    window.addEventListener('keydown', touch, options);
    window.addEventListener('pointerdown', touch, options);
    window.addEventListener('wheel', touch, options);
    return () => {
      window.removeEventListener('keydown', touch, options);
      window.removeEventListener('pointerdown', touch, options);
      window.removeEventListener('wheel', touch, options);
    };
  }, [binding]);

  // The session ends with the tab, and when its workbook closes.
  useEffect(() => {
    const end = () => sessionRef.current?.end();
    window.addEventListener('pagehide', end);
    return () => window.removeEventListener('pagehide', end);
  }, []);
  const hadBinding = useRef(false);
  useEffect(() => {
    if (binding) {
      hadBinding.current = true;
      return;
    }
    if (!hadBinding.current) return;
    hadBinding.current = false;
    stopWaiting();
    unwatchRef.current?.();
    sessionRef.current?.end();
    sessionRef.current = null;
    pendingRef.current = null;
    setDisconnected(false);
    setExpired(false);
  }, [binding, stopWaiting]);

  // Closing the tab with changes asks first when the sheet keeps no recovery draft: past its cap,
  // or with no room on the device (decisions, 2026-11-11, Phase 18).
  const pending = hasChanges(binding?.counts);
  const unprotected = pending && draft === 'dropped';
  useEffect(() => {
    if (!unprotected) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [unprotected]);

  // The window's title names the ERP data, as the workbook's does.
  useEffect(() => {
    if (!binding) return;
    const before = document.title;
    document.title = `${t.workbookTitle(binding.label, binding.company)} – CashGear Sheet`;
    return () => {
      document.title = before;
    };
  }, [binding, t]);

  const offline = expired || disconnected || busy;
  const tabs = useMemo<CgRibbonTab[]>(() => {
    const shown = Boolean(binding) && binding?.sheet === host.sheet;
    const changed = hasChanges(binding?.counts);
    const values = Boolean(binding?.counts.changed);
    const editable = Boolean(binding?.fields.some((field) => field.editable));
    const inConflict = Boolean(binding?.counts.conflicts);
    return [
      {
        key: 'erp-data',
        text: t.TAB,
        visible: shown,
        groups: [
          {
            key: 'erp-data-group',
            text: t.GROUP_DATA,
            icon: 'redo',
            items: [
              {
                name: 'erp-refresh',
                text: t.REFRESH,
                icon: 'redo',
                size: 'large',
                disabled: offline,
                onClick: () => void refresh(),
              },
            ],
          },
          {
            key: 'erp-changes',
            text: t.GROUP_CHANGES,
            icon: 'eraser',
            visible: editable,
            items: [
              {
                name: 'erp-validate',
                text: t.VALIDATE,
                icon: 'check',
                size: 'large',
                disabled: !values || offline,
                onClick: () => void validate(),
              },
              {
                name: 'erp-review',
                text: t.REVIEW,
                icon: 'eye',
                size: 'large',
                disabled: !changed && !binding?.counts.invalid && !binding?.counts.conflicts,
                onClick: () => void openReview(),
              },
              {
                name: 'erp-commit',
                text: t.COMMIT,
                icon: 'save',
                size: 'large',
                disabled: !values || offline,
                children: [
                  {
                    name: 'erp-commit-all',
                    text: t.COMMIT_ALL,
                    onClick: () => void commit(false),
                  },
                  {
                    name: 'erp-commit-visible',
                    text: t.COMMIT_VISIBLE,
                    onClick: () => void commit(true),
                  },
                ],
              },
              {
                name: 'erp-discard',
                text: t.DISCARD,
                icon: 'eraser',
                size: 'large',
                disabled: !changed || busy,
                children: [
                  {
                    name: 'erp-discard-all',
                    text: t.DISCARD_ALL,
                    onClick: () => void discard(false),
                  },
                  {
                    name: 'erp-discard-selected',
                    text: t.DISCARD_SELECTED,
                    onClick: () => void discard(true),
                  },
                ],
              },
              {
                name: 'erp-next-error',
                text: t.NEXT_ERROR,
                icon: 'error-check',
                size: 'large',
                disabled: !binding?.counts.invalid,
                onClick: () => void nextError(),
              },
            ],
          },
          {
            key: 'erp-conflicts',
            text: t.GROUP_CONFLICTS,
            icon: 'cells-merge',
            visible: editable,
            items: [
              {
                name: 'erp-resolve',
                text: t.RESOLVE,
                icon: 'cells-merge',
                size: 'large',
                disabled: !inConflict || busy,
                onClick: () => void openResolve(),
              },
              {
                name: 'erp-next-conflict',
                text: t.NEXT_CONFLICT,
                icon: 'error-check',
                size: 'large',
                disabled: !inConflict,
                onClick: () => void next(true),
              },
            ],
          },
          {
            key: 'erp-connection',
            text: t.GROUP_CONNECTION,
            icon: 'link',
            items: [
              {
                name: 'erp-connection-info',
                text: t.CONNECTION_INFO,
                icon: 'link',
                size: 'large',
                onClick: () => setInfoOpen(true),
              },
              {
                name: 'erp-reconnect',
                text: t.RECONNECT,
                icon: 'link',
                size: 'large',
                disabled: !(expired || disconnected),
                onClick: () => {
                  setReconnectOpen(true);
                  waitForLaunch();
                },
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
    disconnected,
    expired,
    host.sheet,
    next,
    nextError,
    offline,
    openResolve,
    openReview,
    refresh,
    t,
    validate,
    waitForLaunch,
  ]);

  const badge = binding ? (
    <CgStatusBadge
      data-testid="erp-status"
      type={
        expired || disconnected || binding.counts.invalid > 0 || binding.counts.conflicts > 0
          ? 'warning'
          : 'info'
      }
      text={t.badgeText(
        binding.label,
        binding.records,
        { expired, disconnected, noDraft: draft === 'dropped' },
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
      ? t.CONNECTING
      : panel?.phase === 'loading'
        ? t.loadingText(panel.noun, panel.loaded, panel.total)
        : panel?.phase === 'binding'
          ? t.loadingText(panel.noun, panel.total, panel.total)
          : (panel?.text ?? '');
  const working = panel !== null && panel.phase !== 'failed' && panel.phase !== 'done';
  const progress =
    panel?.phase === 'loading' && panel.total
      ? panel.loaded / panel.total
      : panel?.phase === 'binding'
        ? 1
        : undefined;

  const session = sessionRef.current;
  const infoRows: [string, string, string][] = binding
    ? [
        ['data', t.INFO_DATA, binding.label],
        ['company', t.INFO_COMPANY, binding.company],
        ['user', t.INFO_USER, `${binding.userName} (${binding.userId})`],
        ['records', t.INFO_RECORDS, t.infoRecords(binding.records, binding.counts.gone)],
        ['read-from-the-erp', t.INFO_READ, t.readAt(binding.asOf)],
        [
          'session',
          t.INFO_SESSION,
          disconnected
            ? t.DISCONNECTED
            : expired || !session
              ? t.SESSION_ENDED_SHORT
              : t.openUntil(session.expiresAt),
        ],
        ['server', t.INFO_SERVER, apiBase],
        [
          'last-problem',
          t.INFO_PROBLEM,
          session?.lastFailure
            ? t.problemText(
                session.lastFailure.status,
                session.lastFailure.at,
                session.lastFailure.correlationId,
              )
            : t.NO_PROBLEM,
        ],
        [
          'editing',
          t.INFO_EDITING,
          t.editingText(
            binding.fields.filter((field) => field.editable).length,
            binding.fields.length,
          ),
        ],
        ['recovery', t.INFO_RECOVERY, t.draftText(draft !== 'dropped')],
      ]
    : [];

  const draftTexts = draftOffer ? textsFor(draftOffer.session.info.language) : t;
  const overlays = (
    <>
      <CgWindow
        open={panel !== null}
        onOpenChange={(open) => {
          if (!open && !working) setPanel(null);
        }}
        headerText={t.openingTitle(panel?.noun ?? null)}
        width={440}
        data-testid="erp-load-panel"
        renderBody={() => (
          <div className="cgs-dialog-column" dir={t.dir}>
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
          <div className="cgs-dialog-actions" dir={t.dir}>
            {panel?.phase === 'failed' && panel.retry && launchRef.current && (
              <CgButton
                intent="primary"
                data-testid="erp-load-retry"
                onClick={() => {
                  if (launchRef.current) launch(launchRef.current);
                }}
              >
                {t.TRY_AGAIN}
              </CgButton>
            )}
            {working ? (
              <CgButton
                data-testid="erp-load-cancel"
                disabled={panel?.phase === 'binding'}
                onClick={cancel}
              >
                {t.CANCEL}
              </CgButton>
            ) : (
              <CgButton data-testid="erp-load-close" onClick={() => setPanel(null)}>
                {t.CLOSE}
              </CgButton>
            )}
          </div>
        )}
      />
      <CgWindow
        open={draftOffer !== null}
        onOpenChange={(open) => {
          if (!open && draftOffer) void answerDraft(false);
        }}
        headerText={draftTexts.RECOVER_LAUNCH_TITLE}
        width={480}
        data-testid="erp-draft-offer"
        renderBody={() => (
          <p className="cgs-dialog-message" dir={draftTexts.dir} data-testid="erp-draft-text">
            {draftOffer
              ? draftTexts.recoverLaunchText(
                  draftOffer.entry.erp!.label,
                  draftOffer.entry.erp!.company,
                )
              : ''}
          </p>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions" dir={draftTexts.dir}>
            <CgButton
              intent="primary"
              data-testid="erp-draft-recover"
              onClick={() => void answerDraft(true)}
            >
              {draftTexts.RECOVER_CHANGES}
            </CgButton>
            <CgButton data-testid="erp-draft-afresh" onClick={() => void answerDraft(false)}>
              {draftTexts.OPEN_AFRESH}
            </CgButton>
          </div>
        )}
      />
      <ReviewChangesDialog
        open={reviewOpen && Boolean(binding)}
        review={review}
        noun={noun}
        t={t}
        busy={busy}
        canCommit={!expired && !disconnected && Boolean(binding?.counts.changed)}
        onClose={() => setReviewOpen(false)}
        onGoTo={goTo}
        onDiscard={(entries) => void discardEntries(entries)}
        onCommit={() => void commit(false)}
      />
      <ResolveConflictsDialog
        open={resolveOpen && Boolean(binding)}
        list={conflicts}
        t={t}
        noun={noun}
        busy={busy}
        onClose={() => setResolveOpen(false)}
        onResolve={(resolution) => void resolve(resolution)}
        onGoTo={(entry: ErpConflictEntry) => goTo(entry)}
      />
      <CgWindow
        open={confirm !== null}
        onOpenChange={(open) => {
          if (!open) setConfirm(null);
        }}
        headerText={t.COMMIT_TITLE}
        width={480}
        data-testid="erp-commit-confirm"
        renderBody={() => (
          <div className="cgs-erp-lines" data-testid="erp-commit-text" dir={t.dir}>
            {confirm?.lines.map((line) => (
              <p key={line}>{line}</p>
            ))}
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions" dir={t.dir}>
            <CgButton
              intent="primary"
              data-testid="erp-commit-send"
              onClick={() => {
                if (confirm) void send(confirm.visible);
              }}
            >
              {t.commitButton(confirm?.changes ?? 0)}
            </CgButton>
            <CgButton
              data-testid="erp-commit-review"
              onClick={() => {
                setConfirm(null);
                void openReview();
              }}
            >
              {t.REVIEW}
            </CgButton>
            <CgButton data-testid="erp-commit-cancel" onClick={() => setConfirm(null)}>
              {t.CANCEL}
            </CgButton>
          </div>
        )}
      />
      <CgWindow
        open={commitPanel !== null}
        onOpenChange={(open) => {
          if (open || !commitPanel || commitPanel.phase === 'committing') return;
          if (commitPanel.phase === 'unknown') void stopCommitWait();
          else setCommitPanel(null);
        }}
        headerText={t.COMMIT_TITLE}
        width={480}
        data-testid="erp-commit-panel"
        renderBody={() => (
          <div className="cgs-dialog-column" dir={t.dir}>
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
            <div className="cgs-dialog-actions" dir={t.dir}>
              {commitPanel?.phase === 'unknown' && commitPanel.reconnect && (
                <CgButton
                  intent="primary"
                  data-testid="erp-commit-reconnect"
                  onClick={() => {
                    setReconnectOpen(true);
                    waitForLaunch();
                  }}
                >
                  {t.RECONNECT}
                </CgButton>
              )}
              {commitPanel?.phase === 'unknown' && !commitPanel.reconnect && (
                <CgButton
                  intent="primary"
                  data-testid="erp-commit-retry"
                  disabled={busy}
                  onClick={() => void retry()}
                >
                  {t.TRY_AGAIN}
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
                  {t.NEXT_ERROR}
                </CgButton>
              )}
              {commitPanel?.phase === 'refused' && commitPanel.resolve && (
                <CgButton
                  intent="primary"
                  data-testid="erp-commit-resolve"
                  onClick={() => void openResolve()}
                >
                  {t.RESOLVE}
                </CgButton>
              )}
              <CgButton
                data-testid="erp-commit-close"
                onClick={() => {
                  if (commitPanel?.phase === 'unknown') void stopCommitWait();
                  else setCommitPanel(null);
                }}
              >
                {t.CLOSE}
              </CgButton>
            </div>
          )
        }
      />
      <CgWindow
        open={refusal !== null}
        onOpenChange={(open) => {
          if (!open) setRefusal(null);
        }}
        headerText={t.COMMIT_TITLE}
        width={440}
        data-testid="erp-commit-refused"
        renderBody={() => (
          <p className="cgs-dialog-message" data-testid="erp-commit-refused-text" dir={t.dir}>
            {refusal?.text}
          </p>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions" dir={t.dir}>
            {refusal?.nextError && (
              <CgButton
                data-testid="erp-commit-refused-next"
                onClick={() => {
                  setRefusal(null);
                  void nextError();
                }}
              >
                {t.NEXT_ERROR}
              </CgButton>
            )}
            {refusal?.resolve && (
              <CgButton data-testid="erp-commit-refused-resolve" onClick={() => void openResolve()}>
                {t.RESOLVE}
              </CgButton>
            )}
            <CgButton
              intent="primary"
              data-testid="erp-commit-refused-close"
              onClick={() => setRefusal(null)}
            >
              {t.OK}
            </CgButton>
          </div>
        )}
      />
      <CgWindow
        open={reconnectOpen && Boolean(binding)}
        onOpenChange={(open) => {
          if (!open) setReconnectOpen(false);
        }}
        headerText={t.RECONNECT_TITLE}
        width={480}
        data-testid="erp-reconnect"
        renderBody={() => (
          <div className="cgs-dialog-column" dir={t.dir}>
            <p className="cgs-dialog-message" data-testid="erp-reconnect-text">
              {t.reconnectText(noun)}
            </p>
            <p className="cgs-dialog-message" role="status" data-testid="erp-reconnect-status">
              {t.WAITING}
            </p>
            <CgProgressBar data-testid="erp-reconnect-progress" />
          </div>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions" dir={t.dir}>
            <CgButton
              intent="primary"
              data-testid="erp-reconnect-close"
              onClick={() => setReconnectOpen(false)}
            >
              {t.CLOSE}
            </CgButton>
          </div>
        )}
      />
      <CgWindow
        open={infoOpen && Boolean(binding)}
        onOpenChange={(open) => {
          if (!open) setInfoOpen(false);
        }}
        headerText={t.INFO_TITLE}
        width={460}
        allowDrag
        data-testid="erp-info"
        renderBody={() => (
          <dl className="cgs-erp-info" dir={t.dir}>
            {infoRows.map(([id, term, value]) => (
              <div key={id} className="cgs-erp-info-row">
                <dt>{term}</dt>
                <dd data-testid={`erp-info-${id}`}>{value}</dd>
              </div>
            ))}
          </dl>
        )}
        renderFooter={() => (
          <div className="cgs-dialog-actions" dir={t.dir}>
            <CgButton
              intent="primary"
              data-testid="erp-info-close"
              onClick={() => setInfoOpen(false)}
            >
              {t.CLOSE}
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
    launching: () => launchingRef.current || draftOffer !== null,
    sync,
    readOnlyAt,
    hoverDetails,
    localize: (message) => tRef.current.localized(message),
    recovered,
    textsFor,
    bindStaged,
    prepare: (session: SessionInfo, schema: EntitySchema) =>
      prepareFor(hostRef.current, session, schema),
  };
}
