// The texts of a workbook from Cash Gear Workspace (Track S1), in the Workspace user's language:
// Arabic for Arabic sessions, English for every other (decisions, 2026-11-12). Arabic uses the
// terms the ERP catalogue does (إعادة الاتصال، إلغاء، حفظ) and Office's مصنف for a workbook. Names
// inside a line are isolated so they keep their place.

import { isolate } from '../erp/messages';
import type { CloudReason, CloudStatus } from '../platform';

export interface CloudTexts {
  readonly dir: 'ltr' | 'rtl';
  readonly lang: 'en' | 'ar';
  readonly WORKSPACE: string;
  /** The status bar's badge, or null when there is nothing to show. */
  badgeText(status: CloudStatus): string | null;
  openingTitle(name: string | null): string;
  openingText(name: string | null, received: number, total: number | null): string;
  /** Why a launch opened nothing. */
  failedText(reason: CloudReason): string;
  /** Why a save, a copy or a reload didn't go, where no dialog of its own says so. */
  saveFailedText(reason: CloudReason): string;
  readonly CONFLICT_TITLE: string;
  conflictText(name: string, by: string | null, at: string | null): string;
  readonly VIEW_ONLY_TITLE: string;
  viewOnlyText(name: string): string;
  readonly ENDED_TITLE: string;
  endedText(name: string): string;
  readonly GONE_TITLE: string;
  goneText(name: string): string;
  readonly COPY_REFUSED_TITLE: string;
  readonly COPY_REFUSED_TEXT: string;
  readonly NOT_SAVED_TITLE: string;
  readonly WAITING_TITLE: string;
  waitingText(name: string): string;
  readonly HANDED_OVER: string;
  readonly RECONNECTED: string;
  copiedText(name: string): string;
  readonly SAVE_COPY: string;
  readonly RELOAD: string;
  readonly RECONNECT: string;
  readonly STOP_WAITING: string;
  readonly SAVE_TO_DEVICE: string;
  readonly CANCEL: string;
  readonly CLOSE: string;
}

const percent = (received: number, total: number | null) =>
  total ? ` ${Math.min(100, Math.floor((received / total) * 100))}%` : '';

const timeEn = (at: number | string) =>
  new Date(at).toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
const whenEn = (at: string) =>
  new Date(at).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });
const timeAr = (at: number | string) =>
  new Date(at).toLocaleTimeString('ar-EG-u-nu-latn', { hour: 'numeric', minute: '2-digit' });
const whenAr = (at: string) =>
  new Date(at).toLocaleString('ar-EG-u-nu-latn', { dateStyle: 'medium', timeStyle: 'short' });

export const EN: CloudTexts = {
  dir: 'ltr',
  lang: 'en',
  WORKSPACE: 'Cash Gear Workspace',
  badgeText(status) {
    switch (status.kind) {
      case 'opening':
        return 'Opening from Cash Gear Workspace…';
      case 'open': {
        const parts = [`${isolate(status.name)} – Cash Gear Workspace`];
        if (status.access === 'view') parts.push('View only');
        if (status.savedAt !== null) parts.push(`Saved ${timeEn(status.savedAt)}`);
        return parts.join(' · ');
      }
      case 'failed':
        return 'Cash Gear Workspace: nothing opened';
      case 'ended':
        return `${isolate(status.name)} – Cash Gear Workspace · Session ended`;
      case 'waiting':
        return 'Waiting for Cash Gear Workspace…';
      case 'handedOver':
        return 'Cash Gear Workspace: handed to the waiting sheet';
      case 'closed':
        return null;
    }
  },
  openingTitle: (name) => (name ? `Opening ${isolate(name)}` : 'Opening from Cash Gear Workspace'),
  openingText: (name, received, total) =>
    name
      ? `Reading ${isolate(name)} from Cash Gear Workspace…${percent(received, total)}`
      : 'Connecting to Cash Gear Workspace…',
  failedText(reason) {
    switch (reason) {
      case 'ticket':
        return 'This link has expired or was already used. Go back to Cash Gear Workspace and choose Open in Cash Gear Sheet again.';
      case 'origin':
        return 'This site can’t connect to Cash Gear Workspace.';
      case 'denied':
        return 'You don’t have access to this workbook in Cash Gear Workspace.';
      case 'gone':
        return 'This workbook was deleted in Cash Gear Workspace.';
      case 'session':
        return 'Your Cash Gear Workspace session ended before the workbook opened. Open it again from Cash Gear Workspace.';
      case 'busy':
        return 'Cash Gear Workspace is busy. Open the workbook again in a moment.';
      case 'network':
      case 'readUrl':
        return 'Cash Gear Workspace can’t be reached, so nothing was opened. Check your connection and open the workbook again.';
      case 'contract':
        return 'Cash Gear Workspace sent an answer the sheet can’t read, so nothing was opened.';
      case 'cancelled':
        return 'Nothing was opened.';
      default:
        return 'Cash Gear Workspace couldn’t open the workbook.';
    }
  },
  saveFailedText(reason) {
    switch (reason) {
      case 'tooLarge':
        return 'This workbook is too large to save to Cash Gear Workspace. Save it to this device instead.';
      case 'invalid':
        return 'Cash Gear Workspace refused the workbook, so it wasn’t saved there.';
      case 'busy':
        return 'Cash Gear Workspace is busy, so the workbook wasn’t saved. Try again in a moment.';
      case 'network':
      case 'readUrl':
        return 'Cash Gear Workspace can’t be reached, so the workbook wasn’t saved there. Your changes are still here.';
      case 'contract':
        return 'Cash Gear Workspace sent an answer the sheet can’t read. Your changes are still here.';
      case 'denied':
        return 'You don’t have access to this workbook in Cash Gear Workspace any more.';
      default:
        return 'Cash Gear Workspace couldn’t save the workbook. Your changes are still here.';
    }
  },
  CONFLICT_TITLE: 'A newer version was saved',
  conflictText: (name, by, at) =>
    `${by ? isolate(by) : 'Someone'} saved a newer version of ${isolate(name)} in Cash Gear Workspace${at ? ` on ${whenEn(at)}` : ''}. Your changes are still here. Save a copy keeps them as a new workbook beside it; Reload opens the newer version and discards them.`,
  VIEW_ONLY_TITLE: 'View only',
  viewOnlyText: (name) =>
    `You can view ${isolate(name)} in Cash Gear Workspace but not save changes to it. Save a copy keeps your changes as a new workbook beside it.`,
  ENDED_TITLE: 'Your Cash Gear Workspace session has ended',
  endedText: (name) =>
    `To save ${isolate(name)} to Cash Gear Workspace, reconnect: choose Reconnect, then open the workbook again from Cash Gear Workspace. That tab hands its connection to this one and closes. Your changes stay here meanwhile.`,
  GONE_TITLE: 'The workbook was deleted',
  goneText: (name) =>
    `${isolate(name)} was deleted in Cash Gear Workspace, so it can’t be saved there. Your changes are still here.`,
  COPY_REFUSED_TITLE: 'No copy was made',
  COPY_REFUSED_TEXT:
    'Cash Gear Workspace didn’t let you make a copy here. You can save the workbook to this device instead.',
  NOT_SAVED_TITLE: 'Not saved to Cash Gear Workspace',
  WAITING_TITLE: 'Reconnect to Cash Gear Workspace',
  waitingText: (name) =>
    `Open ${isolate(name)} again from Cash Gear Workspace and choose Open in Cash Gear Sheet. The new tab hands its connection to this one and closes. You can go on working meanwhile.`,
  HANDED_OVER: 'This connection went to the sheet that was waiting for it. You can close this tab.',
  RECONNECTED: 'Reconnected to Cash Gear Workspace.',
  copiedText: (name) => `Saved as ${isolate(name)} in Cash Gear Workspace.`,
  SAVE_COPY: 'Save a Copy',
  RELOAD: 'Reload',
  RECONNECT: 'Reconnect',
  STOP_WAITING: 'Stop Waiting',
  SAVE_TO_DEVICE: 'Save to This Device',
  CANCEL: 'Cancel',
  CLOSE: 'Close',
};

export const AR: CloudTexts = {
  dir: 'rtl',
  lang: 'ar',
  WORKSPACE: 'Cash Gear Workspace',
  badgeText(status) {
    switch (status.kind) {
      case 'opening':
        return 'جارٍ الفتح من Cash Gear Workspace…';
      case 'open': {
        const parts = [`${isolate(status.name)} – Cash Gear Workspace`];
        if (status.access === 'view') parts.push('للعرض فقط');
        if (status.savedAt !== null) parts.push(`حُفظ ${timeAr(status.savedAt)}`);
        return parts.join(' · ');
      }
      case 'failed':
        return 'Cash Gear Workspace: لم يُفتح شيء';
      case 'ended':
        return `${isolate(status.name)} – Cash Gear Workspace · انتهت الجلسة`;
      case 'waiting':
        return 'في انتظار Cash Gear Workspace…';
      case 'handedOver':
        return 'Cash Gear Workspace: سُلّم الاتصال إلى الورقة المنتظرة';
      case 'closed':
        return null;
    }
  },
  openingTitle: (name) => (name ? `فتح ${isolate(name)}` : 'الفتح من Cash Gear Workspace'),
  openingText: (name, received, total) =>
    name
      ? `جارٍ قراءة ${isolate(name)} من Cash Gear Workspace…${percent(received, total)}`
      : 'جارٍ الاتصال بـ Cash Gear Workspace…',
  failedText(reason) {
    switch (reason) {
      case 'ticket':
        return 'انتهت صلاحية هذا الرابط أو استُخدم من قبل. ارجع إلى Cash Gear Workspace واختر «فتح في Cash Gear Sheet» مرة أخرى.';
      case 'origin':
        return 'لا يمكن لهذا الموقع الاتصال بـ Cash Gear Workspace.';
      case 'denied':
        return 'ليس لديك صلاحية الوصول إلى هذا المصنف في Cash Gear Workspace.';
      case 'gone':
        return 'حُذف هذا المصنف في Cash Gear Workspace.';
      case 'session':
        return 'انتهت جلستك في Cash Gear Workspace قبل أن يُفتح المصنف. افتحه مرة أخرى من Cash Gear Workspace.';
      case 'busy':
        return 'Cash Gear Workspace مشغول. افتح المصنف مرة أخرى بعد قليل.';
      case 'network':
      case 'readUrl':
        return 'تعذّر الوصول إلى Cash Gear Workspace، فلم يُفتح شيء. تحقق من اتصالك وافتح المصنف مرة أخرى.';
      case 'contract':
        return 'أرسل Cash Gear Workspace ردًا لا تستطيع الورقة قراءته، فلم يُفتح شيء.';
      case 'cancelled':
        return 'لم يُفتح شيء.';
      default:
        return 'تعذّر على Cash Gear Workspace فتح المصنف.';
    }
  },
  saveFailedText(reason) {
    switch (reason) {
      case 'tooLarge':
        return 'هذا المصنف أكبر من أن يُحفظ في Cash Gear Workspace. احفظه على هذا الجهاز بدلًا من ذلك.';
      case 'invalid':
        return 'رفض Cash Gear Workspace المصنف، فلم يُحفظ هناك.';
      case 'busy':
        return 'Cash Gear Workspace مشغول، فلم يُحفظ المصنف. حاول مرة أخرى بعد قليل.';
      case 'network':
      case 'readUrl':
        return 'تعذّر الوصول إلى Cash Gear Workspace، فلم يُحفظ المصنف هناك. ما زالت تغييراتك هنا.';
      case 'contract':
        return 'أرسل Cash Gear Workspace ردًا لا تستطيع الورقة قراءته. ما زالت تغييراتك هنا.';
      case 'denied':
        return 'لم تعد لديك صلاحية الوصول إلى هذا المصنف في Cash Gear Workspace.';
      default:
        return 'تعذّر على Cash Gear Workspace حفظ المصنف. ما زالت تغييراتك هنا.';
    }
  },
  CONFLICT_TITLE: 'حُفظ إصدار أحدث',
  conflictText: (name, by, at) =>
    `حفظ ${by ? isolate(by) : 'مستخدم آخر'} إصدارًا أحدث من ${isolate(name)} في Cash Gear Workspace${at ? ` في ${whenAr(at)}` : ''}. ما زالت تغييراتك هنا. «حفظ نسخة» يحتفظ بها في مصنف جديد بجانبه، و«إعادة التحميل» يفتح الإصدار الأحدث ويتجاهلها.`,
  VIEW_ONLY_TITLE: 'للعرض فقط',
  viewOnlyText: (name) =>
    `يمكنك عرض ${isolate(name)} في Cash Gear Workspace دون حفظ تغييرات عليه. «حفظ نسخة» يحتفظ بتغييراتك في مصنف جديد بجانبه.`,
  ENDED_TITLE: 'انتهت جلستك في Cash Gear Workspace',
  endedText: (name) =>
    `لحفظ ${isolate(name)} في Cash Gear Workspace أعد الاتصال: اختر «إعادة الاتصال» ثم افتح المصنف مرة أخرى من Cash Gear Workspace. تُسلّم علامة التبويب تلك اتصالها إلى هذه ثم تُغلق. تبقى تغييراتك هنا في الأثناء.`,
  GONE_TITLE: 'حُذف المصنف',
  goneText: (name) =>
    `حُذف ${isolate(name)} في Cash Gear Workspace، فلا يمكن حفظه هناك. ما زالت تغييراتك هنا.`,
  COPY_REFUSED_TITLE: 'لم تُنشأ نسخة',
  COPY_REFUSED_TEXT:
    'لم يسمح لك Cash Gear Workspace بإنشاء نسخة هنا. يمكنك حفظ المصنف على هذا الجهاز بدلًا من ذلك.',
  NOT_SAVED_TITLE: 'لم يُحفظ في Cash Gear Workspace',
  WAITING_TITLE: 'إعادة الاتصال بـ Cash Gear Workspace',
  waitingText: (name) =>
    `افتح ${isolate(name)} مرة أخرى من Cash Gear Workspace واختر «فتح في Cash Gear Sheet». تُسلّم علامة التبويب الجديدة اتصالها إلى هذه ثم تُغلق. يمكنك متابعة العمل في الأثناء.`,
  HANDED_OVER: 'ذهب هذا الاتصال إلى الورقة التي كانت تنتظره. يمكنك إغلاق علامة التبويب هذه.',
  RECONNECTED: 'تم الاتصال بـ Cash Gear Workspace مرة أخرى.',
  copiedText: (name) => `حُفظ باسم ${isolate(name)} في Cash Gear Workspace.`,
  SAVE_COPY: 'حفظ نسخة',
  RELOAD: 'إعادة التحميل',
  RECONNECT: 'إعادة الاتصال',
  STOP_WAITING: 'إيقاف الانتظار',
  SAVE_TO_DEVICE: 'الحفظ على هذا الجهاز',
  CANCEL: 'إلغاء',
  CLOSE: 'إغلاق',
};

export const cloudTextsFor = (language: string | null | undefined): CloudTexts =>
  language === 'ar' ? AR : EN;
