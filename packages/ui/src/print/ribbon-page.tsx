// The Page Layout tab, as Excel 2016 has it: Page Setup (margins, orientation, size, print area,
// breaks, print titles), Scale to Fit and Sheet Options. Each command is one engine command.
import type { CgRibbonItem, CgRibbonTab } from '@cashgear/ui';
import type { PageMargins, PageSetupInfo, PageSpec } from '@cgs/engine-client';

export interface PageLayoutState {
  hasBook: boolean;
  /** Page setup of the active sheet, once read. */
  setup: PageSetupInfo | null;
  showGrid: boolean;
  showHeadings: boolean;
  /** Whether the active cell is past the first row and column, where a break can go. */
  canBreak: boolean;
}

export interface PageLayoutActions {
  change(spec: PageSpec): void;
  openPageSetup(tab?: PageSetupTab): void;
  printArea(action: 'set' | 'add' | 'clear'): void;
  pageBreak(action: 'insert' | 'remove' | 'reset'): void;
  setView(flags: { grid?: boolean; headings?: boolean }): void;
}

export type PageSetupTab = 'page' | 'margins' | 'header' | 'sheet';

/** Excel's margin presets, in inches. */
export const MARGIN_PRESETS: { name: string; text: string; margins: PageMargins }[] = [
  {
    name: 'normal',
    text: 'Normal',
    margins: { top: 0.75, bottom: 0.75, left: 0.7, right: 0.7, header: 0.3, footer: 0.3 },
  },
  {
    name: 'wide',
    text: 'Wide',
    margins: { top: 1, bottom: 1, left: 1, right: 1, header: 0.5, footer: 0.5 },
  },
  {
    name: 'narrow',
    text: 'Narrow',
    margins: { top: 0.75, bottom: 0.75, left: 0.25, right: 0.25, header: 0.3, footer: 0.3 },
  },
];

const same = (a: PageMargins, b: PageMargins) =>
  (Object.keys(a) as (keyof PageMargins)[]).every((key) => Math.abs(a[key] - b[key]) < 1e-6);

/** The preset these margins are, if any. */
export function marginPreset(margins: PageMargins | undefined): string | null {
  if (!margins) return null;
  return MARGIN_PRESETS.find((preset) => same(preset.margins, margins))?.name ?? null;
}

const inches = (value: number) => `${value.toFixed(2).replace(/\.?0+$/, '')}"`;

/** Pages for Width and Height: Automatic, or 1 to 9 pages. */
const FIT_CHOICES = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

export function pageLayoutTab(state: PageLayoutState, actions: PageLayoutActions): CgRibbonTab {
  const off = !state.hasBook || !state.setup;
  const setup = state.setup;
  const landscape = setup?.orientation === 'landscape';
  const preset = marginPreset(setup?.margins);
  const fit = setup?.fitToPage ?? false;
  const fitWidth = fit ? (setup?.fitWidth ?? 1) : 0;
  const fitHeight = fit ? (setup?.fitHeight ?? 1) : 0;
  const fitSelect = (
    label: string,
    testId: string,
    value: number,
    key: 'fitWidth' | 'fitHeight',
  ) => (
    <label className="cgs-ribbon-labelled">
      <span>{label}</span>
      <select
        className="cgs-ribbon-select"
        aria-label={label}
        data-testid={testId}
        disabled={off}
        value={value}
        onChange={(event) => {
          const pages = Number(event.target.value);
          const other = key === 'fitWidth' ? fitHeight : fitWidth;
          if (!pages && !other) {
            actions.change({ fitToPage: false, scale: 100 });
            return;
          }
          actions.change({
            fitToPage: true,
            fitWidth: key === 'fitWidth' ? pages : other,
            fitHeight: key === 'fitHeight' ? pages : other,
          });
        }}
      >
        {FIT_CHOICES.map((pages) => (
          <option key={pages} value={pages}>
            {pages === 0 ? 'Automatic' : pages === 1 ? '1 page' : `${pages} pages`}
          </option>
        ))}
      </select>
    </label>
  );
  const margins: CgRibbonItem[] = [
    ...MARGIN_PRESETS.map((entry) => ({
      name: 'margins-' + entry.name,
      text: `${entry.text}  (Top ${inches(entry.margins.top)}, Left ${inches(entry.margins.left)})`,
      checked: preset === entry.name,
      onClick: () => actions.change({ margins: entry.margins }),
    })),
    {
      name: 'margins-custom',
      text: 'Custom Margins…',
      onClick: () => actions.openPageSetup('margins'),
    },
  ];
  const papers: CgRibbonItem[] = [
    ...(setup?.papers ?? []).map((paper) => ({
      name: 'size-' + paper.code,
      text: `${paper.name}  ${(paper.width / 72).toFixed(2)}" x ${(paper.height / 72).toFixed(2)}"`,
      checked: setup?.paper === paper.code,
      onClick: () => actions.change({ paper: paper.code }),
    })),
    { name: 'size-more', text: 'More Paper Sizes…', onClick: () => actions.openPageSetup('page') },
  ];
  const hasArea = Boolean(setup?.printArea);
  return {
    key: 'page-layout',
    text: 'Page Layout',
    groups: [
      {
        key: 'page-setup',
        text: 'Page Setup',
        icon: 'settings',
        onLauncherClick: () => actions.openPageSetup('page'),
        launcherLabel: 'Page Setup',
        items: [
          {
            name: 'margins',
            text: 'Margins',
            icon: 'border-outside',
            size: 'large',
            disabled: off,
            children: margins,
          },
          {
            name: 'orientation',
            text: 'Orientation',
            icon: 'text-rotate',
            size: 'large',
            disabled: off,
            children: [
              {
                name: 'orientation-portrait',
                text: 'Portrait',
                checked: !landscape,
                onClick: () => actions.change({ orientation: 'portrait' }),
              },
              {
                name: 'orientation-landscape',
                text: 'Landscape',
                checked: landscape,
                onClick: () => actions.change({ orientation: 'landscape' }),
              },
            ],
          },
          {
            name: 'size',
            text: 'Size',
            icon: 'autofit-width',
            size: 'large',
            disabled: off,
            children: papers,
          },
          {
            name: 'print-area',
            text: 'Print Area',
            icon: 'table',
            size: 'large',
            disabled: off,
            children: [
              {
                name: 'print-area-set',
                text: 'Set Print Area',
                onClick: () => actions.printArea('set'),
              },
              ...(hasArea
                ? [
                    {
                      name: 'print-area-add',
                      text: 'Add to Print Area',
                      onClick: () => actions.printArea('add'),
                    },
                  ]
                : []),
              {
                name: 'print-area-clear',
                text: 'Clear Print Area',
                disabled: !hasArea,
                onClick: () => actions.printArea('clear'),
              },
            ],
          },
          {
            name: 'breaks',
            text: 'Breaks',
            icon: 'row-insert',
            size: 'large',
            disabled: off,
            children: [
              {
                name: 'break-insert',
                text: 'Insert Page Break',
                disabled: !state.canBreak,
                onClick: () => actions.pageBreak('insert'),
              },
              {
                name: 'break-remove',
                text: 'Remove Page Break',
                onClick: () => actions.pageBreak('remove'),
              },
              {
                name: 'break-reset',
                text: 'Reset All Page Breaks',
                onClick: () => actions.pageBreak('reset'),
              },
            ],
          },
          {
            name: 'print-titles',
            text: 'Print Titles',
            icon: 'freeze-top-row',
            size: 'large',
            disabled: off,
            onClick: () => actions.openPageSetup('sheet'),
          },
        ],
      },
      {
        key: 'scale-to-fit',
        text: 'Scale to Fit',
        icon: 'zoom-in',
        onLauncherClick: () => actions.openPageSetup('page'),
        launcherLabel: 'Page Setup',
        items: [
          {
            name: 'fit-width',
            ownsKeyboard: true,
            render: () => fitSelect('Width:', 'fit-width', fitWidth, 'fitWidth'),
          },
          {
            name: 'fit-height',
            ownsKeyboard: true,
            render: () => fitSelect('Height:', 'fit-height', fitHeight, 'fitHeight'),
          },
          {
            name: 'fit-scale',
            ownsKeyboard: true,
            render: () => (
              <label className="cgs-ribbon-labelled">
                <span>Scale:</span>
                <input
                  className="cgs-ribbon-number"
                  type="number"
                  min={10}
                  max={400}
                  aria-label="Scale"
                  data-testid="fit-scale"
                  // Excel greys out Scale while it fits to pages.
                  disabled={off || fit}
                  key={setup?.scale ?? 100}
                  defaultValue={setup?.scale ?? 100}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') (event.target as HTMLInputElement).blur();
                  }}
                  onBlur={(event) => {
                    const scale = Math.round(Number(event.target.value));
                    if (scale >= 10 && scale <= 400 && scale !== setup?.scale) {
                      actions.change({ scale });
                    }
                  }}
                />
                <span>%</span>
              </label>
            ),
          },
        ],
      },
      {
        key: 'sheet-options',
        text: 'Sheet Options',
        icon: 'gridlines',
        onLauncherClick: () => actions.openPageSetup('sheet'),
        launcherLabel: 'Page Setup',
        items: [
          {
            name: 'grid-view',
            text: 'Gridlines: View',
            disabled: !state.hasBook,
            checked: state.showGrid,
            onCheckedChange: (on) => actions.setView({ grid: on }),
          },
          {
            name: 'grid-print',
            text: 'Gridlines: Print',
            disabled: off,
            checked: setup?.gridlines ?? false,
            onCheckedChange: (on) => actions.change({ gridlines: on }),
          },
          {
            name: 'headings-view',
            text: 'Headings: View',
            disabled: !state.hasBook,
            checked: state.showHeadings,
            onCheckedChange: (on) => actions.setView({ headings: on }),
          },
          {
            name: 'headings-print',
            text: 'Headings: Print',
            disabled: off,
            checked: setup?.headings ?? false,
            onCheckedChange: (on) => actions.change({ headings: on }),
          },
        ],
      },
    ],
  };
}
