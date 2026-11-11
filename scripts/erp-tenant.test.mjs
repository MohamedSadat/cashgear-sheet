import { describe, expect, it } from 'vitest';
import { parseEnvFile, redactor } from './erp-tenant.mjs';

describe('erp-tenant', () => {
  it('reads NAME=value lines, comments and quotes, and refuses names it does not take', () => {
    expect(
      parseEnvFile('# keys\r\nERP_TEST_KEY_EDITOR = abc123\n\nERP_TEST_KEY_ADV="q w"\n'),
    ).toEqual({ ERP_TEST_KEY_EDITOR: 'abc123', ERP_TEST_KEY_ADV: 'q w' });
    expect(() => parseEnvFile('PATH=x')).toThrow(/doesn't take/);
    expect(() => parseEnvFile('just text')).toThrow(/NAME=value/);
  });

  it('blanks every key out of what a run prints, even split across chunks', () => {
    const filter = redactor(['secret-key-1', 'other-key-22', undefined]);
    let out = filter.push('sent secret-');
    out += filter.push('key-1 and other-key-22\nnext secret-key-1');
    out += filter.end();
    expect(out).toBe('sent [key] and [key]\nnext [key]');
    expect(out).not.toContain('secret');
  });
});
