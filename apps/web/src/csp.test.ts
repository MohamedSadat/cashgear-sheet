import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { contentSecurityPolicy, LIVE_APIS, LIVE_WORKSPACE_API } from './csp';
import { LIVE_WORKSPACE_API as CONNECTOR_LIVE } from '@cgs/workspace-connector';

const config = JSON.parse(
  readFileSync(new URL('../public/staticwebapp.config.json', import.meta.url), 'utf8'),
) as { globalHeaders: Record<string, string> };

describe('the live site’s headers', () => {
  it('carry the content security policy, with the live APIs the only other origins', () => {
    expect(config.globalHeaders['Content-Security-Policy']).toBe(contentSecurityPolicy(LIVE_APIS));
    expect(LIVE_WORKSPACE_API).toBe(CONNECTOR_LIVE);
    expect(config.globalHeaders['X-Content-Type-Options']).toBe('nosniff');
  });

  it('keep the page cross-origin isolated', () => {
    expect(config.globalHeaders).toMatchObject({
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
      'Cross-Origin-Resource-Policy': 'same-origin',
    });
  });

  it('let no script but the app’s own run, and never frame the page', () => {
    const policy = contentSecurityPolicy(LIVE_APIS);
    expect(policy).toContain("script-src 'self' 'wasm-unsafe-eval'");
    expect(policy).not.toMatch(/ 'unsafe-eval'|script-src[^;]*unsafe-inline/);
    expect(policy).toContain("frame-ancestors 'none'");
  });
});
