import { useEffect, useState } from 'react';
import { CgButton, CgListBox, CgWindow } from '@cashgear/ui';

export interface HiddenSheet {
  index: number;
  name: string;
}

export interface UnhideDialogProps {
  open: boolean;
  /** Hidden sheets in tab order. Very hidden ones aren't offered, as in Excel. */
  sheets: ReadonlyArray<HiddenSheet>;
  onUnhide: (sheet: number) => void;
  onClose: () => void;
}

/** Excel 2016's Unhide: one hidden sheet to bring back. */
export function UnhideDialog(props: UnhideDialogProps) {
  const { open, sheets } = props;
  const [chosen, setChosen] = useState<HiddenSheet | null>(sheets[0] ?? null);

  useEffect(() => {
    if (open) setChosen(sheets[0] ?? null);
    // Start again each time the dialog opens.
  }, [open]);

  const ok = () => {
    if (!chosen) return;
    props.onUnhide(chosen.index);
    props.onClose();
  };

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Unhide"
      width={300}
      allowDrag
      data-testid="unhide-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <span>Unhide sheet:</span>
          <CgListBox
            data-testid="unhide-list"
            items={sheets}
            getItemKey={(item) => item.index}
            getItemLabel={(item) => item.name}
            selectionMode="single"
            value={chosen ? [chosen] : []}
            onValueChange={(value) => setChosen(value[0] ?? null)}
            onItemActivate={(details) => {
              props.onUnhide(details.item.index);
              props.onClose();
            }}
            height={140}
            fullWidth
            autoFocus
          />
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="unhide-ok" disabled={!chosen} onClick={ok}>
            OK
          </CgButton>
          <CgButton data-testid="unhide-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}
