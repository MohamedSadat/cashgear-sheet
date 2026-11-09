import { describe, expect, it } from 'vitest';
import { ERROR_CODES, isErrorCode, type EntitySchema } from './contract.ts';
import {
  ContractError,
  isDecimal,
  isIsoDate,
  isIsoDateTime,
  isVersion,
  readCommitResponse,
  readCommitStatus,
  readConflictBody,
  readCreateTicketResponse,
  readEntitySchema,
  readErrorBody,
  readLaunchStatus,
  readQueryResponse,
  readSheetSession,
  recordProblems,
} from './validate.ts';

const label = { en: 'Name', ar: 'الاسم' };

function schema(fields: unknown[] = [], extra: Record<string, unknown> = {}): unknown {
  return {
    entity: 'vendors',
    schemaVersion: 'vendors.1',
    label: { en: 'Vendors', ar: 'الموردون' },
    keyField: 'vendorAccount',
    idField: 'recId',
    maxPageSize: 1000,
    maxRecords: 10000,
    maxCommitRecords: 500,
    fields: [
      { name: 'vendorAccount', label, type: 'text', editable: false, editReason: 'notEditable' },
      ...fields,
    ],
    lookups: { paymentModes: [{ code: 'CASH', label }] },
    ...extra,
  };
}

const fails = (read: () => unknown, path: string) => {
  try {
    read();
  } catch (error) {
    expect(error).toBeInstanceOf(ContractError);
    expect((error as ContractError).path).toBe(path);
    return;
  }
  throw new Error(`expected a ContractError at ${path}`);
};

describe('wire formats', () => {
  it('decimals keep their scale and refuse anything that isn’t a plain decimal', () => {
    expect(isDecimal('15000.00', 2)).toBe(true);
    expect(isDecimal('-3', 2)).toBe(true);
    expect(isDecimal('0.5', 2)).toBe(true);
    for (const bad of ['1.234', '01', '1e3', '', '.5', '5.', '+1', '1,000'])
      expect(isDecimal(bad, 2), bad).toBe(false);
  });

  it('dates must exist, and instants need a zone', () => {
    expect(isIsoDate('2024-02-29')).toBe(true);
    expect(isIsoDate('2026-02-29')).toBe(false);
    expect(isIsoDate('2026-1-09')).toBe(false);
    expect(isIsoDateTime('2026-11-09T10:00:05Z')).toBe(true);
    expect(isIsoDateTime('2026-11-09T10:00:05.1234567+02:00')).toBe(true);
    expect(isIsoDateTime('2026-11-09T10:00:05')).toBe(false);
    expect(isIsoDateTime('2026-11-09')).toBe(false);
  });

  it('versions are 8 bytes of base64', () => {
    expect(isVersion('AAAAAAAAB9E=')).toBe(true);
    expect(isVersion('AAAAAAAAB9E')).toBe(false);
    expect(isVersion('AAAAAAAAAAAAB9E=')).toBe(false);
  });

  it('error codes are a fixed list', () => {
    expect(ERROR_CODES.length).toBe(new Set(ERROR_CODES).size);
    expect(isErrorCode('version.conflict')).toBe(true);
    expect(isErrorCode('version.Conflict')).toBe(false);
  });
});

describe('launch guards', () => {
  const ticket = 'k3J'.padEnd(43, 'x');

  it('need a 256-bit ticket that the URL carries in its fragment', () => {
    const ok = {
      ticket,
      expiresAt: '2026-11-09T10:02:00Z',
      url: `https://sheet.cashgearerp.com/erp#launch=${ticket}`,
    };
    expect(readCreateTicketResponse(ok).ticket).toBe(ticket);
    fails(() => readCreateTicketResponse({ ...ok, ticket: 'short' }), '$.ticket');
    fails(
      () =>
        readCreateTicketResponse({
          ...ok,
          url: `https://sheet.cashgearerp.com/erp?launch=${ticket}`,
        }),
      '$.url',
    );
  });

  it('read whether the ERP shows its button', () => {
    expect(readLaunchStatus({ enabled: false })).toEqual({ enabled: false });
    fails(() => readLaunchStatus({ enabled: 'yes' }), '$.enabled');
    fails(() => readLaunchStatus({}), '$.enabled');
  });

  it('need a route and a language in the session', () => {
    const session = {
      token: 't',
      idleExpiresAt: '2026-11-09T10:30:00Z',
      maxExpiresAt: '2026-11-09T20:00:00Z',
      company: 'Kit',
      userId: 'u17',
      userName: 'Sara',
      language: 'ar',
      entity: 'vendors',
      entityRoute: 'api/payable/VendorSheet',
      filter: { blocked: false },
    };
    expect(readSheetSession(session).company).toBe('Kit');
    fails(
      () => readSheetSession({ ...session, entityRoute: '/api/payable/VendorSheet' }),
      '$.entityRoute',
    );
    fails(() => readSheetSession({ ...session, language: 'fr' }), '$.language');
    fails(() => readSheetSession({ ...session, filter: { blocked: [] } }), '$.filter.blocked');
  });
});

describe('schema guard', () => {
  it('takes a valid schema', () => {
    const read = readEntitySchema(
      schema([
        {
          name: 'payMethod',
          label,
          type: 'lookup',
          lookup: 'paymentModes',
          required: true,
          editable: true,
        },
        {
          name: 'debitLimit',
          label,
          type: 'decimal',
          scale: 2,
          min: '0',
          editable: false,
          editReason: 'notEditable',
        },
      ]),
    );
    expect(read.fields.map((f) => f.name)).toEqual(['vendorAccount', 'payMethod', 'debitLimit']);
    expect(read.fields[2]).toMatchObject({ scale: 2, min: '0' });
  });

  it('refuses what the sheet couldn’t bind', () => {
    fails(
      () =>
        readEntitySchema(
          schema([{ name: 'x', label, type: 'text', editable: true, editReason: 'needsEdit' }]),
        ),
      '$.fields[1].editReason',
    );
    fails(
      () => readEntitySchema(schema([{ name: 'x', label, type: 'decimal', editable: false }])),
      '$.fields[1].scale',
    );
    fails(
      () =>
        readEntitySchema(
          schema([{ name: 'x', label, type: 'lookup', lookup: 'nope', editable: true }]),
        ),
      '$.fields[1].lookup',
    );
    fails(
      () => readEntitySchema(schema([{ name: 'x', label, type: 'money', editable: true }])),
      '$.fields[1].type',
    );
    fails(
      () =>
        readEntitySchema(schema([{ name: 'vendorAccount', label, type: 'text', editable: false }])),
      '$.fields',
    );
    fails(() => readEntitySchema(schema([], { keyField: 'nope' })), '$.keyField');
    fails(() => readEntitySchema(schema([], { idField: 'vendorAccount' })), '$.idField');
    fails(() => readEntitySchema(schema([], { maxPageSize: 0 })), '$.maxPageSize');
    fails(() => readEntitySchema(schema([], { label: { en: 'Vendors' } })), '$.label.ar');
  });

  it('checks records against the fields', () => {
    const s = readEntitySchema(
      schema([{ name: 'debitLimit', label, type: 'decimal', scale: 2, editable: false }]),
    ) as EntitySchema;
    expect(
      recordProblems(s, { key: 'V-1', values: { vendorAccount: 'V-1', debitLimit: '1.50' } }),
    ).toEqual([]);
    expect(
      recordProblems(s, { key: 'V-1', values: { vendorAccount: 'V-1', debitLimit: 1.5 } }),
    ).toEqual(['debitLimit: not a decimal string at scale 2']);
    expect(
      recordProblems(s, {
        key: 'V-1',
        values: { vendorAccount: 'V-2', debitLimit: null, token: 'x' },
      }),
    ).toEqual(['token: not in the schema', "vendorAccount: differs from the record's key"]);
    expect(recordProblems(s, { key: 'V-1', values: { vendorAccount: 'V-1' } })).toEqual([
      'debitLimit: missing',
    ]);
  });
});

describe('nulls from System.Text.Json', () => {
  it('count as absent in optional properties', () => {
    const read = readEntitySchema(
      schema(
        [
          {
            name: 'mobile',
            label,
            type: 'text',
            editable: true,
            editReason: null,
            required: null,
            maxLength: 50,
            min: null,
            max: null,
            scale: null,
            lookup: null,
          },
        ],
        {
          lookups: {
            paymentModes: [
              { code: 'CASH', label, active: null },
              { code: 'OLD', label, active: false },
            ],
          },
        },
      ),
    );
    expect(read.fields[1]).toEqual({
      name: 'mobile',
      label,
      type: 'text',
      editable: true,
      maxLength: 50,
    });
    expect(read.lookups.paymentModes).toEqual([
      { code: 'CASH', label },
      { code: 'OLD', label, active: false },
    ]);
    const body = readErrorBody({
      message: 'x',
      correlationId: 'c',
      errors: [{ id: null, key: null, field: null, code: 'company.mismatch', message: 'm' }],
    });
    expect(body.errors[0]).toEqual({ code: 'company.mismatch', message: 'm' });
  });
});

describe('result guards', () => {
  const record = {
    id: 4411,
    key: 'V-1001',
    version: 'AAAAAAAAB9E=',
    values: { vendorAccount: 'V-1001' },
  };

  it('keep pages in id order with the next page after the last record', () => {
    const page = {
      records: [record, { ...record, id: 4412 }],
      nextAfterId: 4412,
      total: 3,
      asOf: '2026-11-09T10:00:05Z',
    };
    expect(readQueryResponse(page).nextAfterId).toBe(4412);
    expect(readQueryResponse({ ...page, nextAfterId: null }).nextAfterId).toBeNull();
    fails(
      () => readQueryResponse({ ...page, records: [page.records[1], page.records[0]] }),
      '$.records[1].id',
    );
    fails(() => readQueryResponse({ ...page, nextAfterId: 4411 }), '$.nextAfterId');
    fails(
      () => readQueryResponse({ ...page, records: [{ ...record, version: 'v1' }] }),
      '$.records[0].version',
    );
  });

  it('tie a dry run to having no commit time', () => {
    const body = {
      commitId: 'c',
      dryRun: false,
      committedAt: '2026-11-09T10:05:00Z',
      records: [record],
    };
    expect(readCommitResponse(body).dryRun).toBe(false);
    expect(readCommitResponse({ ...body, dryRun: true, committedAt: null }).dryRun).toBe(true);
    fails(() => readCommitResponse({ ...body, dryRun: true }), '$.committedAt');
    expect(readCommitStatus({ status: 'unknown' })).toEqual({ status: 'unknown' });
    expect(readCommitStatus({ status: 'committed', ...body }).status).toBe('committed');
    fails(
      () => readCommitStatus({ status: 'committed', ...body, dryRun: true, committedAt: null }),
      '$.dryRun',
    );
    fails(() => readCommitStatus({ status: 'pending' }), '$.status');
  });

  it('read errors and conflicts in the ERP’s shape', () => {
    const errors = readErrorBody({
      message: '1 vendor has errors.',
      correlationId: 'c1',
      errors: [
        {
          id: 4411,
          key: 'V-1001',
          field: 'payMethod',
          code: 'lookup.notFound',
          message: 'طريقة الدفع غير موجودة',
        },
      ],
    });
    expect(errors.errors[0]!.code).toBe('lookup.notFound');
    expect(
      readErrorBody({
        message: 'x',
        correlationId: 'c',
        errors: [{ code: 'some.newCode', message: '' }],
      }).errors[0]!.code,
    ).toBe('some.newCode');
    fails(() => readErrorBody({ message: 'x', errors: [] }), '$.correlationId');
    const conflict = {
      ...record,
      fields: ['payMethod'],
      modifiedBy: 'omar',
      modifiedAt: '2026-11-09T10:04:11Z',
    };
    expect(
      readConflictBody({ message: 'm', correlationId: 'c', conflicts: [conflict] }).conflicts,
    ).toHaveLength(1);
    fails(
      () => readConflictBody({ message: 'm', correlationId: 'c', conflicts: [] }),
      '$.conflicts',
    );
    fails(
      () =>
        readConflictBody({
          message: 'm',
          correlationId: 'c',
          conflicts: [{ ...conflict, modifiedAt: 'today' }],
        }),
      '$.conflicts[0].modifiedAt',
    );
  });
});
