# Checks in Excel that what a save kept still works, on generated fixtures only. Reports through
# one JSON object. It works on copies in a temporary folder and saves none of them.
#   -Mode chart: the chart and picture are there, and the chart reads the sheet.
#   -Mode pivot: the pivot table is there, refreshes, and comes out right.
#   -Mode pivotReadOnly: what a read-only open shows, without asking for a refresh.
#   -Mode macro: runs the fixture's `Probe` macro, with macros on for this session only, by the
#     decision logged for Phase 3. Only ever pass it a fixture whose macro this repo generated.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][ValidateSet('chart', 'pivot', 'pivotReadOnly', 'macro')][string]$Mode,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
$result = @{ opened = $false; error = $null }
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  # 1 turns macros on, and 3 leaves them off, as it is everywhere else.
  $excel.AutomationSecurity = $(if ($Mode -eq 'macro') { 1 } else { 3 })
  try {
    $readOnly = $Mode -ne 'pivot'
    $book = $excel.Workbooks.Open($Workbook, 0, $readOnly)
    $result.opened = $true
    # A refresh on opening can finish just after Open returns.
    Start-Sleep -Seconds 1
    switch ($Mode) {
      'chart' {
        $charts = 0
        $shapes = 0
        $series = @()
        foreach ($sheet in $book.Worksheets) {
          $shapes += $sheet.Shapes.Count
          foreach ($object in $sheet.ChartObjects()) {
            $charts++
            foreach ($value in $object.Chart.SeriesCollection(1).Values) { $series += [double]$value }
          }
        }
        $result.charts = $charts
        $result.shapes = $shapes
        $result.series = $series
      }
      'pivot' {
        $sheet = $book.Worksheets.Item('Pivot')
        $tables = $sheet.PivotTables()
        $result.pivots = $tables.Count
        $read = {
          @{
            cells = @(4, 5, 6) | ForEach-Object { [string]$sheet.Cells.Item($_, 1).Value2 + '=' + [string]$sheet.Cells.Item($_, 2).Value2 }
            north = [double]$sheet.Range('B4').Value2
            south = [double]$sheet.Range('B5').Value2
            total = [double]$sheet.Range('B6').Value2
          }
        }
        # What opening showed, before anyone asked for a refresh.
        $result.onOpen = & $read
        $result.refreshOnLoad = [bool]$tables.Item(1).PivotCache().RefreshOnFileOpen
        $result.refreshed = [bool]$tables.Item(1).RefreshTable()
        $result.afterRefresh = & $read
      }
      'pivotReadOnly' {
        $sheet = $book.Worksheets.Item('Pivot')
        $result.total = [double]$sheet.Range('B6').Value2
      }
      'macro' {
        $result.hasProject = [bool]$book.HasVBProject
        $result.probe = [double]$excel.Run('Module1.Probe')
      }
    }
    $book.Close($false)
  } catch {
    $result.error = $_.Exception.GetType().Name + ': ' + $_.InvocationInfo.ScriptLineNumber
  }
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
[System.IO.File]::WriteAllText($Out, (ConvertTo-Json -InputObject $result -Compress -Depth 4), [System.Text.UTF8Encoding]::new($false))
