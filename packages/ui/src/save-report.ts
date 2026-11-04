import type { SaveReport } from '@cgs/engine-client';

const plural = (count: number, one: string, many = one + 's') =>
  `${count} ${count === 1 ? one : many}`;

function joined(items: string[]) {
  if (items.length < 2) return items.join('');
  return items.slice(0, -1).join(', ') + ' and ' + items[items.length - 1];
}

/** What a save did, in lines for the report. Only ever says what is true of this save. */
export function saveReportLines(report: SaveReport): string[] {
  const lines: string[] = [];
  const kept = report.keptContent.map((entry) => entry.what);
  lines.push(
    `${plural(report.keptParts, 'part')} of the file ${report.keptParts === 1 ? 'was' : 'were'} copied exactly as ${report.keptParts === 1 ? 'it was' : 'they were'}.`,
  );
  if (kept.length) lines.push(`This workbook's ${joined(kept)} stayed untouched.`);
  if (report.cellsWritten) lines.push(`${plural(report.cellsWritten, 'cell')} written.`);
  if (report.changedParts.length) {
    lines.push(`Changed: ${report.changedParts.join(', ')}.`);
  } else {
    lines.push('Nothing in the file changed.');
  }
  if (report.addedParts.length) lines.push(`Added: ${report.addedParts.join(', ')}.`);
  if (report.removedParts.length) {
    lines.push(
      `Removed: ${report.removedParts.join(', ')}. Excel rebuilds this when it opens the file.`,
    );
  }
  if (report.recalculatesOnOpen)
    lines.push('Excel will recalculate this workbook when it opens it.');
  if (report.pivotsRefreshOnOpen)
    lines.push('Excel will refresh its pivot tables when it opens it.');
  if (report.tableHeadersFixed) {
    lines.push(`${plural(report.tableHeadersFixed, 'table header')} updated to match the table.`);
  }
  if (report.resultsLeftToExcel) {
    lines.push(
      `${plural(report.resultsLeftToExcel, 'result')} will be worked out by Excel when it opens the file.`,
    );
  }
  return lines;
}

/** One line for the status bar. */
export function saveSummary(report: SaveReport): string {
  if (!report.changedParts.length && !report.addedParts.length && !report.removedParts.length) {
    return 'Saved, with every part unchanged';
  }
  return `Saved: ${plural(report.keptParts, 'part')} kept exactly`;
}

/** The file type a workbook keeps when saved under a new name. Excel refuses a mismatch. */
export function workbookExtension(name: string): string {
  const match = /\.(xlsx|xlsm|xltx|xltm)$/i.exec(name);
  return (match?.[1] ?? 'xlsx').toLowerCase();
}
