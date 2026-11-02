interface FormulaBarProps {
  /** The selection's address. */
  name: string;
  /** The active cell's formula, or its constant. */
  text: string;
}

/** Read-only until Phase 4 makes it the cell editor's other half. */
export function FormulaBar({ name, text }: FormulaBarProps) {
  return (
    <div className="cgs-formula-bar">
      <input
        className="cgs-name-box"
        aria-label="Name box"
        data-testid="name-box"
        readOnly
        value={name}
      />
      <span className="cgs-formula-mark" aria-hidden="true">
        fx
      </span>
      <input
        className="cgs-formula-text"
        aria-label="Formula bar"
        data-testid="formula-bar"
        dir="auto"
        readOnly
        value={text}
      />
    </div>
  );
}
