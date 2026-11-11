// The ERP texts' catalogue for an ERP user's language (Phase 18): Arabic for Arabic sessions,
// English for every other.
import { EN, type ErpTexts } from './messages';
import { AR } from './messages-ar';

export const textsFor = (language: string | null | undefined): ErpTexts =>
  language === 'ar' ? AR : EN;

export { AR, EN };
export type { ErpTexts };
