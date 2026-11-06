// Four of Excel's small dialogs: Remove Duplicates, Insert Hyperlink, Create Table and a cell's note.
// Each reads through the engine when it opens and changes the workbook with one command.
import { useEffect, useState } from 'react';
import { CgButton, CgWindow } from '@cashgear/ui';
import type { Applied, CellArea, Command, EngineClient } from '@cgs/engine-client';
import { quoteSheet } from '../formula-text';
import { columnName } from '../grid/geometry';

type Run = (command: Command) => Promise<Applied>;

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Excel's message after Remove Duplicates. `rows` counts the data rows, without the header. */
export function duplicatesMessage(removed: number, rows: number) {
  if (removed <= 0) return 'No duplicate values found.';
  return `${removed} duplicate values found and removed; ${Math.max(rows - removed, 0)} unique values remain.`;
}

/** A column's name in the list: its heading when the data has headers, else `Column A`. */
export function columnHeading(col: number, heading: string, headers: boolean) {
  const text = heading.trim();
  return headers && text ? text : `Column ${columnName(col - 1)}`;
}

/** `$A$1:$D$9` from a 1-based area, as Create Table shows it. */
export function absoluteRange(area: CellArea) {
  const cell = (row: number, col: number) => `$${columnName(col - 1)}$${row}`;
  const first = cell(area.top, area.left);
  if (area.top === area.bottom && area.left === area.right) return first;
  return first + ':' + cell(area.bottom, area.right);
}

/** A mail link, with the subject made safe for a URL. */
export function mailtoAddress(address: string, subject: string) {
  const to = address.trim().replace(/^mailto:/i, '');
  const about = subject.trim();
  return 'mailto:' + to + (about ? '?subject=' + encodeURIComponent(about) : '');
}

/** The address and subject in a mail link. */
export function readMailto(link: string): { address: string; subject: string } {
  const rest = link.replace(/^mailto:/i, '');
  const at = rest.indexOf('?');
  if (at < 0) return { address: rest, subject: '' };
  const subject = new URLSearchParams(rest.slice(at + 1).replace(/\+/g, '%2B')).get('subject');
  return { address: rest.slice(0, at), subject: subject ?? '' };
}

/** A place in this workbook, such as `'Sheet 2'!A1`. With no sheet it's a defined name. */
export function linkLocation(sheet: string | null, reference: string) {
  const ref = reference.trim();
  return sheet === null ? ref : quoteSheet(sheet) + '!' + ref;
}

/** The sheet and reference in a link's place, unquoting the sheet. */
export function readLocation(location: string): { sheet: string | null; reference: string } {
  const text = location.trim();
  if (text.startsWith("'")) {
    for (let at = 1; at < text.length; at++) {
      if (text[at] !== "'") continue;
      if (text[at + 1] === "'") {
        at++;
        continue;
      }
      if (text[at + 1] !== '!') break;
      return { sheet: text.slice(1, at).replace(/''/g, "'"), reference: text.slice(at + 2) };
    }
    return { sheet: null, reference: text };
  }
  const bang = text.lastIndexOf('!');
  if (bang < 0) return { sheet: null, reference: text };
  return { sheet: text.slice(0, bang), reference: text.slice(bang + 1) };
}

// Remove Duplicates

export interface RemoveDuplicatesDialogProps {
  open: boolean;
  engine: () => EngineClient | null;
  sheet: number;
  /** Already grown to the current region. 1-based. */
  area: CellArea;
  /** Whether the first row looks like headings. */
  headers: boolean;
  run: Run;
  onClose: () => void;
  onDone: (message: string) => void;
}

export function RemoveDuplicatesDialog(props: RemoveDuplicatesDialogProps) {
  const { open, sheet, area } = props;
  const [headers, setHeaders] = useState(props.headers);
  const [headings, setHeadings] = useState<string[]>([]);
  const [chosen, setChosen] = useState<boolean[]>([]);
  const [error, setError] = useState('');
  const width = area.right - area.left + 1;

  useEffect(() => {
    if (!open) return;
    let live = true;
    setHeaders(props.headers);
    setChosen(Array.from({ length: width }, () => true));
    setHeadings([]);
    setError('');
    const engine = props.engine();
    if (engine) {
      void Promise.all(
        Array.from({ length: width }, (_, at) =>
          engine.cellText(sheet, area.top, area.left + at).then((text) => text.display),
        ),
      ).then((texts) => {
        if (live) setHeadings(texts);
      });
    }
    return () => {
      live = false;
    };
    // Read once each time the dialog opens.
  }, [open]);

  const columns = chosen.flatMap((on, at) => (on ? [area.left + at] : []));

  const ok = async () => {
    if (columns.length === 0) return;
    try {
      const applied = await props.run({
        kind: 'removeDuplicates',
        sheet,
        area,
        columns,
        header: headers,
      });
      const rows = area.bottom - area.top + 1 - (headers ? 1 : 0);
      props.onDone(duplicatesMessage(applied.replaced ?? 0, rows));
      props.onClose();
    } catch (failure) {
      setError(errorText(failure));
    }
  };

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Remove Duplicates"
      width={420}
      allowDrag
      data-testid="duplicates-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <p className="cgs-dialog-message">
            To delete duplicate values, select one or more columns that contain duplicates.
          </p>
          <div className="cgs-dialog-row">
            <CgButton
              data-testid="duplicates-select-all"
              onClick={() => setChosen(chosen.map(() => true))}
            >
              Select All
            </CgButton>
            <CgButton
              data-testid="duplicates-unselect-all"
              onClick={() => setChosen(chosen.map(() => false))}
            >
              Unselect All
            </CgButton>
            <label className="cgs-dialog-check" style={{ marginInlineStart: 'auto' }}>
              <input
                type="checkbox"
                data-testid="duplicates-headers"
                checked={headers}
                onChange={(event) => setHeaders(event.target.checked)}
              />
              My data has headers
            </label>
          </div>
          <span>Columns</span>
          <ul
            className="cgs-find-list"
            data-testid="duplicates-columns"
            style={{ padding: 'var(--cg-space-1)' }}
          >
            {chosen.map((on, at) => (
              <li key={at}>
                <label className="cgs-dialog-check">
                  <input
                    type="checkbox"
                    data-testid={`duplicates-column-${area.left + at}`}
                    checked={on}
                    onChange={(event) =>
                      setChosen(chosen.map((was, i) => (i === at ? event.target.checked : was)))
                    }
                  />
                  {columnHeading(area.left + at, headings[at] ?? '', headers)}
                </label>
              </li>
            ))}
          </ul>
          {error && (
            <p role="alert" data-testid="duplicates-error" className="cgs-dialog-message">
              {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton
            intent="primary"
            data-testid="duplicates-ok"
            disabled={columns.length === 0}
            onClick={() => void ok()}
          >
            OK
          </CgButton>
          <CgButton data-testid="duplicates-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

// Insert Hyperlink

type LinkTo = 'web' | 'place' | 'email';

export interface HyperlinkDialogProps {
  open: boolean;
  engine: () => EngineClient | null;
  sheet: number;
  /** 1-based. The link goes on all of it, and its text in the top-left cell. */
  area: CellArea;
  sheetNames: string[];
  run: Run;
  onClose: () => void;
}

export function HyperlinkDialog(props: HyperlinkDialogProps) {
  const { open, sheet, area, sheetNames } = props;
  const [linkTo, setLinkTo] = useState<LinkTo>('web');
  const [address, setAddress] = useState('');
  const [reference, setReference] = useState('A1');
  /** The sheet the place is on, or null for a defined name. */
  const [placeSheet, setPlaceSheet] = useState<string | null>(null);
  const [email, setEmail] = useState('');
  const [subject, setSubject] = useState('');
  const [display, setDisplay] = useState('');
  const [shown, setShown] = useState('');
  const [tip, setTip] = useState('');
  const [linked, setLinked] = useState(false);
  const [names, setNames] = useState<string[]>([]);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!open) return;
    let live = true;
    setLinkTo('web');
    setAddress('');
    setReference('A1');
    setPlaceSheet(sheetNames[sheet] ?? null);
    setEmail('');
    setSubject('');
    setDisplay('');
    setShown('');
    setTip('');
    setLinked(false);
    setNames([]);
    setError('');
    const engine = props.engine();
    if (engine) {
      // Names this sheet can use without a sheet in front.
      void engine.query({ kind: 'names' }).then((list) => {
        if (live) {
          const usable = list.filter((name) => name.sheet === null || name.sheet === sheet);
          setNames([...new Set(usable.map((name) => name.name))]);
        }
      });
      void Promise.all([
        engine.query({ kind: 'linkAt', sheet, row: area.top, col: area.left }),
        engine.cellText(sheet, area.top, area.left),
      ]).then(([link, text]) => {
        if (!live) return;
        setDisplay(text.display);
        setShown(text.display);
        if (!link) return;
        setLinked(true);
        setTip(link.tooltip ?? '');
        if (link.address && /^mailto:/i.test(link.address)) {
          const mail = readMailto(link.address);
          setLinkTo('email');
          setEmail(mail.address);
          setSubject(mail.subject);
        } else if (link.address) {
          setAddress(link.address);
        } else if (link.location) {
          const place = readLocation(link.location);
          setLinkTo('place');
          setPlaceSheet(place.sheet);
          setReference(place.reference);
        }
      });
    }
    return () => {
      live = false;
    };
    // Read once each time the dialog opens.
  }, [open]);

  const ok = async () => {
    const engine = props.engine();
    if (!engine) return;
    setError('');
    const link: { address?: string; location?: string; tooltip?: string; text?: string } = {};
    if (linkTo === 'web') {
      if (!address.trim()) return setError('Enter an address.');
      link.address = address.trim();
    } else if (linkTo === 'email') {
      if (!email.trim().replace(/^mailto:/i, '')) return setError('Enter an e-mail address.');
      link.address = mailtoAddress(email, subject);
    } else {
      const location = linkLocation(placeSheet, reference);
      if (!reference.trim() || !(await engine.resolveName(location, sheet))) {
        return setError('Reference is not valid.');
      }
      link.location = location;
    }
    if (tip.trim()) link.tooltip = tip.trim();
    // Only text that was typed here replaces what the cell holds.
    if (display !== shown && display !== '') link.text = display;
    try {
      await props.run({ kind: 'setLink', sheet, area, link });
      props.onClose();
    } catch (failure) {
      setError(errorText(failure));
    }
  };

  const remove = async () => {
    try {
      await props.run({ kind: 'setLink', sheet, area });
      props.onClose();
    } catch (failure) {
      setError(errorText(failure));
    }
  };

  const choice = (key: LinkTo, text: string) => (
    <button
      type="button"
      className={linkTo === key ? 'cgs-edge cgs-edge-on' : 'cgs-edge'}
      aria-pressed={linkTo === key}
      data-testid={`link-to-${key}`}
      onClick={() => setLinkTo(key)}
    >
      {text}
    </button>
  );

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText={linked ? 'Edit Hyperlink' : 'Insert Hyperlink'}
      width={560}
      allowDrag
      data-testid="link-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <label className="cgs-dialog-field">
            <span>Text to display</span>
            <input
              data-testid="link-text"
              value={display}
              onChange={(event) => setDisplay(event.target.value)}
            />
          </label>
          <div className="cgs-dialog-split" style={{ minBlockSize: '14rem' }}>
            <div className="cgs-dialog-column" role="group" aria-label="Link to">
              <span>Link to</span>
              {choice('web', 'Existing File or Web Page')}
              {choice('place', 'Place in This Document')}
              {choice('email', 'E-mail Address')}
            </div>
            <div className="cgs-dialog-column">
              {linkTo === 'web' && (
                <label className="cgs-dialog-field">
                  <span>Address</span>
                  <input
                    data-testid="link-address"
                    autoFocus
                    value={address}
                    onChange={(event) => setAddress(event.target.value)}
                  />
                </label>
              )}
              {linkTo === 'place' && (
                <>
                  <label className="cgs-dialog-field">
                    <span>Type the cell reference</span>
                    <input
                      data-testid="link-reference"
                      value={reference}
                      onChange={(event) => setReference(event.target.value)}
                    />
                  </label>
                  <label className="cgs-dialog-field">
                    <span>Or select a place in this document</span>
                    <select
                      size={8}
                      data-testid="link-sheet"
                      value={placeSheet === null ? 'name:' + reference : 'sheet:' + placeSheet}
                      onChange={(event) => {
                        const value = event.target.value;
                        if (value.startsWith('sheet:')) {
                          setPlaceSheet(value.slice('sheet:'.length));
                          if (placeSheet === null) setReference('A1');
                        } else {
                          setPlaceSheet(null);
                          setReference(value.slice('name:'.length));
                        }
                      }}
                    >
                      <optgroup label="Cell Reference">
                        {sheetNames.map((name) => (
                          <option key={name} value={'sheet:' + name}>
                            {name}
                          </option>
                        ))}
                      </optgroup>
                      {names.length > 0 && (
                        <optgroup label="Defined Names">
                          {names.map((name) => (
                            <option key={name} value={'name:' + name}>
                              {name}
                            </option>
                          ))}
                        </optgroup>
                      )}
                    </select>
                  </label>
                </>
              )}
              {linkTo === 'email' && (
                <>
                  <label className="cgs-dialog-field">
                    <span>E-mail address</span>
                    <input
                      data-testid="link-email"
                      autoFocus
                      value={email}
                      onChange={(event) => setEmail(event.target.value)}
                    />
                  </label>
                  <label className="cgs-dialog-field">
                    <span>Subject</span>
                    <input
                      data-testid="link-subject"
                      value={subject}
                      onChange={(event) => setSubject(event.target.value)}
                    />
                  </label>
                </>
              )}
              <label className="cgs-dialog-field">
                <span>ScreenTip text</span>
                <input
                  data-testid="link-tip"
                  value={tip}
                  onChange={(event) => setTip(event.target.value)}
                />
              </label>
            </div>
          </div>
          {error && (
            <p role="alert" data-testid="link-error" className="cgs-dialog-message">
              {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          {linked && (
            <CgButton
              data-testid="link-remove"
              style={{ marginInlineEnd: 'auto' }}
              onClick={() => void remove()}
            >
              Remove Link
            </CgButton>
          )}
          <CgButton intent="primary" data-testid="link-ok" onClick={() => void ok()}>
            OK
          </CgButton>
          <CgButton data-testid="link-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

// Create Table

export interface CreateTableDialogProps {
  open: boolean;
  sheet: number;
  /** The guessed range, 1-based. */
  area: CellArea;
  headers: boolean;
  engine: () => EngineClient | null;
  run: Run;
  onClose: () => void;
}

export function CreateTableDialog(props: CreateTableDialogProps) {
  const { open, sheet, area } = props;
  const [range, setRange] = useState(() => '=' + absoluteRange(area));
  const [headers, setHeaders] = useState(props.headers);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!open) return;
    setRange('=' + absoluteRange(area));
    setHeaders(props.headers);
    setError('');
    // Start again each time the dialog opens.
  }, [open]);

  const ok = async () => {
    const engine = props.engine();
    if (!engine) return;
    const text = range.trim().replace(/^=/, '');
    const found = text ? await engine.resolveName(text, sheet) : null;
    if (!found) return setError('The reference isn’t valid.');
    const { sheet: on, ...where } = found;
    try {
      await props.run({ kind: 'createTable', sheet: on, area: where, headers });
      props.onClose();
    } catch (failure) {
      setError(errorText(failure));
    }
  };

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText="Create Table"
      width={320}
      allowDrag
      data-testid="table-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          <label className="cgs-dialog-field">
            <span>Where is the data for your table?</span>
            <input
              data-testid="table-range"
              autoFocus
              value={range}
              onChange={(event) => setRange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  void ok();
                }
              }}
            />
          </label>
          <label className="cgs-dialog-check">
            <input
              type="checkbox"
              data-testid="table-headers"
              checked={headers}
              onChange={(event) => setHeaders(event.target.checked)}
            />
            My table has headers
          </label>
          {error && (
            <p role="alert" data-testid="table-error" className="cgs-dialog-message">
              {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          <CgButton intent="primary" data-testid="table-ok" onClick={() => void ok()}>
            OK
          </CgButton>
          <CgButton data-testid="table-cancel" onClick={props.onClose}>
            Cancel
          </CgButton>
        </div>
      )}
    />
  );
}

// A cell's note

export interface NoteDialogProps {
  open: boolean;
  engine: () => EngineClient | null;
  sheet: number;
  /** 1-based. */
  row: number;
  col: number;
  run: Run;
  onClose: () => void;
}

export function NoteDialog(props: NoteDialogProps) {
  const { open, sheet, row, col } = props;
  const [text, setText] = useState('');
  const [author, setAuthor] = useState('');
  const [exists, setExists] = useState(false);
  const [threaded, setThreaded] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!open) return;
    let live = true;
    setText('');
    setAuthor('');
    setExists(false);
    setThreaded(false);
    setError('');
    const engine = props.engine();
    if (engine) {
      void engine.query({ kind: 'noteAt', sheet, row, col }).then((note) => {
        if (!live || !note) return;
        setExists(true);
        setText(note.text ?? '');
        setAuthor(note.author ?? '');
        setThreaded(note.threaded === true);
      });
    }
    return () => {
      live = false;
    };
    // Read once each time the dialog opens.
  }, [open]);

  const save = async (keep: boolean) => {
    try {
      await props.run(
        keep ? { kind: 'setNote', sheet, row, col, text } : { kind: 'setNote', sheet, row, col },
      );
      props.onClose();
    } catch (failure) {
      setError(errorText(failure));
    }
  };

  return (
    <CgWindow
      open={open}
      onOpenChange={(next) => {
        if (!next) props.onClose();
      }}
      headerText={threaded ? 'Comment' : exists ? 'Edit Note' : 'New Note'}
      width={360}
      allowDrag
      data-testid="note-dialog"
      renderBody={() => (
        <div className="cgs-dialog-column">
          {threaded ? (
            <p data-testid="note-threaded" className="cgs-dialog-message">
              This cell has a threaded comment from a newer Excel, which is kept as it is.
            </p>
          ) : (
            <label className="cgs-dialog-field">
              <span>{author ? author + ':' : 'Note'}</span>
              <textarea
                data-testid="note-text"
                autoFocus
                rows={6}
                value={text}
                onChange={(event) => setText(event.target.value)}
                style={{ font: 'inherit', resize: 'vertical' }}
              />
            </label>
          )}
          {error && (
            <p role="alert" data-testid="note-error" className="cgs-dialog-message">
              {error}
            </p>
          )}
        </div>
      )}
      renderFooter={() => (
        <div className="cgs-dialog-actions">
          {threaded ? (
            <CgButton intent="primary" data-testid="note-close" onClick={props.onClose}>
              Close
            </CgButton>
          ) : (
            <>
              {exists && (
                <CgButton
                  data-testid="note-delete"
                  style={{ marginInlineEnd: 'auto' }}
                  onClick={() => void save(false)}
                >
                  Delete
                </CgButton>
              )}
              <CgButton intent="primary" data-testid="note-ok" onClick={() => void save(true)}>
                OK
              </CgButton>
              <CgButton data-testid="note-cancel" onClick={props.onClose}>
                Cancel
              </CgButton>
            </>
          )}
        </div>
      )}
    />
  );
}
