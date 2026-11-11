// The ERP texts in Arabic (decisions, 2026-11-11, Phase 18), for ERP users whose language is
// Arabic: every member of the English catalogue, with the terms `ERPClient_React`'s Arabic locale
// uses (الموردون، تحديث، تجاهل، إعادة المحاولة، «تعديل في Cash Gear Sheet»). Counts take Arabic's
// plural forms by `Intl.PluralRules`, digits stay Latin as the ERP's own lists show them, and
// Latin names and values inside a line are isolated so they keep their place.

import type {
  ErpCellInfo,
  ErpConflictInfo,
  ErpCounts,
  ErpReviewEntry,
  ErpRuleInfo,
  ErpServerError,
} from '@cgs/engine-client';
import type { ErpErrorKind } from '@cgs/erp-connector';
import { count, DRAFT_CAP_MB, isolate, type ErpTexts } from './messages';

/** A noun's forms for Arabic's plural categories, and its definite plural and singular. */
interface Noun {
  one: string;
  two: string;
  few: string;
  many: string;
  other: string;
  /** "the vendors", as in "open the vendors". */
  the: string;
  /** "the vendor", as in "this vendor". */
  theOne: string;
}

const NOUNS: Record<string, Noun> = {
  vendors: {
    one: 'مورد',
    two: 'موردان',
    few: 'موردين',
    many: 'موردًا',
    other: 'مورد',
    the: 'الموردين',
    theOne: 'المورد',
  },
};

/** Any other entity's records, until it has its own words. */
const RECORD: Noun = {
  one: 'سجل',
  two: 'سجلان',
  few: 'سجلات',
  many: 'سجلًا',
  other: 'سجل',
  the: 'السجلات',
  theOne: 'السجل',
};

const CHANGE = { one: 'تغيير', two: 'تغييران', few: 'تغييرات', many: 'تغييرًا', other: 'تغيير' };
const CELL = { one: 'خلية', two: 'خليتان', few: 'خلايا', many: 'خليةً', other: 'خلية' };
const FIELD = { one: 'حقل', two: 'حقلان', few: 'حقول', many: 'حقلًا', other: 'حقل' };
const FORMULA = { one: 'صيغة', two: 'صيغتان', few: 'صيغ', many: 'صيغةً', other: 'صيغة' };

const plural = new Intl.PluralRules('ar');

/** `مورد واحد`, `موردان`, `3 موردين`, `12 موردًا`, `100 مورد`. */
function counted(n: number, forms: Omit<Noun, 'the' | 'theOne'>): string {
  switch (plural.select(n)) {
    case 'one':
      return forms === CELL || forms === FORMULA ? `${forms.one} واحدة` : `${forms.one} واحد`;
    case 'two':
      return forms.two;
    case 'few':
      return `${count(n)} ${forms.few}`;
    case 'many':
      return `${count(n)} ${forms.many}`;
    default:
      return `${count(n)} ${forms.other}`;
  }
}

const nounOf = (noun: string) => NOUNS[noun.toLowerCase()] ?? RECORD;
const records = (n: number, noun: string) => counted(n, nounOf(noun));
const changes = (n: number) => counted(n, CHANGE);
const the = (noun: string) => nounOf(noun).the;
const theOne = (noun: string) => nounOf(noun).theOne;

/** The engine's own English refusals and limits, in Arabic; anything else as it is. */
const REFUSALS: Record<string, string> = {
  'This column is read-only in Cash Gear ERP.': 'هذا العمود للقراءة فقط في Cash Gear ERP.',
  "Rows can't be added or deleted in ERP data.": 'لا يمكن إضافة صفوف أو حذفها في بيانات ERP.',
  "Columns can't be added or deleted in ERP data.": 'لا يمكن إضافة أعمدة أو حذفها في بيانات ERP.',
  "The table that holds ERP data can't be resized, renamed or reshaped.":
    'لا يمكن تغيير حجم الجدول الذي يحمل بيانات ERP أو إعادة تسميته أو تغيير شكله.',
  'ERP data can only be sorted top to bottom.': 'لا تُفرز بيانات ERP إلا من الأعلى إلى الأسفل.',
  'ERP data is sorted as a whole table. Select a cell in it and sort again.':
    'تُفرز بيانات ERP جدولًا كاملًا. حدد خلية فيه ثم افرز مرة أخرى.',
  "A sheet that holds ERP data can't be deleted.": 'لا يمكن حذف ورقة تحمل بيانات ERP.',
  'Wait until the commit to Cash Gear ERP finishes.': 'انتظر حتى ينتهي الاعتماد في Cash Gear ERP.',
};

function localized(message: string): string {
  const refusal = REFUSALS[message];
  if (refusal) return refusal;
  // The 10,000-record cap, as the engine words it.
  const cap = /^This list has ([\d,]+) (\w+), and a sheet opens at most ([\d,]+)\./.exec(message);
  if (cap) {
    const total = Number(cap[1]!.replace(/,/g, ''));
    const max = Number(cap[3]!.replace(/,/g, ''));
    return `تضم هذه القائمة ${records(total, cap[2]!)}، وتفتح الورقة ${count(max)} على الأكثر. ضيّق البحث في Cash Gear ERP ثم افتحها مرة أخرى.`;
  }
  return message;
}

function failureText(
  kind: ErpErrorKind,
  message: string,
  noun: string,
): { text: string; retry: boolean } {
  switch (kind) {
    case 'ticket':
      return {
        text: 'انتهت صلاحية هذا الرابط أو استُخدم من قبل. ارجع إلى Cash Gear ERP واختر «تعديل في Cash Gear Sheet» مرة أخرى.',
        retry: false,
      };
    case 'privilege':
      return { text: `لا يتيح حسابك في Cash Gear ERP عرض ${the(noun)}.`, retry: false };
    case 'origin':
      return { text: 'لا يمكن لهذا الموقع الاتصال بـ Cash Gear ERP.', retry: false };
    case 'network':
      return {
        text: 'تعذّر الوصول إلى Cash Gear ERP. تحقّق من الاتصال، ثم حاول مرة أخرى.',
        retry: true,
      };
    case 'busy':
    case 'server':
      return { text: 'تعذّر على Cash Gear ERP الرد. حاول مرة أخرى بعد قليل.', retry: true };
    case 'session':
      return {
        text: 'انتهت جلستك في Cash Gear ERP. افتح الورقة مرة أخرى من Cash Gear ERP.',
        retry: false,
      };
    case 'cancelled':
      return { text: 'لم يُفتح شيء.', retry: false };
    default:
      return { text: localized(message), retry: false };
  }
}

function badgeText(
  label: string,
  total: number,
  state: { expired: boolean; disconnected?: boolean; noDraft?: boolean },
  userName: string,
  company: string,
  counts: ErpCounts = {
    changed: 0,
    changedRecords: 0,
    invalid: 0,
    formulas: 0,
    conflicts: 0,
    gone: 0,
  },
  editable = false,
) {
  const parts = [isolate(label), count(total)];
  if (counts.changed > 0) parts.push(`معدّلة: ${count(counts.changed)}`);
  if (counts.invalid > 0) parts.push(`غير صالحة: ${count(counts.invalid)}`);
  if (counts.formulas > 0) parts.push(`صيغ: ${count(counts.formulas)}`);
  if (counts.conflicts > 0) parts.push(`تعارضات: ${count(counts.conflicts)}`);
  if (counts.gone > 0) parts.push(`غير موجودة في ERP: ${count(counts.gone)}`);
  if (state.disconnected) parts.push('غير متصلة');
  else if (state.expired) parts.push('انتهت الجلسة');
  else if (!editable) parts.push('للقراءة فقط');
  if (state.noDraft) parts.push('لا توجد نسخة استرداد');
  parts.push(`${isolate(userName)} @ ${isolate(company)}`);
  return parts.join(' · ');
}

function reasonText(reason: string | undefined, noun: string) {
  switch (reason) {
    case 'key':
      return 'المفتاح الذي يُبقي كل صف مع سجله في Cash Gear ERP.';
    case 'needsAdvEdit':
      return `يتطلب تعديل هذا صلاحية «تعديل متقدم» على ${the(noun)} في Cash Gear ERP.`;
    case 'needsEdit':
      return `يتطلب تعديل هذا صلاحية «تعديل» على ${the(noun)} في Cash Gear ERP.`;
    default:
      return 'لا يتيح Cash Gear ERP تعديل هذا من الورقة.';
  }
}

const KIND_NEEDS: Record<ErpRuleInfo['type'], string> = {
  text: 'نص',
  lookup: 'رمز من قائمته',
  integer: 'عدد صحيح',
  decimal: 'رقم',
  boolean: 'TRUE أو FALSE',
  date: 'تاريخ',
  dateTime: 'تاريخ ووقت',
};

function ruleText(label: string, rule: ErpRuleInfo) {
  const field = isolate(label);
  switch (rule.code) {
    case 'required':
      return `لا يمكن أن يكون ${field} فارغًا.`;
    case 'maxLength':
      return `يقبل ${field} ${count(rule.maxLength ?? 0)} حرفًا على الأكثر.`;
    case 'type':
      return `يحتاج ${field} إلى ${KIND_NEEDS[rule.type]}.`;
    case 'range':
      if (rule.min !== null && rule.max !== null)
        return `يجب أن يكون ${field} بين ${rule.min} و${rule.max}.`;
      if (rule.min !== null) return `يجب ألا يقل ${field} عن ${rule.min}.`;
      return `يجب ألا يزيد ${field} على ${rule.max ?? 0}.`;
    case 'scale':
      return `يقبل ${field} ${rule.scale ?? 0} منازل عشرية على الأكثر، و15 رقمًا في المجموع.`;
    case 'lookup.notFound':
      return rule.inactive
        ? `لم يعد ${isolate(rule.value ?? '')} متاحًا في ${field}.`
        : `${isolate(rule.value ?? '')} غير موجود في قائمة ${field}.`;
  }
}

function serverText(label: string, error: ErpServerError, noun: string, value?: string): string {
  const field = isolate(label);
  const one = theOne(noun);
  switch (error.code) {
    case 'required':
      return `لا يقبل Cash Gear ERP أن يكون ${field} فارغًا.`;
    case 'maxLength':
      return `${field} أطول مما يقبله Cash Gear ERP.`;
    case 'type':
      return `${field} ليس من نوع القيم التي يقبلها Cash Gear ERP.`;
    case 'range':
      return `${field} خارج ما يقبله Cash Gear ERP.`;
    case 'scale':
      return `في ${field} منازل عشرية أكثر مما يقبله Cash Gear ERP.`;
    case 'lookup.notFound':
      return value
        ? `${isolate(value)} غير موجود في قائمة Cash Gear ERP لـ ${field}.`
        : `هذه القيمة غير موجودة في قائمة Cash Gear ERP لـ ${field}.`;
    case 'field.readOnly':
      return `لا يتيح Cash Gear ERP تعديل ${field} من الورقة.`;
    case 'privilege.denied':
      return `يتطلب تعديل ${field} صلاحية لا يملكها حسابك في Cash Gear ERP.`;
    case 'record.notFound':
      return `هذا ${one} لم يعد موجودًا في Cash Gear ERP.`;
    case 'record.locked':
      return `هذا ${one} مقفل في Cash Gear ERP.`;
    case 'version.missing':
      return `أرسلت الورقة هذا ${one} دون إصداره. حدّث، ثم حاول مرة أخرى.`;
    default:
      return error.message || `رفض Cash Gear ERP هذا (${error.code}).`;
  }
}

const when = (iso: string) =>
  new Date(iso).toLocaleString('ar-EG-u-nu-latn', { dateStyle: 'medium', timeStyle: 'short' });

function conflictText(conflict: ErpConflictInfo): string {
  const parts = ['عُدّل في Cash Gear ERP'];
  if (conflict.by) parts[0] += ` بواسطة ${isolate(conflict.by)}`;
  if (conflict.at) parts[0] += ` في ${when(conflict.at)}`;
  if (conflict.known) parts.push(`الآن ${conflict.now ? isolate(conflict.now) : '(فارغ)'}`);
  return parts.join(' · ');
}

function cellTip(info: ErpCellInfo, noun: string): { title: string; text: string } | null {
  const shown = (value: string | undefined) => (value === undefined ? '(فارغ)' : isolate(value));
  if (info.state === 'gone') {
    return {
      title: isolate(info.label),
      text: `هذا ${theOne(noun)} لم يعد موجودًا في Cash Gear ERP، فلا يمكن اعتماد تغييراته.`,
    };
  }
  if (!info.editable) return { title: isolate(info.label), text: reasonText(info.reason, noun) };
  if (info.heading || !info.state) return null;
  const lines: string[] = [];
  if (info.state === 'formula') {
    lines.push('صيغة: حوّلها إلى قيمة لإرسالها إلى Cash Gear ERP.');
  } else if (info.state === 'committed') {
    lines.push('تم الاعتماد في Cash Gear ERP.');
  } else if (info.original !== undefined || info.now !== undefined) {
    lines.push(`الأصلية ${shown(info.original)} ← الجديدة ${shown(info.now)}`);
  }
  if (info.rule) lines.push(ruleText(info.label, info.rule));
  if (info.server) lines.push(serverText(info.label, info.server, noun, info.now));
  if (info.conflict) lines.push(conflictText(info.conflict));
  return { title: isolate(info.label), text: lines.join('\n') };
}

function validatedText(
  noun: string,
  sent: { changes: number; records: number },
  found: { errors: number; errorRecords: number; conflicts: number; batch: string[] },
) {
  if (!found.errors && !found.conflicts && !found.batch.length)
    return `تحقّق Cash Gear ERP من ${changes(sent.changes)} على ${records(sent.records, noun)} ولم يجد مشكلات.`;
  const parts: string[] = [];
  if (found.errors)
    parts.push(
      `يرفض Cash Gear ERP ${changes(found.errors)} على ${records(found.errorRecords, noun)}`,
    );
  if (found.conflicts)
    parts.push(`عُدّل ${records(found.conflicts, noun)} في Cash Gear ERP منذ فتحتها`);
  const marked = parts.length ? `${parts.join('، و')}. الخلايا محددة.` : '';
  return [marked, ...found.batch].filter(Boolean).join(' ');
}

const localErrorsText = (n: number) =>
  `${counted(n, CELL)} تخالف قاعدة، فلم يُرسل شيء. صحّحها أو تجاهل تلك التغييرات، ثم اعتمد.`;

const conflictsWaitText = (n: number, noun: string) =>
  `في ${records(n, noun)} حقول عدّلتها أنت وعدّلها Cash Gear ERP. اختر «حل التعارضات»، ثم اعتمد.`;

const tooManyText = (n: number, max: number, noun: string, visible: boolean) =>
  `لدى ${records(n, noun)}${visible ? ' في الصفوف المعروضة' : ''} تغييرات، ويقبل Cash Gear ERP ${count(max)} على الأكثر في اعتماد واحد. صفِّ الجدول إلى عدد أقل منها، ثم اختر «اعتماد التغييرات في الصفوف المرئية».`;

function confirmLines(
  noun: string,
  company: string,
  set: { changes: number; records: number; hidden: number; formulas: number },
): string[] {
  const lines = [
    `اعتماد ${changes(set.changes)} على ${records(set.records, noun)} في Cash Gear ERP (${isolate(company)})؟`,
    'يتحقق Cash Gear ERP منها مرة أخرى ويحفظها كلها أو لا يحفظ شيئًا. يُمسح سجل التراجع بعد حفظها.',
  ];
  if (set.hidden > 0)
    lines.push(
      set.hidden === 1
        ? 'أحد هذه التغييرات في صف يخفيه عامل التصفية.'
        : `${count(set.hidden)} من هذه التغييرات في صفوف يخفيها عامل التصفية.`,
    );
  if (set.formulas > 0)
    lines.push(
      set.formulas === 1
        ? 'تُستثنى صيغة واحدة: حوّلها إلى قيمة لاعتمادها.'
        : `تُستثنى ${counted(set.formulas, FORMULA)}: حوّلها إلى قيم لاعتمادها.`,
    );
  return lines;
}

function committedText(n: number, m: number, noun: string, kept: number) {
  const done = `تم اعتماد ${changes(n)} على ${records(m, noun)}.`;
  return kept > 0 ? `${done} وما زال في الورقة ${changes(kept)} أُجري بعد ذلك.` : done;
}

function refusedText(
  noun: string,
  found: { errors: number; errorRecords: number },
  batch: string[],
) {
  const marked = found.errors
    ? `رفض Cash Gear ERP ${changes(found.errors)} على ${records(found.errorRecords, noun)}، فلم يُحفظ شيء. الخلايا محددة، وينتقل «الخطأ التالي» بينها.`
    : 'رفض Cash Gear ERP الاعتماد، فلم يُحفظ شيء.';
  return [marked, ...batch].join(' ');
}

function conflictText409(found: { conflicts: number; merged: number }, noun: string) {
  const n = found.conflicts + found.merged;
  const parts = [`عُدّل ${records(n, noun)} في Cash Gear ERP منذ فتحتها، فلم يُحفظ شيء.`];
  if (found.merged > 0)
    parts.push(`دُمج ${records(found.merged, noun)} مع تغييراتك: اعتمد مرة أخرى لإرسالها.`);
  if (found.conflicts > 0)
    parts.push(
      `في ${records(found.conflicts, noun)} حقول عدّلتماها كلاكما: اختر «حل التعارضات»، ثم اعتمد مرة أخرى.`,
    );
  return parts.join(' ');
}

function commitFailureText(kind: ErpErrorKind, message: string, noun: string, max: number) {
  switch (kind) {
    case 'session':
      return 'انتهت جلستك في Cash Gear ERP، فلا يُعرف إن كانت التغييرات قد حُفظت. اختر «إعادة الاتصال»: تسأل الورقة Cash Gear ERP بعد ذلك قبل أن ترسلها مرة أخرى.';
    case 'privilege':
      return `لا يتيح حسابك في Cash Gear ERP تعديل ${the(noun)}، فلم يُحفظ شيء.`;
    case 'tooMany':
      return `يقبل Cash Gear ERP ${count(max)} على الأكثر من ${the(noun)} في اعتماد واحد. صفِّ الجدول إلى عدد أقل منها، ثم اختر «اعتماد التغييرات في الصفوف المرئية».`;
    case 'contract':
      return AR.UNREADABLE_ANSWER;
    default:
      return message;
  }
}

function batchErrorText(code: string, message: string, noun: string): string {
  switch (code) {
    case 'schema.changed':
      return `تغيّرت بيانات ${the(noun)} في Cash Gear ERP منذ فتحتها الورقة. افتح الورقة مرة أخرى من Cash Gear ERP.`;
    case 'company.mismatch':
      return 'شركة هذه الورقة ليست شركة الجلسة.';
    case 'commit.reused':
      return 'يحمل Cash Gear ERP اعتمادًا آخر بهذا المعرّف. اعتمد مرة أخرى.';
    case 'batch.tooLarge':
      return `يقبل Cash Gear ERP عددًا أقل من ${the(noun)} في اعتماد واحد. صفِّ الجدول إلى عدد أقل منها، ثم اختر «اعتماد التغييرات في الصفوف المرئية».`;
    default:
      return message;
  }
}

function statusText(entry: ErpReviewEntry): string {
  switch (entry.state) {
    case 'formula':
      return 'صيغة، لن تُعتمد';
    case 'conflict':
      return 'تعارض';
    case 'invalid':
      return 'غير صالح';
    case 'gone':
      return 'غير موجود في ERP';
    default:
      return 'معدّل';
  }
}

function reviewMessage(entry: ErpReviewEntry, noun: string): string {
  const lines: string[] = [];
  if (entry.state === 'gone')
    lines.push(`هذا ${theOne(noun)} لم يعد موجودًا في Cash Gear ERP، فلن يُعتمد.`);
  if (entry.state === 'formula') lines.push('حوّلها إلى قيمة لاعتمادها.');
  if (entry.rule) lines.push(ruleText(entry.label, entry.rule));
  if (entry.server) lines.push(serverText(entry.label, entry.server, noun, entry.now));
  if (entry.conflictWith) lines.push(conflictText(entry.conflictWith));
  if (entry.hidden) lines.push('في صف يخفيه عامل التصفية.');
  return lines.join(' ');
}

function refreshedText(
  noun: string,
  summary: { updated: number; changed: number; merged: number; conflicts: number; missing: number },
) {
  const parts = [`تم تحديث ${records(summary.updated, noun)}.`];
  if (summary.changed === 0) parts.push('لم يتغيّر شيء في Cash Gear ERP.');
  else {
    const detail: string[] = [];
    if (summary.merged > 0) detail.push(`دُمج ${count(summary.merged)} مع تغييراتك`);
    if (summary.conflicts > 0) detail.push(`${count(summary.conflicts)} في تعارض`);
    parts.push(
      `عُدّل ${count(summary.changed)} في Cash Gear ERP${detail.length ? `: ${detail.join('، و')}` : ''}.`,
    );
  }
  if (summary.missing > 0)
    parts.push(`لم يعد في Cash Gear ERP: ${records(summary.missing, noun)}.`);
  return parts.join(' ');
}

function resolvedText(found: { fields: number; records: number; left: number }, noun: string) {
  const parts = [`تم حل ${counted(found.fields, FIELD)}.`];
  if (found.records === 1) parts.push(`${records(1, noun)} جاهز للاعتماد.`);
  else if (found.records > 1) parts.push(`جاهزة للاعتماد: ${records(found.records, noun)}.`);
  if (found.left > 0) parts.push(`ما زالت التعارضات في ${records(found.left, noun)}.`);
  return parts.join(' ');
}

export const AR: ErpTexts = {
  language: 'ar',
  dir: 'rtl',
  localized,
  count,
  records,
  changes,
  openingTitle: (noun) => (noun ? `فتح ${the(noun)} من Cash Gear ERP` : 'الفتح من Cash Gear ERP'),
  CONNECTING: 'جارٍ الاتصال بـ Cash Gear ERP…',
  loadingText: (noun, loaded, total) =>
    total === null
      ? `جارٍ تحميل ${the(noun)}…`
      : `جارٍ تحميل ${the(noun)} · ${count(loaded)} من ${count(total)}`,
  CANCELLED: 'لم يُفتح شيء.',
  failureText,
  badgeText,
  reasonText,
  ruleText,
  serverText,
  conflictText,
  cellTip,
  TAB: 'بيانات ERP',
  GROUP_DATA: 'البيانات',
  GROUP_CHANGES: 'التغييرات',
  GROUP_CONFLICTS: 'التعارضات',
  GROUP_CONNECTION: 'الاتصال',
  REFRESH: 'تحديث',
  VALIDATE: 'تحقق',
  REVIEW: 'مراجعة التغييرات',
  COMMIT: 'اعتماد',
  COMMIT_ALL: 'اعتماد كل التغييرات…',
  COMMIT_VISIBLE: 'اعتماد التغييرات في الصفوف المرئية…',
  DISCARD: 'تجاهل التغييرات',
  DISCARD_ALL: 'تجاهل كل التغييرات',
  DISCARD_SELECTED: 'تجاهل التغييرات في الخلايا المحددة',
  NEXT_ERROR: 'الخطأ التالي',
  RESOLVE: 'حل التعارضات',
  NEXT_CONFLICT: 'التعارض التالي',
  CONNECTION_INFO: 'معلومات الاتصال',
  RECONNECT: 'إعادة الاتصال',
  TRY_AGAIN: 'حاول مرة أخرى',
  CANCEL: 'إلغاء',
  CLOSE: 'إغلاق',
  OK: 'موافق',
  GO_TO_CELL: 'الانتقال إلى الخلية',
  NO_ERRORS: 'لا توجد خلايا تخالف قاعدة.',
  NO_CONFLICTS: 'لا توجد خلايا متعارضة.',
  NOTHING_TO_DISCARD: 'لا توجد تغييرات لتجاهلها.',
  refreshingText: (noun) => `جارٍ تحديث ${the(noun)}…`,
  refreshedText,
  HIDDEN_ROW: 'هذا الصف مخفي بعامل تصفية.',
  NOTHING_TO_VALIDATE: 'لا توجد تغييرات للتحقق منها.',
  NOTHING_TO_COMMIT: 'لا توجد تغييرات لاعتمادها.',
  NOTHING_VISIBLE: 'لا توجد تغييرات في الصفوف المعروضة.',
  validatingText: (n) => `جارٍ التحقق من ${changes(n)} مع Cash Gear ERP…`,
  validatedText,
  localErrorsText,
  conflictsWaitText,
  tooManyText,
  COMMIT_TITLE: 'الاعتماد في Cash Gear ERP',
  confirmLines,
  commitButton: (n) => `اعتماد ${changes(n)}`,
  reviewCommitButton: (n, m, noun) => `اعتماد ${changes(n)} على ${records(m, noun)}`,
  committingText: (n, m, noun) => `جارٍ اعتماد ${changes(n)} على ${records(m, noun)}…`,
  committedText,
  refusedText,
  conflictText409,
  UNKNOWN_OUTCOME:
    'لم يرد Cash Gear ERP، فلا يُعرف إن كانت التغييرات قد حُفظت. «حاول مرة أخرى» يسأل Cash Gear ERP أولًا، ولا يرسل الاعتماد نفسه إلا إن لم يُحفظ.',
  SERVER_FAILED:
    'تعذّر على Cash Gear ERP إتمام الاعتماد، فلا يُعرف إن كانت التغييرات قد حُفظت. «حاول مرة أخرى» يسأل Cash Gear ERP أولًا، ولا يرسل الاعتماد نفسه إلا إن لم يُحفظ.',
  BUSY_FAILED: 'Cash Gear ERP مشغول، فلم يُحفظ شيء. حاول مرة أخرى بعد قليل.',
  UNREADABLE_ANSWER:
    'ردّ Cash Gear ERP بطريقة لا تستطيع الورقة قراءتها، فلا يُعرف ما الذي حُفظ. حدّث لترى ما يحمله Cash Gear ERP.',
  ASKING_STATUS: 'جارٍ سؤال Cash Gear ERP إن كان الاعتماد قد حُفظ…',
  STATUS_UNKNOWN:
    'لا يملك Cash Gear ERP سجلًا لهذا الاعتماد، فلم يُحفظ شيء. ما زالت تغييراتك هنا: اعتمدها مرة أخرى عندما تكون جاهزًا.',
  commitFailureText,
  batchErrorText,
  REVIEW_TITLE: 'مراجعة التغييرات',
  FILTER_ALL: 'الكل',
  FILTER_INVALID: 'غير الصالحة',
  FILTER_CONFLICTS: 'التعارضات',
  FILTER_HIDDEN: 'في صفوف مخفية',
  FILTER_FORMULAS: 'الصيغ',
  FILTERS_LABEL: 'عرض',
  COLUMN_RECORD: 'السجل',
  COLUMN_FIELD: 'الحقل',
  COLUMN_ORIGINAL: 'الأصلية',
  COLUMN_NEW: 'الجديدة',
  COLUMN_STATUS: 'الحالة',
  COLUMN_MESSAGE: 'الرسالة',
  NOTHING_HERE: 'لا شيء لعرضه هنا.',
  DISCARD_CHOSEN: 'تجاهل المحدد',
  statusText,
  reviewMessage,
  RESOLVE_TITLE: 'حل التعارضات',
  resolveIntro: (n, noun) =>
    `عُدّل ${records(n, noun)} في Cash Gear ERP منذ تحميلها، في حقول عدّلتها أنت أيضًا.`,
  COLUMN_MINE: 'قيمتي',
  COLUMN_ERP: 'قيمة ERP الآن',
  COLUMN_BY: 'عدّلها',
  KEEP_MINE: 'الاحتفاظ بقيمتي',
  TAKE_ERP: 'أخذ قيمة ERP',
  KEEP_ALL_MINE: 'الاحتفاظ بكل قيمي',
  TAKE_ALL_ERP: 'أخذ كل قيم ERP',
  UNKNOWN_VALUE: 'حدّث لترى ما يحمله Cash Gear ERP.',
  EMPTY: '(فارغ)',
  NO_CONFLICTS_LEFT: 'لم تبق تعارضات.',
  resolvedText,
  changedAt: (by, at) => [by ? isolate(by) : '', at ? when(at) : ''].filter(Boolean).join(' · '),
  SESSION_ENDED:
    'انتهت جلستك في Cash Gear ERP. تبقى تغييراتك في هذه الورقة: اختر «إعادة الاتصال» للمتابعة.',
  RECONNECT_TITLE: 'إعادة الاتصال بـ Cash Gear ERP',
  reconnectText: (noun) =>
    `افتح قائمة ${the(noun)} في Cash Gear ERP واختر «تعديل في Cash Gear Sheet». تُسلّم علامة التبويب الجديدة اتصالها إلى هذه الورقة ثم تُغلق، وتُحدَّث هذه الورقة بعد ذلك. تبقى تغييراتك هنا في الأثناء.`,
  WAITING: 'في انتظار Cash Gear ERP…',
  RECONNECTED: 'تم الاتصال بـ Cash Gear ERP مرة أخرى.',
  refusedHandoverText: (company, user, noun) =>
    `رُفض اتصال لشركة أو مستخدم أو قائمة أخرى: تحمل هذه الورقة ${the(noun)} لشركة ${isolate(company)} والمستخدم ${isolate(user)}.`,
  HANDED_OVER: 'ذهب هذا الاتصال إلى الورقة التي كانت تنتظره. يمكنك إغلاق علامة التبويب هذه.',
  HANDOVER_REFUSED:
    'رفضت الورقة المنتظرة في علامة تبويب أخرى هذا الاتصال. افتح الورقة مرة أخرى من Cash Gear ERP.',
  RECOVER_TITLE: 'استرداد تغييراتك على بيانات ERP؟',
  recoverText: (name, noun) =>
    `في ${isolate(name)} تغييرات على ${the(noun)} في Cash Gear ERP لم تُعتمد قبل انتهاء جلستها. تُفتح غير متصلة: أعد الاتصال من Cash Gear ERP، ثم اعتمدها. لا يُرسل شيء من تلقاء نفسه.`,
  RECOVER: 'استرداد',
  RECOVER_DISCARD: 'تجاهل هذه التغييرات',
  RECOVER_LAUNCH_TITLE: 'استرداد تغييراتك السابقة؟',
  recoverLaunchText: (label, company) =>
    `لديك تغييرات على ${isolate(label)} – ${isolate(company)} من جلسة انتهت قبل اعتمادها. استردها في هذا الاتصال، ثم تُحدَّث، أو افتح البيانات من جديد.`,
  RECOVER_CHANGES: 'استرداد التغييرات',
  OPEN_AFRESH: 'فتح من جديد',
  recoverFailedText: (noun) =>
    `لا يمكن استرداد اتصال ERP هذا. افتح ${the(noun)} مرة أخرى من Cash Gear ERP.`,
  RECOVERED: 'تم الاسترداد دون اتصال بـ Cash Gear ERP. اختر «إعادة الاتصال»، ثم اعتمد تغييراتك.',
  noDraftText: () =>
    `تجاوزت هذه الورقة ${DRAFT_CAP_MB} ميغابايت، فلا تحتفظ بنسخة استرداد: اعتمد أو احفظ قبل إغلاقها.`,
  INFO_TITLE: 'معلومات الاتصال',
  INFO_DATA: 'البيانات',
  INFO_COMPANY: 'الشركة',
  INFO_USER: 'المستخدم',
  INFO_RECORDS: 'السجلات',
  INFO_READ: 'قُرئت من ERP',
  INFO_SESSION: 'الجلسة',
  INFO_SERVER: 'الخادم',
  INFO_EDITING: 'التعديل',
  INFO_RECOVERY: 'نسخة الاسترداد',
  infoRecords: (total, gone) =>
    gone ? `${count(total)} · غير موجودة في ERP: ${count(gone)}` : count(total),
  SESSION_ENDED_SHORT: 'انتهت',
  DISCONNECTED: 'غير متصلة',
  openUntil: (at) =>
    `مفتوحة حتى ${new Date(at).toLocaleTimeString('ar-EG-u-nu-latn', { timeStyle: 'short' })}`,
  readAt: (iso) => new Date(iso).toLocaleString('ar-EG-u-nu-latn'),
  INFO_PROBLEM: 'آخر طلب لم ينجح',
  NO_PROBLEM: 'لا يوجد',
  problemText: (status, at, reference) =>
    `${status === null ? 'لم يصل رد' : `الرد ${status}`} الساعة ${new Date(at).toLocaleTimeString('ar-EG-u-nu-latn', { timeStyle: 'short' })}${reference ? ` · المرجع ${isolate(reference)}` : ''}`,
  editingText: (editable, all) =>
    editable === 0
      ? 'كل الأعمدة للقراءة فقط بالنسبة لك.'
      : `يمكن تعديل ${count(editable)} من ${count(all)} عمودًا. تبقى التغييرات في هذه الورقة حتى تعتمدها.`,
  draftText: (kept) =>
    kept
      ? 'محفوظة على هذا الجهاز حتى تعتمد أو تتجاهل أو تغلق.'
      : `لا توجد: تجاوزت هذه الورقة ${DRAFT_CAP_MB} ميغابايت.`,
  workbookTitle: (label, company) => `${label} – ${company}`,
};
