import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  contentSecurityPolicy,
  isEmbedPath,
  LIVE_APIS,
  LIVE_FRAME_PARENTS,
  LIVE_WORKSPACE_API,
  LOCAL_FRAME_PARENTS,
} from './csp';
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

describe('the embed’s headers (Track S3)', () => {
  const routes = (
    JSON.parse(
      readFileSync(new URL('../public/staticwebapp.config.json', import.meta.url), 'utf8'),
    ) as { routes: { route: string; rewrite?: string; headers?: Record<string, string> }[] }
  ).routes;

  it('let Workspace’s live origins frame /embed, and nothing else', () => {
    const policy = contentSecurityPolicy(LIVE_APIS, LIVE_FRAME_PARENTS);
    expect(policy).toContain(
      'frame-ancestors https://workspace.cashgearerp.com tauri://localhost http://tauri.localhost',
    );
    for (const path of ['/embed', '/embed.html']) {
      const route = routes.find((r) => r.route === path);
      expect(route?.headers?.['Content-Security-Policy']).toBe(policy);
    }
    expect(routes.find((r) => r.route === '/embed')?.rewrite).toBe('/embed.html');
  });

  it('keep the rest of the site unframed, and the embed otherwise as strict', () => {
    const app = contentSecurityPolicy(LIVE_APIS);
    const embed = contentSecurityPolicy(LIVE_APIS, LIVE_FRAME_PARENTS);
    expect(app).toContain("frame-ancestors 'none'");
    expect(embed.replace(/frame-ancestors [^;]*/, '')).toBe(
      app.replace(/frame-ancestors [^;]*/, ''),
    );
    expect(LOCAL_FRAME_PARENTS.every((origin) => !embed.includes(origin))).toBe(true);
  });

  it('serve /embed’s page for /embed and its file only', () => {
    expect(isEmbedPath('/embed')).toBe(true);
    expect(isEmbedPath('/embed.html')).toBe(true);
    expect(isEmbedPath('/embed?x=1')).toBe(true);
    expect(isEmbedPath('/')).toBe(false);
    expect(isEmbedPath('/cloud')).toBe(false);
    expect(isEmbedPath('/embed/x')).toBe(false);
    expect(isEmbedPath('/embedded')).toBe(false);
  });
});
