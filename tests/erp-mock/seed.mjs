// Seed data for the mock ERP: one database shared by Kit and Dot, as ASTDataTest is, with
// vendors whose values exercise the contract's wire rules (Arabic and English names, mobiles
// and tax files with leading zeros, decimals, empty fields, a blocked vendor, a locked one).

/** Which database each company lives in. Kit and Dot share one, as they do in the ERP. */
export const COMPANIES = { Kit: 'ASTDataTest', Dot: 'ASTDataTest' };

/** API keys the mock accepts on `CreateTicket`, with the Vendors unit (2004) privileges. */
export const API_KEYS = {
  'mock-kit-editor': {
    company: 'Kit',
    userId: 'u17',
    userName: 'Sara',
    privileges: ['View', 'Edit'],
  },
  'mock-kit-adv': {
    company: 'Kit',
    userId: 'u18',
    userName: 'Omar',
    privileges: ['View', 'Edit', 'AdvEdit'],
  },
  'mock-kit-viewer': { company: 'Kit', userId: 'u19', userName: 'Mona', privileges: ['View'] },
  'mock-kit-none': { company: 'Kit', userId: 'u20', userName: 'Hany', privileges: [] },
  'mock-dot-editor': {
    company: 'Dot',
    userId: 'u31',
    userName: 'Karim',
    privileges: ['View', 'Edit'],
  },
};

const label = (en, ar) => ({ en, ar });

/** One ERP description, sent as both languages, as the ERP's lookup tables hold one. */
const same = (text) => ({ en: text, ar: text });

/**
 * Lookup lists per company, shaped as the ERP's tables are (Track E0): `VendorsGroup` with its
 * `Active` flag, `PaymentModes` (payment terms: `PaymentId`, `Description`, `PaymentDays`) and
 * `LedgerCurrencies`, each with one description. Dot has no Services group, so lookups are
 * visibly per company, and Kit's `OLD` group is inactive but still held by `V-1008`.
 */
export function lookups(company) {
  const groups = [
    { code: 'LOCAL', label: same('محلي') },
    { code: 'IMPORT', label: same('Import') },
  ];
  if (company === 'Kit') {
    groups.push({ code: 'SERVICES', label: same('خدمات') });
    groups.push({ code: 'OLD', label: same('Old suppliers'), active: false });
  }
  return {
    vendorGroups: groups,
    paymentModes: [
      { code: 'CASH', label: same('نقدي') },
      { code: 'NET30', label: same('30 days') },
      { code: 'NET60', label: same('آجل 60 يوم') },
    ],
    currencies: [
      { code: 'EGP', label: same('جنيه مصري') },
      { code: 'USD', label: same('US dollar') },
      { code: 'EUR', label: same('Euro') },
    ],
  };
}

/**
 * The Vendors fields, in the sheet's column order, with the privilege each needs to edit:
 * `Edit`, `AdvEdit`, or `null` for a field that is never editable (decisions, 2026-11-09, Phase
 * 14). The order and labels are the ERP's own Vendors list's (decisions, 2026-11-09, Phase 15).
 */
export const VENDOR_FIELDS = [
  {
    name: 'vendorAccount',
    label: label('Account', 'الحساب'),
    type: 'text',
    maxLength: 50,
    needs: null,
  },
  {
    name: 'vendorName',
    label: label('Name', 'الاسم'),
    type: 'text',
    maxLength: 150,
    required: true,
    needs: 'Edit',
  },
  {
    name: 'vendorGroup',
    label: label('Group', 'المجموعة'),
    type: 'lookup',
    lookup: 'vendorGroups',
    required: true,
    needs: 'Edit',
  },
  {
    name: 'mobile',
    label: label('Mobile', 'الجوال'),
    type: 'text',
    maxLength: 50,
    needs: 'Edit',
  },
  {
    name: 'currency',
    label: label('Currency', 'العملة'),
    type: 'lookup',
    lookup: 'currencies',
    required: true,
    needs: 'AdvEdit',
  },
  {
    name: 'payMethod',
    label: label('Payment method', 'طريقة الدفع'),
    type: 'lookup',
    lookup: 'paymentModes',
    required: true,
    needs: 'Edit',
  },
  {
    name: 'address',
    label: label('Address', 'العنوان'),
    type: 'text',
    maxLength: 150,
    needs: 'Edit',
  },
  {
    name: 'taxFile',
    label: label('Tax file', 'الملف الضريبي'),
    type: 'text',
    maxLength: 50,
    needs: 'Edit',
  },
  {
    name: 'blocked',
    label: label('Blocked', 'موقوف'),
    type: 'boolean',
    required: true,
    needs: 'AdvEdit',
  },
  {
    name: 'bankAccount',
    label: label('Bank account', 'الحساب البنكي'),
    type: 'text',
    maxLength: 50,
    needs: null,
  },
  {
    name: 'debitLimit',
    label: label('Credit limit', 'حد الائتمان'),
    type: 'decimal',
    scale: 2,
    min: '0',
    needs: null,
  },
  {
    name: 'modifiedBy',
    label: label('Modified by', 'عدل بواسطة'),
    type: 'text',
    maxLength: 50,
    needs: null,
  },
  {
    name: 'modifiedDate',
    label: label('Modified', 'تاريخ التعديل'),
    type: 'dateTime',
    needs: null,
  },
];

/** A small deterministic generator (mulberry32), so every reset seeds the same vendors. */
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const ENGLISH = [
  'Delta',
  'Nile',
  'Pyramid',
  'Sphinx',
  'Oasis',
  'Lotus',
  'Cairo',
  'Giza',
  'Red Sea',
];
const ARABIC = [
  'الدلتا',
  'النيل',
  'الأهرام',
  'الواحة',
  'اللوتس',
  'القاهرة',
  'الجيزة',
  'البحر الأحمر',
];
const KINDS = ['Trading', 'Supplies', 'Logistics', 'Foods', 'Steel', 'Paper'];
const KINDS_AR = ['للتجارة', 'للتوريدات', 'للنقل', 'للأغذية', 'للحديد', 'للورق'];

function vendor(company, account, values) {
  return {
    company,
    vendorAccount: account,
    vendorName: '',
    vendorGroup: 'LOCAL',
    payMethod: 'CASH',
    currency: 'EGP',
    mobile: null,
    phone: null,
    taxFile: null,
    address: null,
    blocked: 0,
    bankAccount: null,
    debitLimit: '0.00',
    modifiedBy: 'seed',
    modifiedDate: '2026-10-30T08:12:00Z',
    token: 'never-sent-' + account,
    secRule: '',
    locked: false,
    ...values,
  };
}

function generated(company, n, pick) {
  const arabic = pick() < 0.4;
  const i = Math.floor(pick() * 8);
  const k = Math.floor(pick() * 6);
  const name = arabic
    ? `شركة ${ARABIC[i % ARABIC.length]} ${KINDS_AR[k]} ${n}`
    : `${ENGLISH[i % ENGLISH.length]} ${KINDS[k]} ${n}`;
  const groups = company === 'Kit' ? ['LOCAL', 'IMPORT', 'SERVICES'] : ['LOCAL', 'IMPORT'];
  const cents = Math.floor(pick() * 5_000_000);
  return {
    vendorName: name,
    vendorGroup: groups[Math.floor(pick() * groups.length)],
    payMethod: ['CASH', 'NET30', 'NET60'][Math.floor(pick() * 3)],
    currency: ['EGP', 'EGP', 'EGP', 'USD', 'EUR'][Math.floor(pick() * 5)],
    mobile: pick() < 0.85 ? '01' + String(Math.floor(pick() * 1e9)).padStart(9, '0') : null,
    taxFile: pick() < 0.7 ? String(Math.floor(pick() * 1e6)).padStart(9, '0') : null,
    address: pick() < 0.6 ? `${1 + Math.floor(pick() * 200)} ${arabic ? 'شارع' : 'St'} ${n}` : null,
    blocked: pick() < 0.08 ? 1 : 0,
    debitLimit: `${Math.floor(cents / 100)}.${String(cents % 100).padStart(2, '0')}`,
    modifiedDate: `2026-${String(1 + Math.floor(pick() * 10)).padStart(2, '0')}-${String(
      1 + Math.floor(pick() * 28),
    ).padStart(2, '0')}T09:30:00Z`,
  };
}

/** Kit's hand-made vendors: the values the contract's wire rules must carry unchanged. */
const KIT_FIXED = [
  [
    'V-1001',
    {
      vendorName: 'Al Noor Trading',
      mobile: '01001234567',
      taxFile: '001-234-567',
      address: '12 Tahrir St, Cairo',
      bankAccount: 'EG380019000500000000263180002',
      debitLimit: '15000.00',
    },
  ],
  [
    'V-1002',
    {
      vendorName: 'مؤسسة النور للتوريدات',
      vendorGroup: 'IMPORT',
      payMethod: 'NET30',
      currency: 'USD',
      mobile: '01119876543',
      taxFile: '0045',
      address: 'شارع الهرم، الجيزة',
      debitLimit: '0.00',
    },
  ],
  [
    'V-1003',
    { vendorName: 'Delta Supplies', blocked: 1, debitLimit: '2500.50', phone: '0223456789' },
  ],
  ['V-1004', { vendorName: 'شركة الدلتا', taxFile: '001', mobile: '0020123456789' }],
  ['V-1005', { vendorName: 'Locked Vendor', locked: true, secRule: 'LOCKED' }],
  [
    'V-1006',
    {
      vendorName: 'Noor El Sabah',
      vendorGroup: 'SERVICES',
      payMethod: 'NET60',
      phone: '0227654321',
    },
  ],
  ['V-1007', { vendorName: 'Empty Currency', currency: '' }],
  ['V-1008', { vendorName: 'Old Group Supplies', vendorGroup: 'OLD' }],
];

/** Kit's vendors: the fixed ones, then generated ones to 2,345 in all (three full pages). */
export const KIT_VENDORS = 2345;
export const DOT_VENDORS = 40;

/**
 * The shared database's vendors in `RecId` order. Kit's and Dot's records interleave in the id
 * space, as two companies' rows do in one table, and both have a `V-1001`.
 */
export function seedVendors() {
  const pick = random(20261109);
  const kit = KIT_FIXED.map(([account, values]) => vendor('Kit', account, values));
  for (let n = kit.length + 1; kit.length < KIT_VENDORS; n++) {
    kit.push(vendor('Kit', `V-${String(2000 + n).padStart(5, '0')}`, generated('Kit', n, pick)));
  }
  const dot = [vendor('Dot', 'V-1001', { vendorName: 'Dot Al Noor', mobile: '01500000001' })];
  for (let n = 2; dot.length < DOT_VENDORS; n++) {
    dot.push(vendor('Dot', `D-${String(n).padStart(4, '0')}`, generated('Dot', n, pick)));
  }
  const rows = [];
  let d = 0;
  for (let k = 0; k < kit.length; k++) {
    rows.push(kit[k]);
    if (k % 60 === 3 && d < dot.length) rows.push(dot[d++]);
  }
  while (d < dot.length) rows.push(dot[d++]);
  return rows.map((row, i) => ({ recId: 4411 + i, ...row }));
}
