import { describe, expect, it } from 'vitest';
import { linkToOpen } from './links';

describe('linkToOpen', () => {
  it('opens web and mail addresses', () => {
    expect(linkToOpen('https://example.com/a?b=1')).toBe('https://example.com/a?b=1');
    expect(linkToOpen(' http://example.com ')).toBe('http://example.com/');
    expect(linkToOpen('mailto:sales@example.com')).toBe('mailto:sales@example.com');
  });

  it('opens nothing that could run or stand in for the app', () => {
    for (const address of [
      'javascript:alert(1)',
      ' JavaScript:alert(1)',
      'java\tscript:alert(1)',
      'data:text/html,<script>alert(1)</script>',
      'vbscript:msgbox(1)',
      'blob:https://sheet.cashgearerp.com/x',
      'file:///C:/secret.txt',
      'report.xlsx',
      '',
    ]) {
      expect(linkToOpen(address), address).toBeNull();
    }
  });
});
