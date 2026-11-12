import { describe, expect, it } from 'vitest';
import type { CloudReason, CloudStatus } from '../platform';
import { AR, cloudTextsFor, EN } from './texts';

const REASONS: CloudReason[] = [
  'ticket',
  'session',
  'origin',
  'denied',
  'gone',
  'conflict',
  'tooLarge',
  'invalid',
  'busy',
  'server',
  'readUrl',
  'network',
  'contract',
  'cancelled',
  'handoverRefused',
];

const STATUSES: CloudStatus[] = [
  { kind: 'opening', name: null, received: 0, total: null },
  { kind: 'open', name: 'Budget', access: 'edit', savedAt: null },
  { kind: 'open', name: 'Budget', access: 'view', savedAt: Date.UTC(2026, 10, 12, 9, 5) },
  { kind: 'failed', reason: 'ticket' },
  { kind: 'ended', name: 'Budget' },
  { kind: 'waiting', name: 'Budget' },
  { kind: 'handedOver', name: 'Budget' },
];

describe('the cloud texts', () => {
  it('follow the Workspace user’s language, English for any other', () => {
    expect(cloudTextsFor('ar')).toBe(AR);
    expect(cloudTextsFor('en')).toBe(EN);
    expect(cloudTextsFor('fr')).toBe(EN);
    expect(cloudTextsFor(undefined)).toBe(EN);
    expect([EN.dir, AR.dir]).toEqual(['ltr', 'rtl']);
  });

  it('have the same entries in both languages, none empty', () => {
    expect(Object.keys(AR).sort()).toEqual(Object.keys(EN).sort());
    for (const texts of [EN, AR]) {
      for (const [key, value] of Object.entries(texts)) {
        if (typeof value === 'string') expect(value, key).not.toBe('');
      }
      for (const reason of REASONS) {
        expect(texts.failedText(reason)).not.toBe('');
        expect(texts.saveFailedText(reason)).not.toBe('');
      }
      for (const status of STATUSES) expect(texts.badgeText(status)).toBeTruthy();
      expect(texts.badgeText({ kind: 'closed' })).toBeNull();
    }
  });

  it('isolate the names they carry, and say who saved a newer version', () => {
    for (const texts of [EN, AR]) {
      expect(texts.conflictText('Budget', 'Omar', '2026-11-12T10:41:07Z')).toContain('⁨Omar⁩');
      expect(texts.viewOnlyText('ميزانية')).toContain('⁨ميزانية⁩');
    }
    expect(EN.badgeText(STATUSES[2]!)).toMatch(/View only · Saved \d/);
    expect(AR.badgeText(STATUSES[2]!)).toContain('للعرض فقط');
    expect(EN.openingText('Budget', 50, 200)).toContain('25%');
  });
});
