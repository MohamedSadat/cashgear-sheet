// The Format Cells dialog: Number, Alignment, Font, Border and Fill. It starts from how the active cell
// looks, and only what's changed here goes to the selection, as in Excel.
import { useEffect, useMemo, useState } from 'react';
import {
  CgButton,
  CgColorPicker,
  CgDropDownButton,
  CgTabs,
  CgWindow,
  type CgColorPickerThemeColor,
  type CgColorPickerValue,
} from '@cashgear/ui';
import type {
  BorderChange,
  CellFormat,
  FormatChange,
  FormatColor,
  FormatLine,
  StoredColor,
} from '@cgs/engine-client';
import {
  accountingCode,
  BUILT_IN_CODES,
  categoryOf,
  currencyCode,
  CURRENCIES,
  DATE_CODES,
  FRACTION_CODES,
  NUMBER_CATEGORIES,
  numberCode,
  placesOf,
  SPECIAL_CODES,
  TIME_CODES,
  type NumberCategory,
} from './number-formats';
import { FONT_NAMES, FONT_SIZES, type FormatTab } from './ribbon';
import { pickedColor, shownColor } from './sheet-format';

const HORIZONTAL = [
  ['general', 'General'],
  ['left', 'Left (Indent)'],
  ['center', 'Center'],
  ['right', 'Right (Indent)'],
  ['fill', 'Fill'],
  ['justify', 'Justify'],
  ['centerContinuous', 'Center Across Selection'],
  ['distributed', 'Distributed (Indent)'],
] as const;

const VERTICAL = [
  ['top', 'Top'],
  ['center', 'Center'],
  ['bottom', 'Bottom'],
  ['justify', 'Justify'],
  ['distributed', 'Distributed'],
] as const;

const UNDERLINES = [
  ['none', 'None'],
  ['single', 'Single'],
  ['double', 'Double'],
  ['singleAccounting', 'Single Accounting'],
  ['doubleAccounting', 'Double Accounting'],
] as const;

const PATTERNS: [string, string][] = [
  ['solid', 'Solid'],
  ['darkGray', '75% Gray'],
  ['mediumGray', '50% Gray'],
  ['lightGray', '25% Gray'],
  ['gray125', '12.5% Gray'],
  ['gray0625', '6.25% Gray'],
  ['darkHorizontal', 'Horizontal Stripe'],
  ['darkVertical', 'Vertical Stripe'],
  ['darkDown', 'Reverse Diagonal Stripe'],
  ['darkUp', 'Diagonal Stripe'],
  ['darkTrellis', 'Thick Diagonal Crosshatch'],
  ['lightHorizontal', 'Thin Horizontal Stripe'],
  ['lightVertical', 'Thin Vertical Stripe'],
  ['lightDown', 'Thin Reverse Diagonal Stripe'],
  ['lightUp', 'Thin Diagonal Stripe'],
  ['darkGrid', 'Thin Horizontal Crosshatch'],
  ['lightGrid', 'Thin Crosshatch'],
  ['lightTrellis', 'Thin Diagonal Crosshatch'],
];

const LINE_STYLES = [
  'none',
  'hair',
  'dotted',
  'dashDotDot',
  'dashDot',
  'dashed',
  'thin',
  'mediumDashDotDot',
  'slantDashDot',
  'mediumDashDot',
  'mediumDashed',
  'medium',
  'thick',
  'double',
];

type Edge =
  | 'top'
  | 'insideHorizontal'
  | 'bottom'
  | 'left'
  | 'insideVertical'
  | 'right'
  | 'diagonalUp'
  | 'diagonalDown';

/** A colour control in a dialog: a button that opens the picker. */
function ColorField({
  label,
  value,
  theme,
  automatic,
  noneLabel,
  testId,
  onChange,
}: {
  label: string;
  value: CgColorPickerValue;
  theme: CgColorPickerThemeColor[];
  automatic?: boolean;
  noneLabel?: string;
  testId: string;
  onChange: (value: CgColorPickerValue) => void;
}) {
  const shown = 'color' in value ? value.color : 'transparent';
  return (
    <CgDropDownButton
      data-testid={testId}
      icon={<span className="cgs-color-chip" style={{ background: shown }} />}
      renderFlyout={({ close }) => (
        <CgColorPicker
          value={value}
          themeColors={theme}
          allowAutomatic={automatic}
          allowNone={noneLabel !== undefined}
          allowCustomColor
          labels={noneLabel ? { none: noneLabel } : {}}
          onValueChange={(next) => {
            onChange(next);
            close();
          }}
        />
      )}
    >
      {label}
    </CgDropDownButton>
  );
}

const swatchOf = (color: StoredColor) => ('color' in color ? color.color : undefined);

/** Keeps a number box's text as typed until it's a number. */
function NumberField({
  label,
  value,
  min,
  max,
  testId,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  testId: string;
  onChange: (value: number) => void;
}) {
  return (
    <label className="cgs-dialog-field">
      <span>{label}</span>
      <input
        type="number"
        data-testid={testId}
        min={min}
        max={max}
        value={value}
        onChange={(event) => {
          const next = Number(event.target.value);
          if (Number.isFinite(next)) onChange(Math.max(min, Math.min(max, next)));
        }}
      />
    </label>
  );
}

export interface FormatCellsProps {
  open: boolean;
  tab: FormatTab;
  format: CellFormat;
  theme: CgColorPickerThemeColor[];
  /** Codes the workbook itself defines, for the Custom list. */
  codes: string[];
  /** What the active cell would look like in a code, or null if the code can't be used. */
  sample: (code: string) => Promise<string | null>;
  onApply: (change: FormatChange, merge: boolean | null) => void;
  onClose: () => void;
}

export function FormatCellsDialog(props: FormatCellsProps) {
  const { format, theme, open } = props;
  const [tab, setTab] = useState<string>(props.tab);
  useEffect(() => setTab(props.tab), [props.tab, open]);

  // Number.
  const [category, setCategory] = useState<NumberCategory>(categoryOf(format.numberFormat));
  const [places, setPlaces] = useState(placesOf(format.numberFormat));
  const [grouping, setGrouping] = useState(format.numberFormat.includes('#,##'));
  const [negative, setNegative] = useState(0);
  const [symbol, setSymbol] = useState('$');
  const [listed, setListed] = useState(format.numberFormat);
  const [custom, setCustom] = useState(format.numberFormat);
  const [sampleText, setSampleText] = useState<string | null>('');
  const code = useMemo(() => {
    switch (category) {
      case 'General':
        return 'General';
      case 'Number':
        return numberCode(places, grouping, negative);
      case 'Currency':
        return currencyCode(places, symbol, negative);
      case 'Accounting':
        return accountingCode(places, symbol);
      case 'Percentage':
        return '0' + (places ? '.' + '0'.repeat(places) : '') + '%';
      case 'Scientific':
        return '0' + (places ? '.' + '0'.repeat(places) : '') + 'E+00';
      case 'Text':
        return '@';
      case 'Custom':
        return custom;
      default:
        return listed;
    }
  }, [category, custom, grouping, listed, negative, places, symbol]);
  const { sample } = props;
  useEffect(() => {
    let live = true;
    void sample(code).then((text) => {
      if (live) setSampleText(text);
    });
    return () => {
      live = false;
    };
  }, [code, sample]);

  // Alignment.
  const [horizontal, setHorizontal] = useState(format.horizontal);
  const [vertical, setVertical] = useState(format.vertical);
  const [indent, setIndent] = useState(format.indent);
  const [rotation, setRotation] = useState(format.rotation);
  const [wrap, setWrap] = useState(format.wrap);
  const [shrink, setShrink] = useState(format.shrink);
  const [merged, setMerged] = useState(format.merged);
  const [readingOrder, setReadingOrder] = useState(format.readingOrder);
  const degrees = rotation > 90 && rotation !== 255 ? 90 - rotation : rotation;

  // Font.
  const [fontName, setFontName] = useState(format.fontName);
  const [fontSize, setFontSize] = useState(format.fontSize);
  const [bold, setBold] = useState(format.bold);
  const [italic, setItalic] = useState(format.italic);
  const [strike, setStrike] = useState(format.strike);
  const [underline, setUnderline] = useState(format.underline);
  const [fontColor, setFontColor] = useState<CgColorPickerValue>(shownColor(format.fontColor));
  const [fontColorChanged, setFontColorChanged] = useState(false);

  // Fill.
  const [fillColor, setFillColor] = useState<CgColorPickerValue>(
    format.fill.pattern === 'none' ? { kind: 'none' } : shownColor(format.fill.color),
  );
  const [patternColor, setPatternColor] = useState<CgColorPickerValue>(
    shownColor(format.fill.patternColor),
  );
  const [pattern, setPattern] = useState(format.fill.pattern);
  const [fillChanged, setFillChanged] = useState(false);

  // Border: what's been touched here, as lines.
  const [locked, setLocked] = useState(format.locked);
  const [formulaHidden, setFormulaHidden] = useState(format.formulaHidden);
  const [lineStyle, setLineStyle] = useState('thin');
  const [lineColor, setLineColor] = useState<CgColorPickerValue>({ kind: 'automatic' });
  const [edges, setEdges] = useState<Partial<Record<Edge, FormatLine>>>({});
  const line = (): FormatLine => ({ style: lineStyle, color: pickedColor(lineColor) });
  const edgeOn = (edge: Edge) => {
    const set = edges[edge];
    if (set) return set.style !== 'none';
    switch (edge) {
      case 'top':
        return format.border.top.style !== 'none';
      case 'bottom':
        return format.border.bottom.style !== 'none';
      case 'left':
        return format.border.left.style !== 'none';
      case 'right':
        return format.border.right.style !== 'none';
      case 'diagonalUp':
        return format.border.diagonalUp;
      case 'diagonalDown':
        return format.border.diagonalDown;
      default:
        return false;
    }
  };
  const toggleEdge = (edge: Edge) => {
    setEdges((list) => ({
      ...list,
      [edge]: edgeOn(edge) ? { style: 'none' } : line(),
    }));
  };
  const preset = (kind: 'none' | 'outline' | 'inside') => {
    const none = { style: 'none' };
    if (kind === 'none') {
      setEdges({
        top: none,
        bottom: none,
        left: none,
        right: none,
        insideHorizontal: none,
        insideVertical: none,
        diagonalUp: none,
        diagonalDown: none,
      });
    } else if (kind === 'outline') {
      const lines = line();
      setEdges((list) => ({ ...list, top: lines, bottom: lines, left: lines, right: lines }));
    } else {
      const lines = line();
      setEdges((list) => ({ ...list, insideHorizontal: lines, insideVertical: lines }));
    }
  };

  const apply = () => {
    const change: FormatChange = {};
    if (code !== format.numberFormat) change.numberFormat = code;
    if (horizontal !== format.horizontal) change.horizontal = horizontal;
    if (vertical !== format.vertical) change.vertical = vertical;
    if (indent !== format.indent) change.indent = indent;
    if (rotation !== format.rotation) change.rotation = rotation;
    if (wrap !== format.wrap) change.wrap = wrap;
    if (shrink !== format.shrink) change.shrink = shrink;
    if (readingOrder !== format.readingOrder) change.readingOrder = readingOrder;
    if (fontName !== format.fontName) change.fontName = fontName;
    if (fontSize !== format.fontSize) change.fontSize = fontSize;
    if (bold !== format.bold) change.bold = bold;
    if (italic !== format.italic) change.italic = italic;
    if (strike !== format.strike) change.strike = strike;
    if (underline !== format.underline) change.underline = underline;
    if (fontColorChanged) change.fontColor = pickedColor(fontColor);
    if (locked !== format.locked) change.locked = locked;
    if (formulaHidden !== format.formulaHidden) change.formulaHidden = formulaHidden;
    if (fillChanged) {
      const color: FormatColor = pickedColor(fillColor);
      const none = fillColor.kind === 'none';
      if (pattern === 'solid' || pattern === 'none') {
        change.fill = none ? { pattern: 'none' } : { pattern: 'solid', color };
      } else {
        change.fill = { pattern, patternColor: pickedColor(patternColor), color };
      }
    }
    if (Object.keys(edges).length) {
      const border: BorderChange = {};
      for (const edge of ['top', 'bottom', 'left', 'right'] as const) {
        if (edges[edge]) border[edge] = edges[edge];
      }
      if (edges.insideHorizontal) border.insideHorizontal = edges.insideHorizontal;
      if (edges.insideVertical) border.insideVertical = edges.insideVertical;
      if (edges.diagonalUp || edges.diagonalDown) {
        const up = edges.diagonalUp;
        const down = edges.diagonalDown;
        const on = [up, down].find((one) => one && one.style !== 'none');
        border.diagonal = on ?? { style: 'none' };
        if (up) border.diagonalUp = up.style !== 'none';
        if (down) border.diagonalDown = down.style !== 'none';
      }
      change.border = border;
    }
    props.onApply(change, merged !== format.merged ? merged : null);
    props.onClose();
  };

  const numberTab = (
    <div className="cgs-dialog-split">
      <label className="cgs-dialog-field">
        <span>Category</span>
        <select
          size={12}
          data-testid="number-category"
          value={category}
          onChange={(event) => {
            const next = event.target.value as NumberCategory;
            setCategory(next);
            if (next === 'Date') setListed(DATE_CODES[0]!);
            if (next === 'Time') setListed(TIME_CODES[0]!);
            if (next === 'Fraction') setListed(FRACTION_CODES[0]!);
            if (next === 'Special') setListed(SPECIAL_CODES[0]!);
            if (next === 'Custom') setCustom(code);
            if (next === 'Number' || next === 'Currency' || next === 'Accounting') {
              setPlaces(2);
            }
          }}
        >
          {NUMBER_CATEGORIES.map((name) => (
            <option key={name}>{name}</option>
          ))}
        </select>
      </label>
      <div className="cgs-dialog-column">
        <p className="cgs-dialog-sample" data-testid="number-sample">
          <span>Sample</span>
          <strong>
            {sampleText === null ? 'That number format can’t be used.' : sampleText || ' '}
          </strong>
        </p>
        {(category === 'Number' ||
          category === 'Currency' ||
          category === 'Accounting' ||
          category === 'Percentage' ||
          category === 'Scientific') && (
          <NumberField
            label="Decimal places"
            testId="number-places"
            value={places}
            min={0}
            max={30}
            onChange={setPlaces}
          />
        )}
        {category === 'Number' && (
          <label className="cgs-dialog-check">
            <input
              type="checkbox"
              data-testid="number-grouping"
              checked={grouping}
              onChange={(event) => setGrouping(event.target.checked)}
            />
            Use 1000 Separator (,)
          </label>
        )}
        {(category === 'Currency' || category === 'Accounting') && (
          <label className="cgs-dialog-field">
            <span>Symbol</span>
            <select
              data-testid="number-symbol"
              value={symbol}
              onChange={(event) => setSymbol(event.target.value)}
            >
              {CURRENCIES.map((currency) => (
                <option key={currency.label} value={currency.symbol}>
                  {currency.label}
                </option>
              ))}
            </select>
          </label>
        )}
        {(category === 'Number' || category === 'Currency') && (
          <label className="cgs-dialog-field">
            <span>Negative numbers</span>
            <select
              size={4}
              data-testid="number-negative"
              value={negative}
              onChange={(event) => setNegative(Number(event.target.value))}
            >
              <option value={0}>-1234.10</option>
              <option value={1}>1234.10 in red</option>
              <option value={2}>(1234.10)</option>
              <option value={3}>(1234.10) in red</option>
            </select>
          </label>
        )}
        {(category === 'Date' ||
          category === 'Time' ||
          category === 'Fraction' ||
          category === 'Special') && (
          <label className="cgs-dialog-field">
            <span>Type</span>
            <select
              size={8}
              data-testid="number-type"
              value={listed}
              onChange={(event) => setListed(event.target.value)}
            >
              {(category === 'Date'
                ? DATE_CODES
                : category === 'Time'
                  ? TIME_CODES
                  : category === 'Fraction'
                    ? FRACTION_CODES
                    : SPECIAL_CODES
              ).map((one) => (
                <option key={one} value={one}>
                  {one}
                </option>
              ))}
            </select>
          </label>
        )}
        {category === 'Custom' && (
          <>
            <label className="cgs-dialog-field">
              <span>Type</span>
              <input
                data-testid="number-custom"
                value={custom}
                onChange={(event) => setCustom(event.target.value)}
              />
            </label>
            <select
              size={7}
              aria-label="Number format codes"
              value={custom}
              onChange={(event) => setCustom(event.target.value)}
            >
              {[...new Set([...BUILT_IN_CODES, ...props.codes])].map((one) => (
                <option key={one} value={one}>
                  {one}
                </option>
              ))}
            </select>
          </>
        )}
        {category === 'General' && <p>General format cells have no specific number format.</p>}
        {category === 'Text' && <p>Text format cells show what is in them, as it was typed.</p>}
      </div>
    </div>
  );

  const alignmentTab = (
    <div className="cgs-dialog-split">
      <div className="cgs-dialog-column">
        <label className="cgs-dialog-field">
          <span>Horizontal</span>
          <select
            data-testid="align-horizontal"
            value={horizontal}
            onChange={(event) => setHorizontal(event.target.value)}
          >
            {HORIZONTAL.map(([value, text]) => (
              <option key={value} value={value}>
                {text}
              </option>
            ))}
          </select>
        </label>
        <NumberField
          label="Indent"
          testId="align-indent"
          value={indent}
          min={0}
          max={250}
          onChange={setIndent}
        />
        <label className="cgs-dialog-field">
          <span>Vertical</span>
          <select
            data-testid="align-vertical"
            value={vertical}
            onChange={(event) => setVertical(event.target.value)}
          >
            {VERTICAL.map(([value, text]) => (
              <option key={value} value={value}>
                {text}
              </option>
            ))}
          </select>
        </label>
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="align-wrap"
            checked={wrap}
            onChange={(event) => setWrap(event.target.checked)}
          />
          Wrap text
        </label>
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="align-shrink"
            checked={shrink}
            onChange={(event) => setShrink(event.target.checked)}
          />
          Shrink to fit
        </label>
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="align-merge"
            checked={merged}
            onChange={(event) => setMerged(event.target.checked)}
          />
          Merge cells
        </label>
      </div>
      <div className="cgs-dialog-column">
        <NumberField
          label="Orientation, in degrees"
          testId="align-rotation"
          value={rotation === 255 ? 0 : degrees}
          min={-90}
          max={90}
          onChange={(value) => setRotation(value < 0 ? 90 - value : value)}
        />
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="align-stacked"
            checked={rotation === 255}
            onChange={(event) => setRotation(event.target.checked ? 255 : 0)}
          />
          Vertical text
        </label>
        <label className="cgs-dialog-field">
          <span>Text direction</span>
          <select
            data-testid="align-direction"
            value={readingOrder}
            onChange={(event) => setReadingOrder(Number(event.target.value))}
          >
            <option value={0}>Context</option>
            <option value={1}>Left-to-Right</option>
            <option value={2}>Right-to-Left</option>
          </select>
        </label>
      </div>
    </div>
  );

  const fontTab = (
    <div className="cgs-dialog-column">
      <div className="cgs-dialog-row">
        <label className="cgs-dialog-field cgs-dialog-grow">
          <span>Font</span>
          <input
            list="cgs-dialog-fonts"
            data-testid="font-dialog-name"
            value={fontName}
            onChange={(event) => setFontName(event.target.value)}
          />
          <datalist id="cgs-dialog-fonts">
            {FONT_NAMES.map((name) => (
              <option key={name} value={name} />
            ))}
          </datalist>
        </label>
        <label className="cgs-dialog-field">
          <span>Font style</span>
          <select
            data-testid="font-dialog-style"
            value={(bold ? 'b' : '') + (italic ? 'i' : '')}
            onChange={(event) => {
              setBold(event.target.value.includes('b'));
              setItalic(event.target.value.includes('i'));
            }}
          >
            <option value="">Regular</option>
            <option value="i">Italic</option>
            <option value="b">Bold</option>
            <option value="bi">Bold Italic</option>
          </select>
        </label>
        <label className="cgs-dialog-field">
          <span>Size</span>
          <input
            list="cgs-dialog-sizes"
            data-testid="font-dialog-size"
            value={fontSize}
            onChange={(event) => {
              const next = Number(event.target.value);
              if (next >= 1 && next <= 409) setFontSize(next);
            }}
          />
          <datalist id="cgs-dialog-sizes">
            {FONT_SIZES.map((size) => (
              <option key={size} value={size} />
            ))}
          </datalist>
        </label>
      </div>
      <div className="cgs-dialog-row">
        <label className="cgs-dialog-field">
          <span>Underline</span>
          <select
            data-testid="font-dialog-underline"
            value={underline}
            onChange={(event) => setUnderline(event.target.value as CellFormat['underline'])}
          >
            {UNDERLINES.map(([value, text]) => (
              <option key={value} value={value}>
                {text}
              </option>
            ))}
          </select>
        </label>
        <ColorField
          label="Color"
          testId="font-dialog-color"
          value={fontColor}
          theme={theme}
          automatic
          onChange={(next) => {
            setFontColor(next);
            setFontColorChanged(true);
          }}
        />
        <label className="cgs-dialog-check">
          <input
            type="checkbox"
            data-testid="font-dialog-strike"
            checked={strike}
            onChange={(event) => setStrike(event.target.checked)}
          />
          Strikethrough
        </label>
      </div>
      <div
        className="cgs-dialog-preview"
        data-testid="font-dialog-preview"
        style={{
          fontFamily: `"${fontName}", var(--cg-font-family)`,
          fontSize: Math.min(fontSize, 28) * (96 / 72),
          fontWeight: bold ? 700 : 400,
          fontStyle: italic ? 'italic' : 'normal',
          textDecoration:
            (underline === 'none' ? '' : 'underline ') + (strike ? 'line-through' : '') || 'none',
          color: 'color' in fontColor ? fontColor.color : undefined,
        }}
      >
        AaBbCcYyZz
      </div>
    </div>
  );

  const edgeButton = (edge: Edge, text: string) => (
    <button
      type="button"
      className={'cgs-edge' + (edgeOn(edge) ? ' cgs-edge-on' : '')}
      aria-pressed={edgeOn(edge)}
      data-testid={'border-' + edge}
      onClick={() => toggleEdge(edge)}
    >
      {text}
    </button>
  );
  const borderTab = (
    <div className="cgs-dialog-split">
      <div className="cgs-dialog-column">
        <label className="cgs-dialog-field">
          <span>Line style</span>
          <select
            size={8}
            data-testid="border-style"
            value={lineStyle}
            onChange={(event) => setLineStyle(event.target.value)}
          >
            {LINE_STYLES.map((style) => (
              <option key={style}>{style}</option>
            ))}
          </select>
        </label>
        <ColorField
          label="Color"
          testId="border-color"
          value={lineColor}
          theme={theme}
          automatic
          onChange={setLineColor}
        />
      </div>
      <div className="cgs-dialog-column">
        <div className="cgs-dialog-row">
          <CgButton data-testid="border-none" onClick={() => preset('none')}>
            None
          </CgButton>
          <CgButton data-testid="border-outline" onClick={() => preset('outline')}>
            Outline
          </CgButton>
          <CgButton data-testid="border-inside" onClick={() => preset('inside')}>
            Inside
          </CgButton>
        </div>
        <div className="cgs-dialog-edges">
          {edgeButton('top', 'Top')}
          {edgeButton('insideHorizontal', 'Inside H')}
          {edgeButton('bottom', 'Bottom')}
          {edgeButton('left', 'Left')}
          {edgeButton('insideVertical', 'Inside V')}
          {edgeButton('right', 'Right')}
          {edgeButton('diagonalDown', 'Diagonal \\')}
          {edgeButton('diagonalUp', 'Diagonal /')}
        </div>
      </div>
    </div>
  );

  const fillTab = (
    <div className="cgs-dialog-split">
      <div className="cgs-dialog-column">
        <span>Background color</span>
        <CgColorPicker
          value={fillColor}
          themeColors={theme}
          allowNone
          allowCustomColor
          labels={{ none: 'No Color' }}
          data-testid="fill-color"
          onValueChange={(next) => {
            setFillColor(next);
            setFillChanged(true);
            if (next.kind !== 'none' && pattern === 'none') setPattern('solid');
          }}
        />
      </div>
      <div className="cgs-dialog-column">
        <ColorField
          label="Pattern color"
          testId="fill-pattern-color"
          value={patternColor}
          theme={theme}
          automatic
          onChange={(next) => {
            setPatternColor(next);
            setFillChanged(true);
          }}
        />
        <label className="cgs-dialog-field">
          <span>Pattern style</span>
          <select
            data-testid="fill-pattern"
            value={pattern === 'none' ? 'solid' : pattern}
            onChange={(event) => {
              setPattern(event.target.value);
              setFillChanged(true);
            }}
          >
            {PATTERNS.map(([value, text]) => (
              <option key={value} value={value}>
                {text}
              </option>
            ))}
          </select>
        </label>
        <div
          className="cgs-dialog-preview"
          data-testid="fill-preview"
          style={{ background: 'color' in fillColor ? fillColor.color : 'transparent' }}
        >
          Sample
        </div>
      </div>
    </div>
  );

  // Excel 2016's own words under its two boxes.
  const protectionTab = (
    <div className="cgs-dialog-column" data-testid="format-protection">
      <label className="cgs-dialog-check">
        <input
          type="checkbox"
          data-testid="format-locked"
          checked={locked}
          onChange={(event) => setLocked(event.target.checked)}
        />
        Locked
      </label>
      <label className="cgs-dialog-check">
        <input
          type="checkbox"
          data-testid="format-hidden"
          checked={formulaHidden}
          onChange={(event) => setFormulaHidden(event.target.checked)}
        />
        Hidden
      </label>
      <p className="cgs-dialog-message">
        Locking cells or hiding formulas has no effect until you protect the worksheet (Review tab,
        Changes group, Protect Sheet button).
      </p>
    </div>
  );

  const tabs = [
    { key: 'number', text: 'Number', content: numberTab },
    { key: 'alignment', text: 'Alignment', content: alignmentTab },
    { key: 'font', text: 'Font', content: fontTab },
    { key: 'border', text: 'Border', content: borderTab },
    { key: 'fill', text: 'Fill', content: fillTab },
    { key: 'protection', text: 'Protection', content: protectionTab },
  ];

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Format Cells"
      width={560}
      allowDrag
      data-testid="format-cells"
      renderBody={() => (
        <CgTabs
          tabs={tabs}
          activeKey={tab}
          onActiveKeyChange={(key) => key && setTab(key)}
          size="small"
          contentMode="active-only"
          ariaLabel="Format Cells"
        />
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            intent="primary"
            data-testid="format-ok"
            disabled={sampleText === null}
            onClick={apply}
          >
            OK
          </CgButton>
          <CgButton data-testid="format-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

/** How a cell's fill would read as a colour to show, for tests and the ribbon. */
export const fillSwatch = (format: CellFormat) => swatchOf(format.fill.color);
