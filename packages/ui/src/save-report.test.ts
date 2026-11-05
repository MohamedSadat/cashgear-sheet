import { describe, expect, it } from 'vitest';
import type { SaveReport } from '@cgs/engine-client';
import { saveReportLines, saveSummary, workbookExtension } from './save-report';

const nothing: SaveReport = {
  keptParts: 1,
  cellsWritten: 0,
  stringsAdded: 0,
  recalculatesOnOpen: false,
  pivotsRefreshOnOpen: false,
  resultsLeftToExcel: 0,
  tableHeadersFixed: 0,
  leftExtensions: [],
  changedParts: [],
  removedParts: [],
  addedParts: [],
  keptContent: [],
};

describe('the save report', () => {
  it('says nothing changed when nothing did', () => {
    expect(saveSummary(nothing)).toBe('Saved, with every part unchanged');
    expect(saveReportLines(nothing)).toEqual([
      '1 part of the file was copied exactly as it was.',
      'Nothing in the file changed.',
    ]);
  });

  it('names what it kept, changed, added and removed', () => {
    const lines = saveReportLines({
      ...nothing,
      keptParts: 12,
      cellsWritten: 2,
      recalculatesOnOpen: true,
      changedParts: ['xl/workbook.xml'],
      addedParts: ['xl/sharedStrings.xml'],
      removedParts: ['xl/calcChain.xml'],
      tableHeadersFixed: 1,
      resultsLeftToExcel: 3,
      keptContent: [
        { what: 'macros', parts: 1 },
        { what: 'charts', parts: 2 },
        { what: 'pivot tables', parts: 4 },
      ],
    });
    expect(lines).toEqual([
      '12 parts of the file were copied exactly as they were.',
      "This workbook's macros, charts and pivot tables stayed untouched.",
      '2 cells written.',
      'Changed: xl/workbook.xml.',
      'Added: xl/sharedStrings.xml.',
      'Removed: xl/calcChain.xml. Excel rebuilds this when it opens the file.',
      'Excel will recalculate this workbook when it opens it.',
      '1 table header updated to match the table.',
      '3 results will be worked out by Excel when it opens the file.',
    ]);
  });

  it('counts the parts it kept once something changed', () => {
    expect(saveSummary({ ...nothing, keptParts: 9, changedParts: ['a'] })).toBe(
      'Saved: 9 parts kept exactly',
    );
  });
});

describe('the file type of a saved workbook', () => {
  it('stays what it was', () => {
    expect(workbookExtension('a.XLSM')).toBe('xlsm');
    expect(workbookExtension('a.b.xltx')).toBe('xltx');
    expect(workbookExtension('Book1')).toBe('xlsx');
  });
});
