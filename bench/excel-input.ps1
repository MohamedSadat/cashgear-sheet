# Shows Excel what someone might type, for bench/run-excel-input.mjs: each line of text goes into a
# cell of a new, empty workbook, which is never saved. Writes what Excel made of each.
param(
  [Parameter(Mandatory)][string]$Inputs,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AutomationSecurity = 3
  $book = $excel.Workbooks.Add()
  $sheet = $book.Worksheets.Item(1)
  $cases = Get-Content -Raw -Encoding UTF8 $Inputs | ConvertFrom-Json
  $results = New-Object System.Collections.Generic.List[object]
  foreach ($text in $cases) {
    $cell = $sheet.Cells.Item(5, 5)
    $cell.Clear() | Out-Null
    $entry = [ordered]@{ text = [string]$text }
    try {
      $cell.Value2 = [string]$text
      if ($cell.HasFormula) {
        $entry.kind = 'formula'
        $entry.value = [string]$cell.Formula
      } else {
        $value = $cell.Value2
        if ($null -eq $value) {
          $entry.kind = 'clear'
        } elseif ($value -is [double]) {
          $entry.kind = 'number'
          $entry.value = $value
        } elseif ($value -is [bool]) {
          $entry.kind = 'bool'
          $entry.value = $value
        } elseif ($value -is [int]) {
          $entry.kind = 'error'
          $entry.value = [string]$cell.Text
        } else {
          $entry.kind = 'text'
          $entry.value = [string]$value
        }
      }
      $entry.format = [string]$cell.NumberFormat
    } catch {
      $entry.kind = 'refused'
    }
    $results.Add($entry)
  }
  $book.Close($false)
  $results | ConvertTo-Json -Depth 3 | Set-Content -Encoding UTF8 $Out
} finally {
  $excel.Quit()
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
}
