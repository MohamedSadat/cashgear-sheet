// What the ERP features say (Phases 16 to 18): the badge, hover details, Validate, Review and
// Commit's wording as decided on 2026-11-10, and Phase 18's merge, Resolve, Reconnect and Arabic
// catalogue (2026-11-11).
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
  EN,
} from './messages';
import { AR, textsFor } from './texts';

const plain = (text: string) => text.replace(/[⁦-⁩]/g, '');

describe('the ERP’s wording', () => {
  it('counts conflicts on the badge', () => {
    expect(
      plain(
        badgeText(
          'Vendors',
          1240,
          { expired: false },
          'Sara',
          'Kit',
          {
            changed: 12,
            changedRecords: 9,
            invalid: 2,
            formulas: 0,
            conflicts: 1,
            gone: 2,
          },
          true,
        ),
      ),
    ).toBe('Vendors · 1,240 · 12 changed · 2 invalid · 1 conflict · 2 not in ERP · Sara @ Kit');
    expect(
      plain(
        badgeText('Vendors', 8, { expired: false, disconnected: true, noDraft: true }, 'S', 'K'),
      ),
    ).toBe('Vendors · 8 · Disconnected · No recovery copy · S @ K');
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
    expect(conflictsWaitText(2, 'vendors')).toBe(
      '2 vendors have fields you and Cash Gear ERP both changed. Choose Resolve Conflicts, then commit.',
    );
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
    expect(conflictText409({ conflicts: 1, merged: 0 }, 'vendors')).toMatch(
      /^1 vendor was changed/,
    );
    expect(conflictText409({ conflicts: 1, merged: 2 }, 'vendors')).toBe(
      '3 vendors were changed in Cash Gear ERP since you opened them, so nothing was saved. 2 vendors merged with your changes: commit again to send them. 1 vendor has fields you both changed: choose Resolve Conflicts, then commit again.',
    );
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

  it('says what a refresh merged and what Resolve did', () => {
    expect(
      EN.refreshedText('vendors', {
        updated: 1240,
        changed: 3,
        merged: 2,
        conflicts: 1,
        missing: 0,
      }),
    ).toBe(
      'Refreshed 1,240 vendors. 3 changed in Cash Gear ERP: 2 merged with your changes, 1 in conflict.',
    );
    expect(
      EN.refreshedText('vendors', { updated: 8, changed: 0, merged: 0, conflicts: 0, missing: 1 }),
    ).toBe(
      "Refreshed 8 vendors. Nothing changed in Cash Gear ERP. 1 isn't in Cash Gear ERP any more.",
    );
    expect(EN.resolvedText({ fields: 3, records: 2, left: 1 }, 'vendors')).toBe(
      'Resolved 3 fields. 2 vendors are ready to commit. 1 vendor still has conflicts.',
    );
  });
});

describe('the ERP’s Arabic', () => {
  it('has every text the English catalogue has', () => {
    expect(Object.keys(AR).sort()).toEqual(Object.keys(EN).sort());
    for (const [name, value] of Object.entries(AR)) {
      if (typeof value === 'string' && !['language', 'dir'].includes(name)) {
        expect(value, name).not.toBe((EN as Record<string, unknown>)[name]);
        expect(value, name).toMatch(/[\u0600-\u06FF]/);
      }
    }
    expect(textsFor('ar')).toBe(AR);
    expect(textsFor('en')).toBe(EN);
    expect(textsFor(undefined)).toBe(EN);
  });

  it('counts with Arabic plural forms and Latin digits', () => {
    expect(AR.records(1, 'vendors')).toBe('مورد واحد');
    expect(AR.records(2, 'vendors')).toBe('موردان');
    expect(AR.records(3, 'vendors')).toBe('3 موردين');
    expect(AR.records(12, 'vendors')).toBe('12 موردًا');
    expect(AR.records(1240, 'vendors')).toBe('1,240 موردًا');
    expect(AR.records(100, 'vendors')).toBe('100 مورد');
    expect(AR.changes(37)).toBe('37 تغييرًا');
    // An entity with no words of its own counts records.
    expect(AR.records(5, 'items')).toBe('5 سجلات');
  });

  it('words the badge, a commit and a merge', () => {
    expect(
      plain(
        AR.badgeText('الموردون', 1240, { expired: true }, 'سارة', 'Kit', {
          changed: 12,
          changedRecords: 9,
          invalid: 0,
          formulas: 0,
          conflicts: 1,
          gone: 0,
        }),
      ),
    ).toBe('الموردون · 1,240 · معدّلة: 12 · تعارضات: 1 · انتهت الجلسة · سارة @ Kit');
    expect(
      AR.confirmLines('vendors', 'Kit', { changes: 37, records: 12, hidden: 0, formulas: 0 }).map(
        plain,
      )[0],
    ).toBe('اعتماد 37 تغييرًا على 12 موردًا في Cash Gear ERP (Kit)؟');
    expect(
      AR.refreshedText('vendors', { updated: 8, changed: 2, merged: 1, conflicts: 1, missing: 0 }),
    ).toBe('تم تحديث 8 موردين. عُدّل 2 في Cash Gear ERP: دُمج 1 مع تغييراتك، و1 في تعارض.');
  });

  it('puts the engine’s refusals and limits in Arabic', () => {
    expect(AR.localized('This column is read-only in Cash Gear ERP.')).toBe(
      'هذا العمود للقراءة فقط في Cash Gear ERP.',
    );
    expect(
      AR.localized(
        'This list has 12,400 vendors, and a sheet opens at most 10,000. Narrow the search in Cash Gear ERP and open it again.',
      ),
    ).toBe(
      'تضم هذه القائمة 12,400 مورد، وتفتح الورقة 10,000 على الأكثر. ضيّق البحث في Cash Gear ERP ثم افتحها مرة أخرى.',
    );
    expect(AR.localized('Something the sheet never said.')).toBe('Something the sheet never said.');
    expect(EN.localized('This column is read-only in Cash Gear ERP.')).toBe(
      'This column is read-only in Cash Gear ERP.',
    );
  });

  it('explains a cell gone from the ERP, and one in conflict', () => {
    const info: ErpCellInfo = {
      field: 'mobile',
      label: 'الجوال',
      heading: false,
      editable: true,
      key: 'V-1006',
      state: 'conflict',
      original: '0100',
      now: '0122',
      conflict: { by: 'omar', at: null, now: '0111', known: true },
    };
    expect(plain(AR.cellTip(info, 'vendors')!.text)).toBe(
      'الأصلية 0100 ← الجديدة 0122\nعُدّل في Cash Gear ERP بواسطة omar · الآن 0111',
    );
    expect(AR.cellTip({ ...info, state: 'gone' }, 'vendors')!.text).toBe(
      'هذا المورد لم يعد موجودًا في Cash Gear ERP، فلا يمكن اعتماد تغييراته.',
    );
    expect(EN.cellTip({ ...info, state: 'gone' }, 'vendors')!.text).toBe(
      "This vendor isn't in Cash Gear ERP any more, so its changes can't be committed.",
    );
  });
});
