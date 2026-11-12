import { describe, expect, it } from 'vitest';
import {
  LIVE_WORKSPACE_API,
  MOCK_WORKSPACE_API,
  onCloudPath,
  readCloudLaunch,
  workspaceApiBase,
  type LaunchHistory,
} from './launch.ts';

const TICKET = 'aB3_-'.repeat(8) + 'xyz';

function history() {
  const calls: string[] = [];
  const value: LaunchHistory = {
    state: { kept: true },
    replaceState(_data, _unused, url) {
      calls.push(String(url));
    },
  };
  return { value, calls };
}

describe('the /cloud launch', () => {
  it('reads the ticket and removes it from the address bar and history at once', () => {
    const h = history();
    const launch = readCloudLaunch(
      { pathname: '/cloud', search: '', hash: `#launch=${TICKET}` },
      h.value,
      'https://workspace-api.example.com/',
    );
    expect(launch).toEqual({ ticket: TICKET, apiBase: 'https://workspace-api.example.com' });
    expect(h.calls).toEqual(['/']);
  });

  it('keeps the query, and takes /cloud/ and any case', () => {
    const h = history();
    expect(
      readCloudLaunch(
        { pathname: '/Cloud/', search: '?x=1', hash: `#launch=${TICKET}` },
        h.value,
        'a',
      ),
    ).not.toBeNull();
    expect(h.calls).toEqual(['/?x=1']);
  });

  it('clears a malformed ticket too, and opens nothing', () => {
    for (const hash of [
      '#launch=short',
      '#launch=' + TICKET + 'A',
      '#launch=' + '%'.repeat(43),
      '',
    ]) {
      const h = history();
      expect(readCloudLaunch({ pathname: '/cloud', search: '', hash }, h.value, 'a')).toBeNull();
      expect(h.calls).toEqual(['/']);
    }
  });

  it('leaves every other path alone, the ERP’s /erp included', () => {
    const h = history();
    expect(
      readCloudLaunch({ pathname: '/erp', search: '', hash: `#launch=${TICKET}` }, h.value, 'a'),
    ).toBeNull();
    expect(readCloudLaunch({ pathname: '/', search: '', hash: '' }, h.value, 'a')).toBeNull();
    expect(h.calls).toEqual([]);
    expect(onCloudPath('/clouds')).toBe(false);
  });
});

describe('the Workspace API the sheet trusts', () => {
  it('is the build’s when it names one, the mock on localhost, else Workspace’s own', () => {
    expect(workspaceApiBase('https://sheet.cashgearerp.com', 'https://w.example.com/')).toBe(
      'https://w.example.com',
    );
    expect(workspaceApiBase('http://127.0.0.1:4173')).toBe(MOCK_WORKSPACE_API);
    expect(workspaceApiBase('http://localhost:5173')).toBe(MOCK_WORKSPACE_API);
    expect(workspaceApiBase('https://sheet.cashgearerp.com')).toBe(LIVE_WORKSPACE_API);
    expect(workspaceApiBase('https://localhost.evil.example')).toBe(LIVE_WORKSPACE_API);
  });
});
