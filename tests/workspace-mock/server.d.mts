export interface MockWorkspaceOptions {
  /** 0, the default, picks a free port. */
  port?: number;
  /** Blob Storage's port. 0, the default, picks a free one. */
  blobPort?: number;
  host?: string;
  /** Origins allowed to redeem web tickets and read answers. */
  origins?: string[];
  /** The desktop app's origins, the only ones a desktop ticket redeems from (Track S6). */
  desktopOrigins?: string[];
  /** Where launch URLs point, without the `/cloud` path. */
  sheetUrl?: string;
}

export interface MockWorkspaceServer {
  readonly url: string;
  readonly blobUrl: string;
  readonly port: number;
  readonly mock: unknown;
  close(): Promise<void>;
}

export function startMockWorkspace(options?: MockWorkspaceOptions): Promise<MockWorkspaceServer>;

/** The seeded budget's bytes. */
export function budgetBook(): Uint8Array;

export const LIMITS: {
  readonly ticketMs: number;
  readonly idleMs: number;
  readonly maxMs: number;
  readonly readUrlMs: number;
  readonly maxBytes: number;
};
export const DEFAULT_ORIGINS: readonly string[];
export const DESKTOP_ORIGINS: readonly string[];
export const DESKTOP_SCHEME: string;
export const XLSX_TYPE: string;
export const USERS: Readonly<
  Record<
    'editor' | 'viewer' | 'other' | 'arabic',
    { id: string; name: string; language: 'en' | 'ar' }
  >
>;
export const WORKBOOKS: Readonly<Record<'budget' | 'empty', string>>;
