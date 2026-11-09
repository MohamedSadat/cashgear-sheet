// Rich text in the cell, as Excel's cell editor shows it: each run in its own font, typed into
// and selected like any text. The page's own editing keeps a run's look for what is typed in it,
// and what the box holds is read back after each change.
import { useLayoutEffect, useRef, type CSSProperties, type KeyboardEvent } from 'react';
import type { EditSession, RichSpan, RunStyle } from './editing';

const PT_TO_PX = 96 / 72;

/** A run's look as CSS, over the cell's own font that the box already has. */
export function runCss(style: RunStyle, zoom = 1): CSSProperties {
  const css: CSSProperties = {};
  if (style.bold) css.fontWeight = 'bold';
  if (style.italic) css.fontStyle = 'italic';
  const lines = [style.underline ? 'underline' : '', style.strike ? 'line-through' : '']
    .filter(Boolean)
    .join(' ');
  if (lines) css.textDecoration = lines;
  if (style.underline === 'double' || style.underline === 'doubleAccounting') {
    css.textDecorationStyle = 'double';
  }
  if (style.size) css.fontSize = `${(style.size * PT_TO_PX * zoom).toFixed(2)}px`;
  if (style.name) css.fontFamily = `"${style.name}"`;
  if (style.color) css.color = '#' + style.color;
  if (style.vertAlign) {
    css.verticalAlign = style.vertAlign === 'superscript' ? 'super' : 'sub';
    css.fontSize = style.size ? `${(style.size * PT_TO_PX * zoom * 0.66).toFixed(2)}px` : '66%';
  }
  return css;
}

/** Where a node and offset fall in the box's text. */
function offsetOf(root: HTMLElement, node: Node | null, offset: number): number {
  if (!node) return 0;
  let at = 0;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let text = walker.nextNode(); text; text = walker.nextNode()) {
    if (text === node) return at + offset;
    at += text.textContent?.length ?? 0;
  }
  // A container and a child index: count the text before that child.
  if (node.nodeType === Node.ELEMENT_NODE) {
    const child = node.childNodes[offset];
    let total = 0;
    const all = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let text = all.nextNode(); text; text = all.nextNode()) {
      if (child && (child === text || child.contains(text))) return total;
      total += text.textContent?.length ?? 0;
    }
    return total;
  }
  return at;
}

/** Puts the selection at characters `anchor` to `caret` of the box. */
function placeSelection(root: HTMLElement, anchor: number, caret: number) {
  const find = (target: number): [Node, number] => {
    let at = 0;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let last: Node | null = null;
    for (let text = walker.nextNode(); text; text = walker.nextNode()) {
      const length = text.textContent?.length ?? 0;
      if (target <= at + length) return [text, target - at];
      at += length;
      last = text;
    }
    return last ? [last, last.textContent?.length ?? 0] : [root, 0];
  };
  const selection = window.getSelection();
  if (!selection) return;
  const [anchorNode, anchorOffset] = find(anchor);
  const [focusNode, focusOffset] = find(caret);
  selection.setBaseAndExtent(anchorNode, anchorOffset, focusNode, focusOffset);
}

/** The box's spans, read from the runs it shows. */
function readSpans(root: HTMLElement): RichSpan[] {
  const spans: RichSpan[] = [];
  root.childNodes.forEach((node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      spans.push({ text: node.textContent ?? '', style: {} });
    } else if (node instanceof HTMLElement) {
      const style = node.dataset.run ? (JSON.parse(node.dataset.run) as RunStyle) : {};
      // A line break the page put in for Alt+Enter is a newline.
      const text = node.tagName === 'BR' ? '\n' : (node.textContent ?? '');
      spans.push({ text, style });
    }
  });
  return spans;
}

export interface RichEditorProps {
  session: EditSession;
  zoom: number;
  onKey: (event: KeyboardEvent<HTMLElement>) => void;
}

/** The cell editor while its text has runs. */
export function RichEditor({ session, zoom, onKey }: RichEditorProps) {
  const box = useRef<HTMLDivElement>(null);
  const edit = session.snapshot();
  const version = edit.rich?.version ?? 0;

  // The runs are drawn again only when they change from outside, keeping the caret.
  useLayoutEffect(() => {
    const root = box.current;
    const rich = session.snapshot().rich;
    if (!root || !rich) return;
    root.replaceChildren(
      ...rich.spans.map((span) => {
        const element = document.createElement('span');
        element.dataset.run = JSON.stringify(span.style);
        Object.assign(element.style, runCss(span.style, zoom));
        element.textContent = span.text;
        return element;
      }),
    );
    root.focus({ preventScroll: true });
    const state = session.snapshot();
    placeSelection(root, state.anchor, state.caret);
  }, [version, zoom, session]);

  const readBack = () => {
    const root = box.current;
    const selection = window.getSelection();
    if (!root || !selection) return;
    const anchor = offsetOf(root, selection.anchorNode, selection.anchorOffset);
    const caret = offsetOf(root, selection.focusNode, selection.focusOffset);
    session.richTyped(readSpans(root), anchor, caret);
  };

  const toggle = (key: 'bold' | 'italic' | 'underline') =>
    session.formatSelection((style) => {
      const next = { ...style };
      if (key === 'underline') {
        if (next.underline) delete next.underline;
        else next.underline = 'single';
      } else if (next[key]) {
        delete next[key];
      } else {
        next[key] = true;
      }
      return next;
    });

  return (
    <div
      ref={box}
      data-testid="cell-editor-rich"
      className="cgs-cell-editor-rich"
      contentEditable
      suppressContentEditableWarning
      role="textbox"
      aria-multiline="true"
      aria-label="Cell editor"
      spellCheck={false}
      onInput={readBack}
      onKeyUp={readBack}
      onMouseUp={readBack}
      onKeyDown={(event) => {
        const ctrl = event.ctrlKey || event.metaKey;
        const key = event.key.toLowerCase();
        if (ctrl && !event.altKey && (key === 'b' || key === 'i' || key === 'u')) {
          event.preventDefault();
          readBack();
          toggle(key === 'b' ? 'bold' : key === 'i' ? 'italic' : 'underline');
          return;
        }
        if (event.key === 'Enter' && event.altKey) {
          // A new line in the cell, as Alt+Enter makes it.
          event.preventDefault();
          document.execCommand('insertText', false, '\n');
          readBack();
          return;
        }
        onKey(event);
      }}
      style={{ whiteSpace: 'pre-wrap', outline: 'none' }}
    />
  );
}
