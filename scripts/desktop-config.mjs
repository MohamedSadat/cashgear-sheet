// What a desktop build changes in tauri.conf.json for where it runs (Track S6): the scheme a test
// product registers, and Cash Gear Workspace's origins for a build that talks to another API.

/** Whether `text` is exactly an origin, as `crates/cgs-native/src/csp.rs` checks one. */
export function isOrigin(text) {
  return /^https?:\/\/(\[::1\]|[A-Za-z0-9][A-Za-z0-9.-]*)(:\d{1,5})?$/.test(text);
}

/** `policy` with `origins` added to its `connect-src`. Throws on anything that isn't an origin. */
export function widenConnectSrc(policy, origins) {
  for (const origin of origins) {
    if (!isOrigin(origin)) throw new Error(`Not an origin: ${origin}`);
  }
  let found = false;
  const widened = policy
    .split(';')
    .map((directive) => directive.trim())
    .filter(Boolean)
    .map((directive) => {
      const sources = directive.split(/\s+/);
      if (sources[0] !== 'connect-src') return directive;
      found = true;
      return [...sources, ...origins.filter((origin) => !sources.includes(origin))].join(' ');
    })
    .join('; ');
  if (!found) throw new Error('The policy has no connect-src.');
  return widened;
}

/**
 * The origins a build must reach beyond the shipped policy's: the Workspace API and Blob Storage
 * the web build was made for (`VITE_WORKSPACE_API_URL`, `VITE_WORKSPACE_BLOB_URL`), when they are
 * set, as `pnpm preview` adds them to its policy.
 */
export function workspaceOrigins(env) {
  return [env.VITE_WORKSPACE_API_URL, env.VITE_WORKSPACE_BLOB_URL]
    .filter(Boolean)
    .map((url) => new URL(url).origin);
}
