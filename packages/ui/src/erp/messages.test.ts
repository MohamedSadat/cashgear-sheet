// What the ERP features say (Phases 16 and 17): the badge, hover details, and Validate, Review and
// Commit's wording as decided on 2026-11-10.
import { describe, expect, it } from 'vitest';
import type { ErpCellInfo, ErpReviewEntry } from '@cgs/engine-client';
import {
  badgeText,
  batchErrorText,
  cellTip,
  committedText,
  confirmLines,
  conflictText409,
  conflictsWaitText,
  localErrorsText,
  refusedText,
  reviewMessage,
  serverText,
  singular,
  statusText,
  tooManyText,
  validatedText,
} from './messages';

const plain = (text: string) => text.replace(/[⁦-⁩]/g, '');

describe('the ERP’s wording', () => {
  it('counts conflicts on the badge', () => {
    expect(
      plain(
        badgeText(
          'Vendors',
          1240,
          0,
          false,
          'Sara',
          'Kit',
          {
            changed: 12,
            changedRecords: 9,
            invalid: 2,
            formulas: 0,
            conflicts: 1,
          },
          true,
        ),
      ),
    ).toBe('Vendors · 1,240 · 12 changed · 2 invalid · 1 conflict · Sara @ Kit');
  });

  it('confirms a commit with its counts, hidden rows and left-out formulas', () => {
    expect(
      confirmLines('vendors', 'Kit', { changes: 37, records: 12, hidden: 3, formulas: 2 }).map(
        plain,
      ),
    ).toEqual([
      'Commit 37 changes to 12 vendors in Cash Gear ERP (Kit)?',
      "Cash Gear ERP checks them again and saves all of them or none. Undo is cleared once they're saved.",
      '3 of these changes are in rows the filter hides.',
      '2 formulas are left out: convert them to values to commit them.',
    ]);
    expect(
      confirmLines('vendors', 'Kit', { changes: 1, records: 1, hidden: 1, formulas: 1 }),
    ).toEqual([
      'Commit 1 change to 1 vendor in Cash Gear ERP (⁨Kit⁩)?',
      "Cash Gear ERP checks them again and saves all of them or none. Undo is cleared once they're saved.",
      '1 of these changes is in a row the filter hides.',
      '1 formula is left out: convert it to a value to commit it.',
    ]);
  });

  it('refuses before sending, past the limit, with local errors or conflicts', () => {
    expect(tooManyText(612, 500, 'vendors', false)).toBe(
      '612 vendors have changes, and Cash Gear ERP takes at most 500 in one commit. Filter the table to fewer of them, then choose Commit Changes in Visible Rows.',
    );
    expect(tooManyText(501, 500, 'vendors', true)).toContain('501 vendors in the rows shown');
    expect(localErrorsText(1)).toBe(
      '1 cell breaks a rule, so nothing was sent. Fix it or discard that change, then commit.',
    );
    expect(localErrorsText(2)).toBe(
      '2 cells break a rule, so nothing was sent. Fix them or discard those changes, then commit.',
    );
    expect(conflictsWaitText(2, 'vendors')).toContain('2 vendors were changed in Cash Gear ERP');
  });

  it('says what Validate and Commit found', () => {
    const sent = { changes: 37, records: 12 };
    expect(
      validatedText('vendors', sent, { errors: 0, errorRecords: 0, conflicts: 0, batch: [] }),
    ).toBe('Cash Gear ERP checked 37 changes to 12 vendors and found no problems.');
    expect(
      validatedText('vendors', sent, { errors: 3, errorRecords: 2, conflicts: 1, batch: [] }),
    ).toBe(
      'Cash Gear ERP refuses 3 changes to 2 vendors, and 1 vendor changed in Cash Gear ERP since you opened it. The cells are marked.',
    );
    expect(refusedText('vendors', { errors: 1, errorRecords: 1 }, [])).toBe(
      'Cash Gear ERP refused 1 change to 1 vendor, so nothing was saved. The cells are marked; Next Error goes through them.',
    );
    expect(refusedText('vendors', { errors: 0, errorRecords: 0 }, ['Schema text.'])).toBe(
      'Cash Gear ERP refused the commit, so nothing was saved. Schema text.',
    );
    expect(conflictText409(1, 'vendors')).toMatch(/^1 vendor was changed/);
    expect(committedText(37, 12, 'vendors', 0)).toBe('Committed 37 changes to 12 vendors.');
    expect(committedText(37, 12, 'vendors', 1)).toBe(
      'Committed 37 changes to 12 vendors. 1 change made since stays in the sheet.',
    );
    expect(batchErrorText('schema.changed', 'x', 'vendors')).toContain('have changed since');
    expect(batchErrorText('whatever', 'The ERP’s own words.', 'vendors')).toBe(
      'The ERP’s own words.',
    );
    expect(singular('vendors')).toBe('vendor');
    expect(singular('companies')).toBe('company');
  });

  it('words the ERP’s errors by code, with its message as the fallback', () => {
    expect(
      plain(
        serverText('Payment method', { code: 'lookup.notFound', message: 'x' }, 'vendors', 'NOPE'),
      ),
    ).toBe("NOPE isn't in Cash Gear ERP's list for Payment method.");
    expect(serverText('Address', { code: 'record.locked', message: 'x' }, 'vendors')).toBe(
      'This vendor is locked in Cash Gear ERP.',
    );
    expect(serverText('Address', { code: 'new.code', message: 'طريقة' }, 'vendors')).toBe('طريقة');
  });

  it('explains a cell in conflict, refused by the ERP, or committed', () => {
    const info: ErpCellInfo = {
      field: 'mobile',
      label: 'Mobile',
      heading: false,
      editable: true,
      key: 'V-1006',
      state: 'conflict',
      original: '0100',
      now: '0122',
      conflict: { by: 'omar', at: null, now: '0111', known: true },
    };
    expect(plain(cellTip(info, 'vendors')!.text)).toBe(
      'Original 0100 → New 0122\nChanged in Cash Gear ERP by omar · now 0111',
    );
    expect(
      plain(
        cellTip(
          {
            ...info,
            state: 'invalid',
            conflict: undefined,
            server: { code: 'record.locked', message: '' },
          },
          'vendors',
        )!.text,
      ),
    ).toBe('Original 0100 → New 0122\nThis vendor is locked in Cash Gear ERP.');
    expect(cellTip({ ...info, state: 'committed', conflict: undefined }, 'vendors')!.text).toBe(
      'Committed to Cash Gear ERP.',
    );
  });

  it('gives each reviewed field a status and a message', () => {
    const entry: ErpReviewEntry = {
      key: 'V-1001',
      name: 'Al Noor',
      row: 2,
      col: 4,
      field: 'mobile',
      label: 'Mobile',
      original: '0100',
      now: '=A1',
      state: 'formula',
      invalid: false,
      conflict: false,
      hidden: true,
    };
    expect(statusText(entry)).toBe('Formula, not committed');
    expect(reviewMessage(entry, 'vendors')).toBe(
      'Convert it to a value to commit it. In a row the filter hides.',
    );
    expect(statusText({ ...entry, state: 'modified' })).toBe('Changed');
  });
});
