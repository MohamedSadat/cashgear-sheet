export interface MockErpOptions {
  /** 0, the default, picks a free port. */
  port?: number;
  host?: string;
  /** Origins allowed to redeem tickets and read responses. */
  origins?: string[];
  /** Where ticket URLs point, without the `/erp` path. */
  sheetUrl?: string;
}

export interface MockErpServer {
  readonly url: string;
  readonly port: number;
  readonly mock: unknown;
  close(): Promise<void>;
}

export function startMockErp(options?: MockErpOptions): Promise<MockErpServer>;

/** A server that redirects every request to another, which notes each request's session header. */
export function startRedirectPair(): Promise<{
  readonly url: string;
  readonly seen: (string | null)[];
  close(): Promise<void>;
}>;

export const LIMITS: {
  readonly ticketMs: number;
  readonly idleMs: number;
  readonly maxMs: number;
  readonly maxPageSize: number;
  readonly maxRecords: number;
  readonly maxCommitRecords: number;
};
export const DEFAULT_ORIGINS: readonly string[];
export const ENTITY_ROUTE: string;
export const SCHEMA_VERSION: string;
