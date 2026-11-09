import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
} from 'react';
import type { EditSession, RunStyle } from './editing';
import { nameAtCaret } from './formula-text';
import { RichEditor } from './RichEditor';

export interface CellEditorHandle {
  /** The element to focus so that the next key lands in it, even from an input method. */
  focus(): void;
  element(): HTMLTextAreaElement | null;
}

interface CellEditorProps {
  session: EditSession;
  /** Every function the engine knows, and the workbook's names. */
  names: string[];
  onKey: (event: KeyboardEvent<HTMLElement>) => void;
  /** The sheet's zoom as a factor, for runs' own sizes. */
  zoom?: number;
  /** Autocomplete is open, and takes Tab and the up and down arrows for itself. */
  completionRef: React.MutableRefObject<CompletionControl | null>;
}

export interface CompletionControl {
  /** Handles a key if the list is open. Returns whether it did. */
  key(event: KeyboardEvent<HTMLElement>): boolean;
}

const MAX_SHOWN = 8;

/**
 * A DOM overlay for typing into a cell, so that input methods and bidirectional text work as they
 * do anywhere. It stays in the page while idle, out of sight, so that focusing it before a key's
 * default action sends that key here. The grid's controller keeps it placed over its cell.
 */
export const CellEditor = forwardRef<CellEditorHandle, CellEditorProps>(function CellEditor(
  { session, names, onKey, completionRef, zoom = 1 },
  ref,
) {
  const edit = useSyncExternalStore(session.subscribe, session.snapshot);
  const area = useRef<HTMLTextAreaElement>(null);
  const [choice, setChoice] = useState(0);
  const [closedAt, setClosedAt] = useState<string | null>(null);

  useImperativeHandle(
    ref,
    () => ({
      focus: () => area.current?.focus({ preventScroll: true }),
      element: () => area.current,
    }),
    [],
  );

  // A click on the sheet, or a key handled outside the box, puts the caret somewhere. Otherwise the
  // box is left alone, as it may be holding a selection of text.
  useEffect(() => {
    const element = area.current;
    if (!element || !edit.active || edit.place !== 'cell') return;
    if (document.activeElement !== element) element.focus({ preventScroll: true });
    element.setSelectionRange(edit.caret, edit.caret);
  }, [edit.active, edit.place, edit.placed]);

  const shown = edit.active;
  const rich = edit.active && edit.place === 'cell' && edit.rich !== null;

  /** Ctrl+B, Ctrl+I and Ctrl+U on selected characters format them, as in Excel. */
  const formatKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (!ctrl || event.altKey || !['b', 'i', 'u'].includes(key)) return false;
    const toggle = (style: RunStyle): RunStyle => {
      const next = { ...style };
      if (key === 'u') {
        if (next.underline) delete next.underline;
        else next.underline = 'single';
      } else {
        const name = key === 'b' ? 'bold' : 'italic';
        if (next[name]) delete next[name];
        else next[name] = true;
      }
      return next;
    };
    if (!session.formatSelection(toggle)) return false;
    event.preventDefault();
    return true;
  };
  const word = edit.active ? nameAtCaret(edit.text, edit.caret) : null;
  const matches = useMemo(() => {
    if (!word || closedAt === edit.text) return [];
    const prefix = word.prefix.toUpperCase();
    const found = names.filter((name) => name.toUpperCase().startsWith(prefix));
    // A name that is already whole needs no help.
    if (found.length === 1 && found[0]!.toUpperCase() === prefix) return [];
    return found.slice(0, MAX_SHOWN);
  }, [names, word, closedAt, edit.text]);
  const selected = Math.min(choice, Math.max(matches.length - 1, 0));

  useEffect(() => {
    setChoice(0);
  }, [word?.prefix]);

  completionRef.current = {
    key(event) {
      if (!matches.length) return false;
      switch (event.key) {
        case 'ArrowDown':
          setChoice((selected + 1) % matches.length);
          break;
        case 'ArrowUp':
          setChoice((selected - 1 + matches.length) % matches.length);
          break;
        case 'Tab':
          session.complete(matches[selected]!);
          break;
        case 'Escape':
          setClosedAt(edit.text);
          break;
        default:
          return false;
      }
      event.preventDefault();
      return true;
    },
  };

  return (
    <div
      className={'cgs-cell-editor' + (shown ? ' cgs-cell-editor-open' : '')}
      data-testid="cell-editor-box"
    >
      <textarea
        ref={area}
        data-testid="cell-editor"
        aria-label="Cell editor"
        dir="auto"
        spellCheck={false}
        rows={1}
        tabIndex={-1}
        style={rich ? { display: 'none' } : undefined}
        value={edit.active ? edit.text : ''}
        onMouseDown={() => session.setPlace('cell')}
        onChange={(event) => {
          session.setText(event.target.value, event.target.selectionStart ?? 0);
        }}
        onSelect={(event) => {
          const element = event.currentTarget;
          if (edit.active && edit.place === 'cell') {
            const start = element.selectionStart ?? 0;
            const end = element.selectionEnd ?? start;
            const backward = element.selectionDirection === 'backward';
            session.setSelection(backward ? end : start, backward ? start : end, element.value);
          }
        }}
        onKeyDown={(event) => {
          if (formatKey(event)) return;
          onKey(event);
        }}
      />
      {rich && <RichEditor session={session} zoom={zoom} onKey={onKey} />}
      {matches.length > 0 && (
        <ul className="cgs-completions" role="listbox" data-testid="completions">
          {matches.map((name, index) => (
            <li
              key={name}
              role="option"
              aria-selected={index === selected}
              className={index === selected ? 'cgs-completion-chosen' : undefined}
              onMouseDown={(event) => {
                event.preventDefault();
                session.complete(name);
              }}
            >
              {name}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
});
