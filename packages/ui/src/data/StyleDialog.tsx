// Excel 2016's Style dialog, for New Cell Style and Modify: a name, what the style includes with
// each part described, and for Modify, Format… to change the style's look in Format Cells.
import { useEffect, useState } from 'react';
import { CgButton, CgWindow } from '@cashgear/ui';
import type { CellFormat, StyleIncludes } from '@cgs/engine-client';

export const ALL_INCLUDED: StyleIncludes = {
  number: true,
  alignment: true,
  font: true,
  border: true,
  fill: true,
  protection: true,
};

const PARTS: { key: keyof StyleIncludes; label: string }[] = [
  { key: 'number', label: 'Number' },
  { key: 'alignment', label: 'Alignment' },
  { key: 'font', label: 'Font' },
  { key: 'border', label: 'Border' },
  { key: 'fill', label: 'Fill' },
  { key: 'protection', label: 'Protection' },
];

const HORIZONTAL: Record<string, string> = {
  general: 'General',
  left: 'Left',
  center: 'Center',
  right: 'Right',
  fill: 'Fill',
  justify: 'Justify',
  centerContinuous: 'Center Across Selection',
  distributed: 'Distributed',
};

/** What Excel's Style dialog says beside each part, from the format. */
export function describePart(key: keyof StyleIncludes, format: CellFormat | null): string {
  if (!format) return '';
  switch (key) {
    case 'number':
      return format.numberFormat;
    case 'alignment': {
      const vertical = format.vertical === 'center' ? 'Center' : format.vertical;
      const words = [HORIZONTAL[format.horizontal] ?? format.horizontal];
      words.push(vertical === 'bottom' ? 'Bottom Aligned' : `${capital(vertical)} Aligned`);
      if (format.wrap) words.push('Wrap');
      return words.join(', ');
    }
    case 'font': {
      const words = [`${format.fontName} ${format.fontSize}`];
      if (format.bold) words.push('Bold');
      if (format.italic) words.push('Italic');
      return words.join(', ');
    }
    case 'border': {
      const sides = (['left', 'right', 'top', 'bottom'] as const).filter(
        (side) => format.border[side].style !== 'none',
      );
      return sides.length ? sides.map(capital).join(', ') + ' Borders' : 'No Borders';
    }
    case 'fill':
      return format.fill.pattern === 'none' ? 'No Shading' : 'Shaded';
    case 'protection':
      return format.locked ? 'Locked' : 'Unlocked';
  }
}

function capital(text: string) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export interface StyleDialogProps {
  open: boolean;
  mode: 'new' | 'modify';
  name: string;
  includes: StyleIncludes;
  /** The look described: the example cell's for New, the style's for Modify. */
  format: CellFormat | null;
  /** Modify's Format…, which changes `format` through Format Cells. */
  onFormat?: () => void;
  onOk: (name: string, includes: StyleIncludes) => void;
  onClose: () => void;
}

export function StyleDialog(props: StyleDialogProps) {
  const { open, mode, format } = props;
  const [name, setName] = useState(props.name);
  const [includes, setIncludes] = useState(props.includes);

  useEffect(() => {
    if (!open) return;
    setName(props.name);
    setIncludes(props.includes);
    // Start again each time the dialog opens.
  }, [open]);

  const ok = () => {
    if (!name.trim()) return;
    props.onOk(name.trim(), includes);
  };

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Style"
      width={420}
      allowDrag
      data-testid="style-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <label className="cgs-dialog-row">
            <span>Style name:</span>
            <input
              data-testid="style-name"
              value={name}
              readOnly={mode === 'modify'}
              autoFocus={mode === 'new'}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') ok();
              }}
            />
            {mode === 'modify' && props.onFormat && (
              <CgButton data-testid="style-format" onClick={props.onFormat}>
                Format…
              </CgButton>
            )}
          </label>
          <span>{mode === 'new' ? 'Style Includes (By Example)' : 'Style Includes'}</span>
          {PARTS.map(({ key, label }) => (
            <label key={key} className="cgs-dialog-check">
              <input
                type="checkbox"
                data-testid={'style-includes-' + key}
                checked={includes[key]}
                onChange={(event) => setIncludes({ ...includes, [key]: event.target.checked })}
              />
              <span style={{ minWidth: 80 }}>{label}</span>
              <span className="cgs-dialog-note">{describePart(key, format)}</span>
            </label>
          ))}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="style-ok" disabled={!name.trim()} onClick={ok}>
            OK
          </CgButton>
          <CgButton data-testid="style-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}
