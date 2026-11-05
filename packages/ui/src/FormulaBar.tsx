import { useEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent } from 'react';
import type { EditSession } from './editing';

interface FormulaBarProps {
  session: EditSession;
  /** The selection's address. */
  name: string;
  /** The active cell's formula, or its constant. */
  text: string;
  /** Starts editing here. */
  onBegin: () => void;
  /** Shared with the cell editor: everything a key can do while editing. */
  onEditorKey: (event: KeyboardEvent<HTMLTextAreaElement>) => void;
  /** Enter in the name box: goes to what it says. */
  onGoTo: (text: string) => void;
  /** The grid takes the keyboard back. */
  onLeave: () => void;
  readOnly: boolean;
}

/** The name box and the formula bar, which is the cell editor's other half. */
export function FormulaBar({
  session,
  name,
  text,
  onBegin,
  onEditorKey,
  onGoTo,
  onLeave,
  readOnly,
}: FormulaBarProps) {
  const edit = useSyncExternalStore(session.subscribe, session.snapshot);
  const [nameDraft, setNameDraft] = useState<string | null>(null);
  const input = useRef<HTMLTextAreaElement>(null);

  // A click on the sheet, or a key handled outside the box, puts the caret somewhere. Otherwise the
  // box is left alone, as it may be holding a selection of text.
  useEffect(() => {
    const element = input.current;
    if (!element || !edit.active || edit.place !== 'bar') return;
    if (document.activeElement !== element) element.focus();
    element.setSelectionRange(edit.caret, edit.caret);
  }, [edit.active, edit.place, edit.placed]);

  return (
    <div className="cgs-formula-bar">
      <input
        className="cgs-name-box"
        aria-label="Name box"
        data-testid="name-box"
        value={nameDraft ?? name}
        spellCheck={false}
        onFocus={(event) => {
          setNameDraft(name);
          event.target.select();
        }}
        onBlur={() => setNameDraft(null)}
        onChange={(event) => setNameDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            const target = nameDraft;
            setNameDraft(null);
            if (target) onGoTo(target);
          } else if (event.key === 'Escape') {
            event.preventDefault();
            setNameDraft(null);
            onLeave();
          }
        }}
      />
      <span className="cgs-formula-mark" aria-hidden="true">
        fx
      </span>
      {/* A text area, as an input would take the line breaks out of what's being edited. */}
      <textarea
        ref={input}
        rows={1}
        className="cgs-formula-text"
        aria-label="Formula bar"
        data-testid="formula-bar"
        dir="auto"
        spellCheck={false}
        readOnly={readOnly}
        value={edit.active ? edit.text : text}
        onMouseDown={() => {
          if (!edit.active && !readOnly) onBegin();
          else if (edit.active) session.setPlace('bar');
        }}
        onFocus={() => {
          if (!edit.active && !readOnly) onBegin();
        }}
        onChange={(event) => {
          session.setText(event.target.value, event.target.selectionStart ?? 0);
        }}
        onSelect={(event) => {
          const element = event.currentTarget;
          if (edit.active && edit.place === 'bar') {
            session.setText(element.value, element.selectionStart ?? 0);
          }
        }}
        onKeyDown={onEditorKey}
      />
    </div>
  );
}
