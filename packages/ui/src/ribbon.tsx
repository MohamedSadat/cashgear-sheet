// The ribbon: Excel's tabs that have working commands behind them. What each command does is in
// `SheetFormat` and `SheetEditing`, so this only says what's shown and what it's called.
import { useState } from 'react';
import {
  CgBorderPicker,
  CgColorPicker,
  CgIcon,
  type CgBorderApplyDetails,
  type CgBorderLineStyle,
  type CgColorPickerThemeColor,
  type CgColorPickerValue,
  type CgRibbonTab,
} from '@cashgear/ui';
import type { CellFormat, FormatChange, FormatLists } from '@cgs/engine-client';
import { RIBBON_FORMATS, ribbonFormatName } from './number-formats';
import { pickedColor, type SheetFormat } from './sheet-format';
import type { SheetEditing } from './sheet-editing';

export type FormatTab = 'number' | 'alignment' | 'font' | 'border' | 'fill';

export const FONT_NAMES = [
  'Aptos',
  'Aptos Narrow',
  'Arabic Typesetting',
  'Arial',
  'Arial Black',
  'Arial Narrow',
  'Calibri',
  'Calibri Light',
  'Cambria',
  'Century Gothic',
  'Comic Sans MS',
  'Consolas',
  'Courier New',
  'Dubai',
  'Garamond',
  'Georgia',
  'Impact',
  'Sakkal Majalla',
  'Segoe UI',
  'Simplified Arabic',
  'Tahoma',
  'Times New Roman',
  'Traditional Arabic',
  'Trebuchet MS',
  'Verdana',
];

export const FONT_SIZES = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];

/** A theme's colours as the picker shows them, in the order Excel's palette does. */
export function themeColors(lists: FormatLists | null): CgColorPickerThemeColor[] {
  const names = [
    'White, Background 1',
    'Black, Text 1',
    'Light, Background 2',
    'Dark, Text 2',
    'Accent 1',
    'Accent 2',
    'Accent 3',
    'Accent 4',
    'Accent 5',
    'Accent 6',
  ];
  if (!lists) return [];
  return names.map((name, index) => ({
    key: String(index),
    name,
    color: lists.theme[index] ?? '#000000',
  }));
}

/** Text in a combo box that takes what's typed, as the ribbon's font boxes do in Excel. */
function RibbonCombo({
  label,
  value,
  options,
  width,
  disabled,
  testId,
  onCommit,
}: {
  label: string;
  value: string;
  options: readonly string[];
  width: string;
  disabled: boolean;
  testId: string;
  onCommit: (text: string) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const listId = 'cgs-list-' + testId;
  const commit = () => {
    if (draft !== null && draft !== value && draft.trim()) onCommit(draft.trim());
    setDraft(null);
  };
  return (
    <span className="cgs-ribbon-combo" style={{ inlineSize: width }}>
      <input
        aria-label={label}
        title={label}
        data-testid={testId}
        list={listId}
        disabled={disabled}
        value={draft ?? value}
        onFocus={(event) => event.currentTarget.select()}
        onChange={(event) => {
          const text = event.target.value;
          // Picking from the list arrives as a change that matches an option.
          if (options.includes(text) && draft !== null && text !== value) {
            setDraft(null);
            onCommit(text);
            return;
          }
          setDraft(text);
        }}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            commit();
            event.currentTarget.blur();
          } else if (event.key === 'Escape') {
            setDraft(null);
            event.currentTarget.blur();
          }
        }}
      />
      <datalist id={listId}>
        {options.map((option) => (
          <option key={option} value={option} />
        ))}
      </datalist>
    </span>
  );
}

export interface RibbonState {
  hasBook: boolean;
  format: CellFormat | null;
  lists: FormatLists | null;
  undo: string | null;
  redo: string | null;
  canSave: boolean;
  painterOn: boolean;
  manual: boolean;
  frozen: boolean;
  showGrid: boolean;
  showHeadings: boolean;
  showFormulaBar: boolean;
  fontColor: CgColorPickerValue;
  fillColor: CgColorPickerValue;
  borderStyle: CgBorderLineStyle;
  borderColor: CgColorPickerValue;
  borderPreset: CgBorderApplyDetails | null;
}

export interface RibbonActions {
  format: SheetFormat;
  editing: SheetEditing;
  openFormat(tab: FormatTab): void;
  openSize(axis: 'row' | 'col'): void;
  openShift(remove: boolean): void;
  openFind(tab: 'find' | 'replace'): void;
  goTo(): void;
  renameSheet(): void;
  insertSheet(): void;
  deleteSheet(): void;
  calculate(kind: 'now' | 'sheet' | 'all'): void;
  setManual(manual: boolean): void;
  setView(flags: { grid?: boolean; headings?: boolean }): void;
  setFormulaBar(show: boolean): void;
  zoom100(): void;
  zoomToSelection(): void;
  setFontColor(value: CgColorPickerValue): void;
  setFillColor(value: CgColorPickerValue): void;
  setBorderStyle(style: CgBorderLineStyle): void;
  setBorderColor(value: CgColorPickerValue): void;
  applyBorders(details: CgBorderApplyDetails): void;
  mergeCells(kind: 'center' | 'across' | 'cells'): void;
  clearAll(): void;
  clearContents(): void;
}

const fillOf = (value: CgColorPickerValue): FormatChange['fill'] =>
  value.kind === 'none' || value.kind === 'automatic'
    ? { pattern: 'none' }
    : { pattern: 'solid', color: pickedColor(value) };

const swatch = (value: CgColorPickerValue) => ('color' in value ? value.color : 'transparent');

export function buildRibbonTabs(state: RibbonState, actions: RibbonActions): CgRibbonTab[] {
  const { format: sheet, editing } = actions;
  const off = !state.hasBook;
  const shown = state.format;
  const theme = themeColors(state.lists);
  const fontName = shown?.fontName ?? '';
  const fontSize = shown ? String(shown.fontSize) : '';
  const horizontal = shown?.horizontal ?? 'general';
  const vertical = shown?.vertical ?? 'bottom';
  const code = shown?.numberFormat ?? 'General';
  const percent = (n: number) => n;

  return [
    {
      key: 'home',
      text: 'Home',
      groups: [
        {
          key: 'clipboard',
          text: 'Clipboard',
          priority: 2,
          icon: 'paste',
          items: [
            {
              name: 'paste',
              text: 'Paste',
              icon: 'paste',
              size: 'large',
              disabled: off,
              onClick: () => void editing.pasteFromMenu(),
              children: [
                {
                  name: 'paste-values',
                  text: 'Paste Values',
                  onClick: () => editing.pasteMode('values'),
                },
                {
                  name: 'paste-formulas',
                  text: 'Paste Formulas',
                  onClick: () => editing.pasteMode('formulas'),
                },
                {
                  name: 'paste-formats',
                  text: 'Paste Formatting',
                  onClick: () => editing.pasteMode('formats'),
                },
              ],
            },
            {
              name: 'cut',
              text: 'Cut',
              icon: 'cut',
              disabled: off,
              onClick: () => editing.copy(true),
            },
            {
              name: 'copy',
              text: 'Copy',
              icon: 'copy',
              disabled: off,
              onClick: () => editing.copy(false),
            },
            {
              name: 'format-painter',
              text: 'Format Painter',
              icon: 'format-painter',
              disabled: off,
              checked: state.painterOn,
              onCheckedChange: (on) => {
                if (on) sheet.startPainter(false);
                else sheet.stopPainter();
              },
            },
          ],
        },
        {
          key: 'font',
          text: 'Font',
          layout: 'rows',
          priority: 5,
          icon: 'text-font',
          launcherLabel: 'Font settings',
          onLauncherClick: () => actions.openFormat('font'),
          items: [
            {
              name: 'font-name',
              ownsKeyboard: true,
              render: () => (
                <RibbonCombo
                  label="Font"
                  testId="font-name"
                  value={fontName}
                  options={FONT_NAMES}
                  width="9.5rem"
                  disabled={off}
                  onCommit={(name) => sheet.format({ fontName: name })}
                />
              ),
            },
            {
              name: 'font-size',
              ownsKeyboard: true,
              render: () => (
                <RibbonCombo
                  label="Font size"
                  testId="font-size"
                  value={fontSize}
                  options={FONT_SIZES.map(String)}
                  width="3.5rem"
                  disabled={off}
                  onCommit={(text) => {
                    const size = Number(text);
                    if (size >= 1 && size <= 409) sheet.format({ fontSize: size });
                  }}
                />
              ),
            },
            {
              name: 'grow',
              text: 'Increase Font Size',
              icon: 'font-size-increase',
              hideText: true,
              disabled: off,
              onClick: () => sheet.format({ fontStep: 1 }),
            },
            {
              name: 'shrink',
              text: 'Decrease Font Size',
              icon: 'font-size-decrease',
              hideText: true,
              disabled: off,
              onClick: () => sheet.format({ fontStep: -1 }),
            },
            {
              name: 'bold',
              text: 'Bold',
              icon: 'text-bold',
              hideText: true,
              beginRow: true,
              disabled: off,
              checked: Boolean(shown?.bold),
              onCheckedChange: sheet.toggleBold,
            },
            {
              name: 'italic',
              text: 'Italic',
              icon: 'text-italic',
              hideText: true,
              disabled: off,
              checked: Boolean(shown?.italic),
              onCheckedChange: sheet.toggleItalic,
            },
            {
              name: 'underline',
              text: 'Underline',
              icon: 'text-underline',
              hideText: true,
              disabled: off,
              onClick: () => sheet.toggleUnderline('single'),
              children: [
                {
                  name: 'underline-single',
                  text: 'Underline',
                  icon: 'text-underline',
                  onClick: () => sheet.toggleUnderline('single'),
                },
                {
                  name: 'underline-double',
                  text: 'Double Underline',
                  icon: 'text-underline-double',
                  onClick: () => sheet.toggleUnderline('double'),
                },
              ],
            },
            {
              name: 'borders',
              text: 'Borders',
              icon: 'border-bottom',
              hideText: true,
              beginGroup: true,
              disabled: off,
              onClick: () => {
                if (state.borderPreset) actions.applyBorders(state.borderPreset);
                else
                  sheet.format({
                    border: { bottom: { style: 'thin', color: pickedColor(state.borderColor) } },
                  });
              },
              renderFlyout: ({ close }) => (
                <CgBorderPicker
                  lineStyle={state.borderStyle}
                  onLineStyleChange={actions.setBorderStyle}
                  color={state.borderColor}
                  onColorChange={actions.setBorderColor}
                  colorPicker={{ themeColors: theme, allowAutomatic: true }}
                  onApply={(details) => {
                    actions.applyBorders(details);
                    close();
                  }}
                />
              ),
            },
            {
              name: 'fill-color',
              text: 'Fill Color',
              hideText: true,
              beginGroup: true,
              disabled: off,
              icon: <CgIcon name="fill-color" accentColor={swatch(state.fillColor)} />,
              onClick: () => {
                sheet.format({ fill: fillOf(state.fillColor) });
              },
              renderFlyout: ({ close }) => (
                <CgColorPicker
                  themeColors={theme}
                  allowNone
                  allowCustomColor
                  labels={{ none: 'No Fill' }}
                  onValueChange={(value) => {
                    actions.setFillColor(value);
                    sheet.format({ fill: fillOf(value) });
                    close();
                  }}
                />
              ),
            },
            {
              name: 'font-color',
              text: 'Font Color',
              hideText: true,
              disabled: off,
              icon: <CgIcon name="text-color" accentColor={swatch(state.fontColor)} />,
              onClick: () => sheet.format({ fontColor: pickedColor(state.fontColor) }),
              renderFlyout: ({ close }) => (
                <CgColorPicker
                  themeColors={theme}
                  allowAutomatic
                  allowCustomColor
                  onValueChange={(value) => {
                    actions.setFontColor(value);
                    sheet.format({ fontColor: pickedColor(value) });
                    close();
                  }}
                />
              ),
            },
          ],
        },
        {
          key: 'alignment',
          text: 'Alignment',
          layout: 'rows',
          priority: 4,
          icon: 'align-center',
          launcherLabel: 'Alignment settings',
          onLauncherClick: () => actions.openFormat('alignment'),
          items: [
            {
              name: 'align-top',
              text: 'Top Align',
              icon: 'align-top',
              hideText: true,
              disabled: off,
              checked: vertical === 'top',
              onCheckedChange: () => sheet.format({ vertical: 'top' }),
            },
            {
              name: 'align-middle',
              text: 'Middle Align',
              icon: 'align-middle',
              hideText: true,
              disabled: off,
              checked: vertical === 'center',
              onCheckedChange: () => sheet.format({ vertical: 'center' }),
            },
            {
              name: 'align-bottom',
              text: 'Bottom Align',
              icon: 'align-bottom',
              hideText: true,
              disabled: off,
              checked: vertical === 'bottom',
              onCheckedChange: () => sheet.format({ vertical: 'bottom' }),
            },
            {
              name: 'orientation',
              text: 'Orientation',
              icon: 'text-rotate',
              hideText: true,
              beginGroup: true,
              disabled: off,
              children: [
                {
                  name: 'rotate-ccw',
                  text: 'Angle Counterclockwise',
                  onClick: () => sheet.format({ rotation: 45 }),
                },
                {
                  name: 'rotate-cw',
                  text: 'Angle Clockwise',
                  onClick: () => sheet.format({ rotation: 135 }),
                },
                {
                  name: 'rotate-stack',
                  text: 'Vertical Text',
                  onClick: () => sheet.format({ rotation: 255 }),
                },
                {
                  name: 'rotate-up',
                  text: 'Rotate Text Up',
                  onClick: () => sheet.format({ rotation: 90 }),
                },
                {
                  name: 'rotate-down',
                  text: 'Rotate Text Down',
                  onClick: () => sheet.format({ rotation: 180 }),
                },
                {
                  name: 'rotate-none',
                  text: 'No Rotation',
                  onClick: () => sheet.format({ rotation: 0 }),
                },
              ],
            },
            {
              name: 'wrap',
              text: 'Wrap Text',
              icon: 'text-wrap',
              beginGroup: true,
              disabled: off,
              checked: Boolean(shown?.wrap),
              onCheckedChange: sheet.toggleWrap,
            },
            {
              name: 'align-left',
              text: 'Align Left',
              icon: 'align-left',
              hideText: true,
              beginRow: true,
              disabled: off,
              checked: horizontal === 'left',
              onCheckedChange: () => sheet.toggleHorizontal('left'),
            },
            {
              name: 'align-center',
              text: 'Center',
              icon: 'align-center',
              hideText: true,
              disabled: off,
              checked: horizontal === 'center',
              onCheckedChange: () => sheet.toggleHorizontal('center'),
            },
            {
              name: 'align-right',
              text: 'Align Right',
              icon: 'align-right',
              hideText: true,
              disabled: off,
              checked: horizontal === 'right',
              onCheckedChange: () => sheet.toggleHorizontal('right'),
            },
            {
              name: 'indent-less',
              text: 'Decrease Indent',
              icon: 'indent-decrease',
              hideText: true,
              beginGroup: true,
              disabled: off,
              onClick: () => sheet.format({ indentStep: -1 }),
            },
            {
              name: 'indent-more',
              text: 'Increase Indent',
              icon: 'indent-increase',
              hideText: true,
              disabled: off,
              onClick: () => sheet.format({ indentStep: 1 }),
            },
            {
              name: 'merge',
              text: 'Merge & Center',
              icon: 'cells-merge',
              beginGroup: true,
              disabled: off,
              onClick: () => {
                if (shown?.merged) sheet.unmerge();
                else actions.mergeCells('center');
              },
              children: [
                {
                  name: 'merge-center',
                  text: 'Merge & Center',
                  onClick: () => actions.mergeCells('center'),
                },
                {
                  name: 'merge-across',
                  text: 'Merge Across',
                  onClick: () => actions.mergeCells('across'),
                },
                {
                  name: 'merge-cells',
                  text: 'Merge Cells',
                  onClick: () => actions.mergeCells('cells'),
                },
                {
                  name: 'unmerge',
                  text: 'Unmerge Cells',
                  icon: 'cells-split',
                  onClick: () => sheet.unmerge(),
                },
              ],
            },
          ],
        },
        {
          key: 'number',
          text: 'Number',
          layout: 'rows',
          priority: 3,
          icon: 'number-format',
          launcherLabel: 'Number format settings',
          onLauncherClick: () => actions.openFormat('number'),
          items: [
            {
              name: 'number-format',
              ownsKeyboard: true,
              render: () => {
                const name = ribbonFormatName(code);
                const known = RIBBON_FORMATS.some((format) => format.name === name);
                return (
                  <select
                    className="cgs-ribbon-select"
                    aria-label="Number format"
                    title="Number format"
                    data-testid="number-format"
                    disabled={off}
                    value={name}
                    onChange={(event) => {
                      const found = RIBBON_FORMATS.find(
                        (format) => format.name === event.target.value,
                      );
                      if (found) sheet.format({ numberFormat: found.code });
                    }}
                  >
                    {RIBBON_FORMATS.map((format) => (
                      <option key={format.name}>{format.name}</option>
                    ))}
                    {!known && <option>{name}</option>}
                  </select>
                );
              },
            },
            {
              name: 'accounting',
              text: 'Accounting Number Format',
              icon: 'number-currency',
              hideText: true,
              beginRow: true,
              disabled: off,
              onClick: () => sheet.format({ numberFormatId: 44 }),
            },
            {
              name: 'percent',
              text: 'Percent Style',
              icon: 'number-percent',
              hideText: true,
              disabled: off,
              onClick: () => sheet.format({ numberFormatId: percent(9) }),
            },
            {
              name: 'comma',
              text: 'Comma Style',
              icon: 'number-thousands',
              hideText: true,
              disabled: off,
              onClick: () => sheet.format({ numberFormatId: 43 }),
            },
            {
              name: 'more-decimals',
              text: 'Increase Decimal',
              icon: 'decimal-increase',
              hideText: true,
              beginGroup: true,
              disabled: off,
              onClick: () => sheet.format({ decimals: 1 }),
            },
            {
              name: 'fewer-decimals',
              text: 'Decrease Decimal',
              icon: 'decimal-decrease',
              hideText: true,
              disabled: off,
              onClick: () => sheet.format({ decimals: -1 }),
            },
          ],
        },
        {
          key: 'cells',
          text: 'Cells',
          priority: 1,
          icon: 'row-insert',
          items: [
            {
              name: 'insert',
              text: 'Insert',
              icon: 'row-insert',
              size: 'large',
              disabled: off,
              children: [
                {
                  name: 'insert-cells',
                  text: 'Insert Cells…',
                  onClick: () => actions.openShift(false),
                },
                {
                  name: 'insert-rows',
                  text: 'Insert Sheet Rows',
                  onClick: () => editing.axis('row', false),
                },
                {
                  name: 'insert-cols',
                  text: 'Insert Sheet Columns',
                  onClick: () => editing.axis('col', false),
                },
                { name: 'insert-sheet', text: 'Insert Sheet', onClick: actions.insertSheet },
              ],
            },
            {
              name: 'delete',
              text: 'Delete',
              icon: 'row-delete',
              size: 'large',
              disabled: off,
              children: [
                {
                  name: 'delete-cells',
                  text: 'Delete Cells…',
                  onClick: () => actions.openShift(true),
                },
                {
                  name: 'delete-rows',
                  text: 'Delete Sheet Rows',
                  onClick: () => editing.axis('row', true),
                },
                {
                  name: 'delete-cols',
                  text: 'Delete Sheet Columns',
                  onClick: () => editing.axis('col', true),
                },
                { name: 'delete-sheet', text: 'Delete Sheet', onClick: actions.deleteSheet },
              ],
            },
            {
              name: 'format',
              text: 'Format',
              icon: 'number-format',
              size: 'large',
              disabled: off,
              children: [
                {
                  name: 'row-height',
                  text: 'Row Height…',
                  icon: 'row-height',
                  onClick: () => actions.openSize('row'),
                },
                {
                  name: 'autofit-rows',
                  text: 'AutoFit Row Height',
                  icon: 'autofit-height',
                  onClick: () => sheet.autoFit('row'),
                },
                {
                  name: 'col-width',
                  text: 'Column Width…',
                  icon: 'column-width',
                  onClick: () => actions.openSize('col'),
                },
                {
                  name: 'autofit-cols',
                  text: 'AutoFit Column Width',
                  icon: 'autofit-width',
                  onClick: () => sheet.autoFit('col'),
                },
                {
                  name: 'hide-unhide',
                  text: 'Hide & Unhide',
                  beginGroup: true,
                  children: [
                    {
                      name: 'hide-rows',
                      text: 'Hide Rows',
                      onClick: () => sheet.setHidden('row', true),
                    },
                    {
                      name: 'hide-cols',
                      text: 'Hide Columns',
                      onClick: () => sheet.setHidden('col', true),
                    },
                    {
                      name: 'unhide-rows',
                      text: 'Unhide Rows',
                      onClick: () => sheet.setHidden('row', false),
                    },
                    {
                      name: 'unhide-cols',
                      text: 'Unhide Columns',
                      onClick: () => sheet.setHidden('col', false),
                    },
                  ],
                },
                {
                  name: 'rename-sheet',
                  text: 'Rename Sheet…',
                  beginGroup: true,
                  onClick: actions.renameSheet,
                },
                {
                  name: 'format-cells',
                  text: 'Format Cells…',
                  beginGroup: true,
                  onClick: () => actions.openFormat('number'),
                },
              ],
            },
          ],
        },
        {
          key: 'editing',
          text: 'Editing',
          priority: 1,
          icon: 'autosum',
          items: [
            {
              name: 'autosum',
              text: 'AutoSum',
              icon: 'autosum',
              disabled: off,
              onClick: () => editing.action('autoSum'),
            },
            {
              name: 'fill',
              text: 'Fill',
              icon: 'fill-down',
              disabled: off,
              children: [
                { name: 'fill-down', text: 'Down', onClick: () => editing.action('fillDown') },
                { name: 'fill-right', text: 'Right', onClick: () => editing.action('fillRight') },
              ],
            },
            {
              name: 'clear',
              text: 'Clear',
              icon: 'eraser',
              disabled: off,
              children: [
                { name: 'clear-all', text: 'Clear All', onClick: actions.clearAll },
                {
                  name: 'clear-formats',
                  text: 'Clear Formats',
                  icon: 'clear-formatting',
                  onClick: () => sheet.format({ clear: true }),
                },
                { name: 'clear-contents', text: 'Clear Contents', onClick: actions.clearContents },
              ],
            },
            {
              name: 'find',
              text: 'Find & Select',
              icon: 'replace',
              size: 'large',
              disabled: off,
              children: [
                { name: 'find-open', text: 'Find…', onClick: () => actions.openFind('find') },
                {
                  name: 'replace-open',
                  text: 'Replace…',
                  onClick: () => actions.openFind('replace'),
                },
                { name: 'go-to', text: 'Go To…', onClick: actions.goTo },
              ],
            },
          ],
        },
      ],
    },
    {
      key: 'insert',
      text: 'Insert',
      groups: [
        {
          key: 'cells',
          text: 'Cells',
          icon: 'row-insert',
          items: [
            {
              name: 'ins-cells',
              text: 'Cells…',
              icon: 'add',
              size: 'large',
              disabled: off,
              onClick: () => actions.openShift(false),
            },
            {
              name: 'ins-rows',
              text: 'Rows',
              icon: 'row-insert',
              size: 'large',
              disabled: off,
              onClick: () => editing.axis('row', false),
            },
            {
              name: 'ins-cols',
              text: 'Columns',
              icon: 'column-insert',
              size: 'large',
              disabled: off,
              onClick: () => editing.axis('col', false),
            },
            {
              name: 'ins-sheet',
              text: 'Sheet',
              icon: 'workbook',
              size: 'large',
              disabled: off,
              onClick: actions.insertSheet,
            },
          ],
        },
      ],
    },
    {
      key: 'formulas',
      text: 'Formulas',
      groups: [
        {
          key: 'library',
          text: 'Function Library',
          icon: 'formula',
          items: [
            {
              name: 'formulas-autosum',
              text: 'AutoSum',
              icon: 'autosum',
              size: 'large',
              disabled: off,
              onClick: () => editing.action('autoSum'),
            },
          ],
        },
        {
          key: 'calculation',
          text: 'Calculation',
          icon: 'calculator',
          items: [
            {
              name: 'calc-manual',
              text: 'Manual Calculation',
              icon: 'settings',
              size: 'large',
              disabled: off,
              checked: state.manual,
              onCheckedChange: (on) => actions.setManual(on),
            },
            {
              name: 'calc-now',
              text: 'Calculate Now',
              icon: 'calculator',
              disabled: off,
              onClick: () => actions.calculate('now'),
            },
            {
              name: 'calc-sheet',
              text: 'Calculate Sheet',
              icon: 'calculator',
              disabled: off,
              onClick: () => actions.calculate('sheet'),
            },
          ],
        },
      ],
    },
    {
      key: 'view',
      text: 'View',
      groups: [
        {
          key: 'show',
          text: 'Show',
          icon: 'gridlines',
          items: [
            {
              name: 'show-grid',
              text: 'Gridlines',
              icon: 'gridlines',
              disabled: off,
              checked: state.showGrid,
              onCheckedChange: (on) => actions.setView({ grid: on }),
            },
            {
              name: 'show-headings',
              text: 'Headings',
              icon: 'table',
              disabled: off,
              checked: state.showHeadings,
              onCheckedChange: (on) => actions.setView({ headings: on }),
            },
            {
              name: 'show-formula-bar',
              text: 'Formula Bar',
              icon: 'formula',
              checked: state.showFormulaBar,
              onCheckedChange: (on) => actions.setFormulaBar(on),
            },
          ],
        },
        {
          key: 'zoom',
          text: 'Zoom',
          icon: 'calculator',
          items: [
            {
              name: 'zoom-100',
              text: '100%',
              size: 'large',
              icon: 'zoom-in',
              disabled: off,
              onClick: actions.zoom100,
            },
            {
              name: 'zoom-selection',
              text: 'Zoom to Selection',
              size: 'large',
              icon: 'fullscreen',
              disabled: off,
              onClick: actions.zoomToSelection,
            },
          ],
        },
        {
          key: 'window',
          text: 'Window',
          icon: 'freeze-panes',
          items: [
            {
              name: 'freeze',
              text: 'Freeze Panes',
              icon: 'freeze-panes',
              size: 'large',
              disabled: off,
              children: [
                state.frozen
                  ? {
                      name: 'freeze-off',
                      text: 'Unfreeze Panes',
                      icon: 'freeze-off',
                      onClick: () => sheet.freeze('off'),
                    }
                  : {
                      name: 'freeze-panes',
                      text: 'Freeze Panes',
                      icon: 'freeze-panes',
                      onClick: () => sheet.freeze('panes'),
                    },
                {
                  name: 'freeze-row',
                  text: 'Freeze Top Row',
                  icon: 'freeze-top-row',
                  onClick: () => sheet.freeze('row'),
                },
                {
                  name: 'freeze-col',
                  text: 'Freeze First Column',
                  icon: 'freeze-first-column',
                  onClick: () => sheet.freeze('column'),
                },
              ],
            },
          ],
        },
      ],
    },
  ];
}
