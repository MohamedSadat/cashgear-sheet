// Format as Table and Cell Styles: Excel's galleries, drawn from what the engine says each style
// looks like. Choosing one is a command; nothing here reads cells.
import { CgGallery, type CgGalleryItem } from '@cashgear/ui';
import type { CellStyleInfo, TableStyleInfo } from '@cgs/engine-client';

/** Excel's names for its built-in table styles, as the gallery's tooltips give them. */
export function tableStyleLabel(name: string) {
  const match = /^TableStyle(Light|Medium|Dark)(\d+)$/.exec(name);
  if (!match) return name;
  return `${match[1]} ${match[2]}`;
}

function TablePreview({ style }: { style: TableStyleInfo }) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(5, 1fr)',
        width: 61,
        height: 46,
        background: '#ffffff',
      }}
      aria-hidden
    >
      {style.cells.flatMap((row, r) =>
        row.map(([fill, , , top, bottom, left, right], c) => (
          <div
            key={`${r}-${c}`}
            style={{
              background: fill ?? 'transparent',
              borderTop: top ? `1px solid ${top}` : undefined,
              borderBottom: bottom ? `1px solid ${bottom}` : undefined,
              borderLeft: left ? `1px solid ${left}` : undefined,
              borderRight: right ? `1px solid ${right}` : undefined,
              boxSizing: 'border-box',
            }}
          />
        )),
      )}
    </div>
  );
}

export interface TableStyleGalleryProps {
  styles: TableStyleInfo[];
  /** The current table's style, to show as chosen. */
  value?: string | null;
  disabled?: boolean;
  /** Shown on the button: Format as Table on Home, Table Styles on Table Design. */
  label: string;
  onPick: (name: string) => void;
}

export function TableStyleGallery({
  styles,
  value,
  disabled,
  label,
  onPick,
}: TableStyleGalleryProps) {
  const groups = ['Custom', 'Light', 'Medium', 'Dark']
    .filter((group) => styles.some((style) => style.group === group))
    .map((group) => ({ key: group, text: group }));
  const items: CgGalleryItem<TableStyleInfo>[] = styles.map((style) => ({
    key: style.name,
    text: tableStyleLabel(style.name),
    group: style.group,
    data: style,
    preview: <TablePreview style={style} />,
  }));
  return (
    <CgGallery
      data-testid="table-style-gallery"
      display="dropdown"
      triggerText={label}
      triggerIcon="table-style"
      items={items}
      groups={groups}
      value={value ?? null}
      columns={7}
      itemWidth={69}
      itemHeight={54}
      maxHeight={420}
      disabled={disabled || !styles.length}
      ariaLabel={label}
      onValueChange={(key) => onPick(key)}
    />
  );
}

function CellStylePreview({ style }: { style: CellStyleInfo }) {
  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        alignItems: 'center',
        padding: '0 4px',
        boxSizing: 'border-box',
        background: style.fill ?? '#ffffff',
        color: style.color ?? '#000000',
        fontFamily: `"${style.font}", sans-serif`,
        fontSize: Math.min(15, Math.max(11, style.size)),
        fontWeight: style.bold ? 700 : 400,
        fontStyle: style.italic ? 'italic' : 'normal',
        border: style.boxed ? '1px solid #7f7f7f' : undefined,
        borderBottom:
          style.bottom && !style.boxed
            ? `${style.bottom === 'thick' ? 3 : style.bottom === 'medium' ? 2 : 1}px ${
                style.bottom === 'double' ? 'double' : 'solid'
              } ${style.bottomColor ?? '#000000'}`
            : undefined,
        overflow: 'hidden',
        whiteSpace: 'nowrap',
      }}
    >
      {style.name}
    </div>
  );
}

export interface CellStyleGalleryProps {
  groups: { group: string; styles: CellStyleInfo[] }[];
  disabled?: boolean;
  onPick: (name: string) => void;
}

export function CellStyleGallery({ groups, disabled, onPick }: CellStyleGalleryProps) {
  const items: CgGalleryItem<CellStyleInfo>[] = groups.flatMap(({ group, styles }) =>
    styles.map((style) => ({
      key: style.name,
      text: style.name,
      group,
      data: style,
      preview: <CellStylePreview style={style} />,
    })),
  );
  return (
    <CgGallery
      data-testid="cell-style-gallery"
      display="dropdown"
      triggerText="Cell Styles"
      triggerIcon="cell-style"
      items={items}
      groups={groups.map(({ group }) => ({ key: group, text: group }))}
      value={null}
      columns={6}
      itemWidth={104}
      itemHeight={26}
      maxHeight={460}
      disabled={disabled || !items.length}
      ariaLabel="Cell Styles"
      onValueChange={(key) => onPick(key)}
    />
  );
}
