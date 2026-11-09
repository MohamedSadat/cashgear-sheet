// Opening and refreshing ERP records: Redeem, then the schema, then the pages, each page's bytes
// handed to the engine unread (plan B.3, item 4). The connector holds no cells and builds no
// record objects; the engine stages and binds them.

import { ENTITY_ROUTES, type EntitySchema } from './contract.ts';
import { ErpError, ErpHttp } from './http.ts';
import { ErpSession } from './session.ts';
import { readEntitySchema } from './validate.ts';

/** What one page added, as the engine answers after reading it. */
export interface StagedPage {
  readonly records: number;
  readonly staged: number;
  readonly nextAfterId: number | null;
  readonly total: number;
}

/** Where record pages go: the engine worker, which reads them from bytes. */
export interface RecordSink {
  /** A new binding's staging, against the schema as JSON. */
  begin(schema: string): Promise<void>;
  /** A refresh's staging, against the binding's own schema. */
  beginRefresh(): Promise<void>;
  /** One page's bytes, which the sink may take (transfer). */
  page(bytes: Uint8Array): Promise<StagedPage>;
  cancel(): Promise<void>;
}

export interface LoadProgress {
  readonly stage: 'connecting' | 'loading';
  readonly loaded: number;
  /** Known once the first page has come. */
  readonly total: number | null;
}

export interface OpenOptions {
  readonly http: ErpHttp;
  readonly ticket: string;
  readonly sink: RecordSink;
  readonly onProgress?: (progress: LoadProgress) => void;
  readonly signal?: AbortSignal;
}

export interface Opened {
  readonly session: ErpSession;
  readonly schema: EntitySchema;
  readonly loaded: number;
}

function asSinkError(error: unknown): ErpError {
  if (error instanceof ErpError) return error;
  // The engine's own words, such as the 10,000-record cap's.
  return new ErpError('contract', error instanceof Error ? error.message : String(error));
}

/** Reads every page of a query into the sink. */
async function pages(
  session: ErpSession,
  sink: RecordSink,
  request: { filter: unknown; ids: readonly number[] | null },
  pageSize: number,
  report: (loaded: number, total: number) => void,
  signal?: AbortSignal,
): Promise<number> {
  let afterId = 0;
  for (;;) {
    const bytes = await session.bytes('POST', session.route(ENTITY_ROUTES.query), {
      body: { filter: request.filter, ids: request.ids, afterId, pageSize },
      signal,
      idempotent: true,
    });
    if (signal?.aborted) throw new ErpError('cancelled', 'Cancelled.');
    let page: StagedPage;
    try {
      page = await sink.page(bytes);
    } catch (error) {
      throw asSinkError(error);
    }
    report(page.staged, page.total);
    if (page.nextAfterId === null) return page.staged;
    afterId = page.nextAfterId;
  }
}

/**
 * Redeems the launch ticket and stages every record its list holds. The sink then binds them into
 * a new workbook. Cancelling drops what was staged.
 */
export async function openRecords(options: OpenOptions): Promise<Opened> {
  const { http, ticket, sink, onProgress, signal } = options;
  onProgress?.({ stage: 'connecting', loaded: 0, total: null });
  try {
    const session = await ErpSession.redeem(http, ticket, { signal });
    const answer = await session.json('GET', session.route(ENTITY_ROUTES.schema), {
      signal,
      idempotent: true,
    });
    let schema: EntitySchema;
    try {
      schema = readEntitySchema(answer);
    } catch {
      throw new ErpError('contract', 'Cash Gear ERP sent a schema the sheet can’t read.');
    }
    try {
      await sink.begin(JSON.stringify(schema));
    } catch (error) {
      throw asSinkError(error);
    }
    onProgress?.({ stage: 'loading', loaded: 0, total: null });
    const loaded = await pages(
      session,
      sink,
      { filter: session.filter, ids: null },
      schema.maxPageSize,
      (count, total) => onProgress?.({ stage: 'loading', loaded: count, total }),
      signal,
    );
    return { session, schema, loaded };
  } catch (error) {
    await sink.cancel().catch(() => undefined);
    throw error;
  }
}

export interface RefreshOptions {
  readonly session: ErpSession;
  readonly sink: RecordSink;
  /** The bound records' ids, ascending. */
  readonly ids: readonly number[];
  readonly pageSize: number;
  readonly onProgress?: (progress: LoadProgress) => void;
  readonly signal?: AbortSignal;
}

/** Stages the bound records again by id, whatever the original filter now matches. */
export async function refreshRecords(options: RefreshOptions): Promise<number> {
  const { session, sink, ids, pageSize, onProgress, signal } = options;
  try {
    await sink.beginRefresh();
    onProgress?.({ stage: 'loading', loaded: 0, total: ids.length });
    return await pages(
      session,
      sink,
      { filter: null, ids },
      pageSize,
      (count) => onProgress?.({ stage: 'loading', loaded: count, total: ids.length }),
      signal,
    );
  } catch (error) {
    await sink.cancel().catch(() => undefined);
    throw error instanceof ErpError ? error : asSinkError(error);
  }
}
