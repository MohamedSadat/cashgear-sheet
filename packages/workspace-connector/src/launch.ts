// Workspace's launch ticket arrives in the URL's fragment, which browsers never send to a server
// (`https://sheet.cashgearerp.com/cloud#launch=<ticket>`). It is read and removed from the address
// bar and from history before anything else runs, so it leaves no trace, as the ERP's launch is.

/** What Workspace's "Open in Cash Gear Sheet" opened the sheet with. */
export interface CloudLaunch {
  readonly ticket: string;
  /** Workspace API's origin the ticket belongs to. Configured, never taken from the link. */
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

/** 256 bits in base64url without padding. */
const TICKET = /^[A-Za-z0-9_-]{43}$/;

/** Whether a page's path is the cloud launch route, `/cloud`. */
export const onCloudPath = (pathname: string): boolean => /^\/cloud\/?$/i.test(pathname);

/**
 * On `/cloud`, reads `#launch=<ticket>` and removes the fragment from the address bar and the
 * current history entry, whether or not the ticket is well formed; `/cloud` becomes `/`, the
 * app's own page. Any other path is left alone (the ERP's `/erp` launch reads its own).
 */
export function readCloudLaunch(
  location: LaunchLocation,
  history: LaunchHistory,
  apiBase: string,
): CloudLaunch | null {
  if (!onCloudPath(location.pathname)) return null;
  const parameters = new URLSearchParams(location.hash.replace(/^#/, ''));
  const ticket = parameters.get('launch');
  history.replaceState(history.state, '', `/${location.search}`);
  if (ticket === null || !TICKET.test(ticket)) return null;
  return { ticket, apiBase: apiBase.replace(/\/+$/, '') };
}

/** Workspace's live API (its ARCHITECTURE.md). */
export const LIVE_WORKSPACE_API = 'https://workspace-api.cashgearerp.com';
/** The mock on this machine (`pnpm workspace:mock`), with its blob server one port up. */
export const MOCK_WORKSPACE_API = 'http://127.0.0.1:4280';

/**
 * Workspace API's origin for this page: `configured` when the build names one, the mock on this
 * machine when the page itself is served from it, or Cash Gear Workspace's own.
 */
export function workspaceApiBase(pageOrigin: string, configured?: string): string {
  if (configured) return configured.replace(/\/+$/, '');
  const local = /^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/i.test(pageOrigin);
  return local ? MOCK_WORKSPACE_API : LIVE_WORKSPACE_API;
}
