# Reads what Excel shows in a workbook's cells, so bench/run-excel-text.mjs can compare it with
# our display text. Opens one file read-only, with macros and links off, and never saves it.
# Writes a JSON array: sheet, row, column, text. Used on generated and private workbooks only.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][string]$Out,
  [int]$MaxRows = 60,
  [int]$MaxColumns = 26
)
$ErrorActionPreference = 'Stop'
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  # UpdateLinks 0, ReadOnly true.
  $book = $excel.Workbooks.Open($Workbook, 0, $true)
  $cells = New-Object System.Collections.Generic.List[object]
  $sheetIndex = -1
  foreach ($sheet in $book.Sheets) {
    $sheetIndex++
    if ($sheet.Type -ne -4167) { continue } # xlWorksheet
    $used = $sheet.UsedRange
    $startRow = $used.Row
    $startCol = $used.Column
    $rows = [Math]::Min($used.Rows.Count, $MaxRows)
    $cols = [Math]::Min($used.Columns.Count, $MaxColumns)
    for ($r = 0; $r -lt $rows; $r++) {
      for ($c = 0; $c -lt $cols; $c++) {
        $cell = $sheet.Cells.Item($startRow + $r, $startCol + $c)
        if ($cell.MergeCells -and $cell.MergeArea.Cells.Item(1, 1).Address() -ne $cell.Address()) { continue }
        $text = $cell.Text
        if ($null -eq $text -or $text -eq '') { continue }
        $cells.Add(@{ sheet = $sheetIndex; row = $startRow + $r; col = $startCol + $c; text = [string]$text })
      }
    }
  }
  $book.Close($false)
  [System.IO.File]::WriteAllText($Out, (ConvertTo-Json -InputObject $cells.ToArray() -Compress -Depth 3), [System.Text.UTF8Encoding]::new($false))
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
