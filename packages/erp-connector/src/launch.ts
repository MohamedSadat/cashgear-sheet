// The launch ticket arrives in the URL's fragment, which browsers never send to a server
// (`https://sheet.cashgearerp.com/erp#launch=<ticket>`, plan B.4). It is read and removed from
// the address bar and from history before anything else runs, so it leaves no trace.

/** What the ERP's button opened the sheet with. */
export interface ErpLaunch {
  readonly ticket: string;
  /** The ERP API's origin the ticket belongs to. Configured, never taken from the link. */
  readonly apiBase: string;
}

/** The parts of `window.location` and `window.history` the launch needs. */
export interface LaunchLocation {
  readonly hash: string;
  readonly pathname: string;
  readonly search: string;
}

export interface LaunchHistory {
  readonly state: unknown;
  replaceState(data: unknown, unused: string, url?: string | URL | null): void;
}

/** 256 bits in base64url, as `CreateTicket` makes them. */
const TICKET = /^[A-Za-z0-9_-]{43}$/;

/**
 * Reads `#launch=<ticket>` and removes the fragment from the address bar and the current history
 * entry, whether or not the ticket is well formed. `/erp` becomes `/`, the app's own page.
 */
export function readErpLaunch(
  location: LaunchLocation,
  history: LaunchHistory,
  apiBase: string,
): ErpLaunch | null {
  const fragment = location.hash.replace(/^#/, '');
  const parameters = new URLSearchParams(fragment);
  const ticket = parameters.get('launch');
  const onErpPath = /^\/erp\/?$/i.test(location.pathname);
  if (ticket === null && !onErpPath) return null;
  const path = onErpPath ? '/' : location.pathname;
  history.replaceState(history.state, '', `${path}${location.search}`);
  if (ticket === null || !TICKET.test(ticket)) return null;
  return { ticket, apiBase: apiBase.replace(/\/+$/, '') };
}

/**
 * The ERP API's origin for this page: `configured` when the build names one, the mock on this
 * machine when the page itself is served from it (`pnpm erp:mock`), or Cash Gear ERP's own.
 */
export function erpApiBase(pageOrigin: string, configured?: string): string {
  if (configured) return configured.replace(/\/+$/, '');
  const local = /^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/i.test(pageOrigin);
  return local ? 'http://127.0.0.1:4180' : 'https://api.cashgearerp.com';
}
