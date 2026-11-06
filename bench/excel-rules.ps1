# Has Excel judge the workbooks `rulecheck` made: what conditional formats show (DisplayFormat), which
# cells their validation rules allow (Validation.Value), which rows filters hide once Excel applies
# them again, and what Excel's own sort makes of the unsorted copy. Generated workbooks only. Each file
# opens read-only with macros and links off, and none is saved.
param(
  [Parameter(Mandatory)][string]$Cases,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
$wanted = Get-Content -Raw -LiteralPath $Cases | ConvertFrom-Json
$excel = New-Object -ComObject Excel.Application
$results = @()

function Rgb([double]$bgr) {
  $value = [int64]$bgr
  $r = $value -band 0xFF
  $g = ($value -shr 8) -band 0xFF
  $b = ($value -shr 16) -band 0xFF
  return ('{0:X2}{1:X2}{2:X2}' -f $r, $g, $b)
}

function Rgb-Text([string]$text) {
  $r = [Convert]::ToInt32($text.Substring(0, 2), 16)
  $g = [Convert]::ToInt32($text.Substring(2, 2), 16)
  $b = [Convert]::ToInt32($text.Substring(4, 2), 16)
  return [double]($r + 256 * $g + 65536 * $b)
}

try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  foreach ($case in $wanted) {
    $result = @{ name = $case.name; kind = $case.kind; error = $null; cells = @(); valid = @(); hidden = @(); values = @() }
    $book = $null
    try {
      $a = $case.area
      $path = if ($case.kind -eq 'sort') { $case.before } else { $case.file }
      $book = $excel.Workbooks.Open($path, 0, $true)
      $sheet = $book.Worksheets.Item(1)
      $sheet.Activate()
      $excel.Calculate()
      switch ($case.kind) {
        'cf' {
          for ($r = [int]$a.top; $r -le [int]$a.bottom; $r++) {
            for ($c = [int]$a.left; $c -le [int]$a.right; $c++) {
              $shown = $sheet.Cells.Item($r, $c).DisplayFormat
              $fill = $null
              if ([int]$shown.Interior.ColorIndex -ne -4142) { $fill = Rgb ([double]$shown.Interior.Color) }
              $result.cells += @{
                row = $r
                col = $c
                fill = $fill
                color = (Rgb ([double]$shown.Font.Color))
                bold = [bool]$shown.Font.Bold
              }
            }
          }
        }
        'dv' {
          for ($r = [int]$a.top; $r -le [int]$a.bottom; $r++) {
            for ($c = [int]$a.left; $c -le [int]$a.right; $c++) {
              $cell = $sheet.Cells.Item($r, $c)
              try { $ok = [bool]$cell.Validation.Value } catch { continue }
              $result.valid += @{ row = $r; col = $c; valid = $ok }
            }
          }
        }
        'filter' {
          if ($sheet.AutoFilterMode) { $sheet.AutoFilter.ApplyFilter() }
          foreach ($table in $sheet.ListObjects) {
            if ($null -ne $table.AutoFilter) { $table.AutoFilter.ApplyFilter() }
          }
          for ($r = [int]$a.top; $r -le [int]$a.bottom; $r++) {
            if ([bool]$sheet.Rows.Item($r).Hidden) { $result.hidden += $r }
          }
        }
        'sort' {
          $spec = $case.sort
          $range = $sheet.Range($sheet.Cells.Item([int]$a.top, [int]$a.left), $sheet.Cells.Item([int]$a.bottom, [int]$a.right))
          $sort = $sheet.Sort
          $sort.SortFields.Clear()
          $first = if ($spec.header -and -not $spec.byColumns) { [int]$a.top + 1 } else { [int]$a.top }
          $firstCol = if ($spec.header -and $spec.byColumns) { [int]$a.left + 1 } else { [int]$a.left }
          foreach ($key in $spec.keys) {
            if ($spec.byColumns) {
              $keyRange = $sheet.Range($sheet.Cells.Item([int]$key.index, $firstCol), $sheet.Cells.Item([int]$key.index, [int]$a.right))
            } else {
              $keyRange = $sheet.Range($sheet.Cells.Item($first, [int]$key.index), $sheet.Cells.Item([int]$a.bottom, [int]$key.index))
            }
            $order = if ($key.descending) { 2 } else { 1 }
            $by = if ($null -eq $key.by) { 'value' } else { [string]$key.by.kind }
            $on = switch ($by) { 'cellColor' { 1 } 'fontColor' { 2 } default { 0 } }
            if ($null -ne $key.list) {
              $field = $sort.SortFields.Add($keyRange, $on, $order, ($key.list -join ','), 0)
            } else {
              $field = $sort.SortFields.Add($keyRange, $on, $order, [Type]::Missing, 0)
            }
            if ($on -eq 1 -and $key.by.color) { $field.SortOnValue.Color = (Rgb-Text $key.by.color) }
            if ($on -eq 2 -and $key.by.color) { $field.SortOnValue.Color = (Rgb-Text $key.by.color) }
          }
          $sort.SetRange($range)
          $sort.Header = if ($spec.header) { 1 } else { 2 }
          $sort.MatchCase = [bool]$spec.caseSensitive
          $sort.Orientation = if ($spec.byColumns) { 2 } else { 1 }
          $sort.Apply()
          # Wide enough that `Text` shows what a cell holds rather than ####.
          [void]$sheet.Columns.AutoFit()
          for ($r = [int]$a.top; $r -le [int]$a.bottom; $r++) {
            $row = @()
            for ($c = [int]$a.left; $c -le [int]$a.right; $c++) { $row += [string]$sheet.Cells.Item($r, $c).Text }
            $result.values += , $row
          }
        }
      }
    } catch {
      $result.error = $_.Exception.Message + ' at ' + $_.InvocationInfo.ScriptLineNumber
    } finally {
      if ($null -ne $book) { $book.Close($false) }
    }
    $results += $result
  }
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
[System.IO.File]::WriteAllText($Out, (ConvertTo-Json -InputObject $results -Depth 6 -Compress), [System.Text.UTF8Encoding]::new($false))
