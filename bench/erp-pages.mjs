// What the binding benches bind (plan B.6): the vendors' 13 fields and 7 more text fields, 20 in
// all, in Query pages of 1,000 records as the ERP sends them. Shared by measure-erp-bind.mjs,
// profile-erp-bind.mjs and erp-speed.mjs.

export const FIELDS = [
  ['vendorAccount', 'text'],
  ['vendorName', 'text'],
  ['vendorGroup', 'lookup'],
  ['mobile', 'text'],
  ['currency', 'lookup'],
  ['payMethod', 'lookup'],
  ['address', 'text'],
  ['taxFile', 'text'],
  ['blocked', 'boolean'],
  ['bankAccount', 'text'],
  ['debitLimit', 'decimal'],
  ['modifiedBy', 'text'],
  ['modifiedDate', 'dateTime'],
  ...Array.from({ length: 7 }, (_, i) => [`extra${i + 1}`, 'text']),
];

export const READ_ONLY = ['vendorAccount', 'debitLimit', 'modifiedDate'];

export const schema = JSON.stringify({
  entity: 'vendors',
  schemaVersion: 'vendors.1',
  label: { en: 'Vendors', ar: 'الموردون' },
  keyField: 'vendorAccount',
  idField: 'recId',
  maxPageSize: 1000,
  maxRecords: 10000,
  maxCommitRecords: 500,
  fields: FIELDS.map(([name, type]) => ({
    name,
    label: { en: name, ar: name },
    type,
    editable: !READ_ONLY.includes(name),
    ...(type === 'decimal' ? { scale: 2 } : {}),
    ...(type === 'lookup' ? { lookup: 'codes' } : {}),
  })),
  lookups: { codes: [{ code: 'LOCAL', label: { en: 'Local', ar: 'محلي' } }] },
});

export const session = JSON.stringify({
  company: 'Kit',
  userId: 'u17',
  userName: 'Sara',
  language: 'en',
  entity: 'vendors',
  entityRoute: 'api/payable/VendorSheet',
});

function value(type, i, name) {
  switch (type) {
    case 'boolean':
      return i % 11 === 0;
    case 'decimal':
      return `${(i * 37) % 100000}.${String(i % 100).padStart(2, '0')}`;
    case 'dateTime':
      return '2026-10-30T08:12:00+02:00';
    case 'lookup':
      return 'LOCAL';
    default:
      return name === 'vendorAccount'
        ? `V-${String(i).padStart(6, '0')}`
        : i % 3 === 0
          ? `شركة النيل ${i} ${name}`
          : `0${100000000 + i} ${name}`;
  }
}

/** Query pages of `n` records, 1,000 a page, as the ERP sends them, each as base64. */
export function pages(n) {
  const list = [];
  for (let start = 0; start < n; start += 1000) {
    const records = [];
    for (let i = start; i < Math.min(n, start + 1000); i++) {
      const values = Object.fromEntries(FIELDS.map(([name, type]) => [name, value(type, i, name)]));
      records.push({ id: i + 1, key: values.vendorAccount, version: 'AAAAAAAAB9E=', values });
    }
    const last = start + 1000 >= n;
    list.push(
      Buffer.from(
        JSON.stringify({
          records,
          nextAfterId: last ? null : records.at(-1).id,
          total: n,
          asOf: '2026-11-09T10:00:05Z',
        }),
      ).toString('base64'),
    );
  }
  return list;
}
