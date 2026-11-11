// Which of a workbook's link addresses a click may open (Phase 19's security review). A file can
// hold any address; one with a `javascript:` or `data:` scheme would run in, or stand in for, the
// app's own origin, where ERP sessions live.

/** The schemes a link may open with: web and mail addresses, as the app says it follows. */
const OPENS = new Set(['http:', 'https:', 'mailto:']);

/** The address to open for a link, or null when its scheme isn't one a click may open. */
export function linkToOpen(address: string): string | null {
  let url: URL;
  try {
    url = new URL(address.trim());
  } catch {
    return null;
  }
  return OPENS.has(url.protocol) ? url.href : null;
}
