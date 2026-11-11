import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { contentSecurityPolicy, LIVE_API } from './csp';

const config = JSON.parse(
  readFileSync(new URL('../public/staticwebapp.config.json', import.meta.url), 'utf8'),
) as { globalHeaders: Record<string, string> };

describe('the live site’s headers', () => {
  it('carry the content security policy, with the live API the only other origin', () => {
    expect(config.globalHeaders['Content-Security-Policy']).toBe(contentSecurityPolicy([LIVE_API]));
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
    const policy = contentSecurityPolicy([LIVE_API]);
    expect(policy).toContain("script-src 'self' 'wasm-unsafe-eval'");
    expect(policy).not.toMatch(/ 'unsafe-eval'|script-src[^;]*unsafe-inline/);
    expect(policy).toContain("frame-ancestors 'none'");
  });
});
