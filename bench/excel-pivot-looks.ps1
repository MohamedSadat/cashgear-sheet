# Reads what Excel shows on every cell of every pivot table in a workbook: its fill, font colour
# and weight, and its edges, from DisplayFormat, which includes the pivot style. Generated workbooks
# only. The file opens read-only with macros and links off, and is never saved.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
$excel = New-Object -ComObject Excel.Application
$result = @{ error = $null; pivots = @() }

function Rgb([double]$bgr) {
  $value = [int64]$bgr
  return ('{0:X2}{1:X2}{2:X2}' -f ($value -band 0xFF), (($value -shr 8) -band 0xFF), (($value -shr 16) -band 0xFF))
}

function Edge($borders, [int]$index) {
  $border = $borders.Item($index)
  $style = $border.LineStyle
  if ($null -eq $style -or $style -is [DBNull] -or [int]$style -eq -4142) { return $null }
  return @{ style = [int]$style; weight = [int]$border.Weight; color = (Rgb ([double]$border.Color)) }
}

try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  $book = $excel.Workbooks.Open($Workbook, 0, $true)
  foreach ($sheet in $book.Worksheets) {
    foreach ($table in $sheet.PivotTables()) {
      $range = $table.TableRange2
      $entry = @{ sheet = [string]$sheet.Name; name = [string]$table.Name; style = [string]$table.TableStyle2.Name; cells = @() }
      foreach ($cell in $range.Cells) {
        $shown = $cell.DisplayFormat
        $fill = $null
        if ([int]$shown.Interior.Pattern -ne -4142) { $fill = Rgb ([double]$shown.Interior.Color) }
        $entry.cells += @{
          row = [int]$cell.Row
          col = [int]$cell.Column
          fill = $fill
          color = (Rgb ([double]$shown.Font.Color))
          bold = [bool]$shown.Font.Bold
          top = (Edge $shown.Borders 8)
          bottom = (Edge $shown.Borders 9)
          left = (Edge $shown.Borders 7)
          right = (Edge $shown.Borders 10)
        }
      }
      $result.pivots += $entry
    }
  }
  $book.Close($false)
} catch {
  $result.error = $_.Exception.Message
} finally {
  $excel.Quit()
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
}
$result | ConvertTo-Json -Depth 8 -Compress | Set-Content -LiteralPath $Out -Encoding UTF8
