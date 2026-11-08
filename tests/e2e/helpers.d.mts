import type { Page } from '@playwright/test';
import type { PNG } from 'pngjs';

export function openBytes(page: Page, name: string, bytes: Buffer): Promise<void>;
export function waitForGrid(page: Page, theme?: string): Promise<void>;
export function gridPicture(page: Page, options?: { fromRight?: boolean }): Promise<PNG>;
export function differenceShare(a: PNG, b: PNG): number;
export function expectGolden(
  page: Page,
  name: string,
  limit?: number,
  options?: { fromRight?: boolean },
): Promise<number>;
export const FONT_OVERRIDE: () => void;
export function controlColor(page: Page): Promise<string>;
