# Has Excel 2016 make pivot tables, as it makes them, over a sheet of generated sales. Builds a new
# workbook from the scenarios in -Cases (JSON), one pivot table per sheet, and saves it to -Out.
# Then it makes the edits listed in -Cases' `edits`, refreshes every pivot table, and saves that as
# -Refreshed. It also writes what Excel calls its own captions to -Captions. Opens no existing file.
param(
  [Parameter(Mandatory)][string]$Cases,
  [Parameter(Mandatory)][string]$Out,
  [Parameter(Mandatory)][string]$Refreshed,
  [Parameter(Mandatory)][string]$Captions
)
$ErrorActionPreference = 'Stop'
$spec = Get-Content -Raw -LiteralPath $Cases | ConvertFrom-Json
$excel = New-Object -ComObject Excel.Application

$functions = @{
  sum = -4157; count = -4112; average = -4106; max = -4136; min = -4139; product = -4149
  countNums = -4113; stdDev = -4155; stdDevp = -4156; var = -4164; varp = -4165
}
$calculations = @{
  normal = -4143; difference = 2; percent = 3; percentDiff = 4; runTotal = 5; percentOfRow = 6
  percentOfCol = 7; percentOfTotal = 8; index = 9; percentOfParentRow = 10; percentOfParentCol = 11
  percentOfParent = 12; percentOfRunningTotal = 13; rankAscending = 14; rankDescending = 15
}
$orientations = @{ row = 1; col = 2; page = 3 }

function Set-Cell($sheet, [int]$row, [int]$col, $value) {
  $cell = $sheet.Cells.Item($row, $col)
  if ($null -eq $value) { $cell.ClearContents() | Out-Null }
  elseif ($value -is [string] -and $value.StartsWith('=')) { $cell.Formula = $value }
  elseif ($value -is [string] -and $value.StartsWith('date:')) {
    $cell.Value2 = [double]$value.Substring(5)
    $cell.NumberFormat = 'm/d/yyyy'
  }
  elseif ($value -is [string]) { $cell.Value2 = [string]$value }
  elseif ($value -is [bool]) { $cell.Value2 = [bool]$value }
  else { $cell.Value2 = [double]$value }
}

function Build($book, $case, [int]$index) {
  $sheet = $book.Worksheets.Add([Type]::Missing, $book.Worksheets.Item($book.Worksheets.Count))
  $sheet.Name = $case.name
  $source = $case.source
  if (-not $source) { $source = $spec.source }
  $cache = $book.PivotCaches().Create(1, $source, 6)
  $at = $case.at
  if (-not $at) { $at = 'R3C1' }
  $table = $cache.CreatePivotTable("'" + $case.name + "'!" + $at, 'PivotTable' + $index, $true, 6)
  $table.ManualUpdate = $true
  if ($case.layout) {
    $form = @{ compact = 0; tabular = 1; outline = 2 }[$case.layout]
    $table.RowAxisLayout($form)
  }
  $position = @{ row = 0; col = 0; page = 0 }
  foreach ($f in $case.fields) {
    $field = $table.PivotFields($f.name)
    $field.Orientation = $orientations[$f.axis]
    $position[$f.axis]++
    $field.Position = $position[$f.axis]
  }
  $data = @()
  foreach ($v in $case.values) {
    $function = $functions[$v.summary]
    if ($null -eq $function) { $function = -4157 }
    $caption = $v.caption
    if (-not $caption) { $caption = ([string]$v.summary + ' of ' + $v.field) }
    $df = $table.AddDataField($table.PivotFields($v.field), $caption, $function)
    if ($v.showAs) {
      $df.Calculation = $calculations[$v.showAs]
      if ($v.baseField) { $df.BaseField = $v.baseField }
      if ($v.baseItem) {
        if ($v.baseItem -eq 'previous') { $df.BaseItem = '(previous)' }
        elseif ($v.baseItem -eq 'next') { $df.BaseItem = '(next)' }
        else { $df.BaseItem = $v.baseItem }
      }
    }
    if ($v.format) { $df.NumberFormat = $v.format }
    $data += $df
  }
  if ($case.valuesAxis -eq 'row' -and $case.values.Count -gt 1) {
    $table.DataPivotField.Orientation = 1
  }
  foreach ($f in $case.fields) {
    $field = $table.PivotFields($f.name)
    if ($f.hide) { foreach ($item in $f.hide) { $field.PivotItems($item).Visible = $false } }
    if ($f.page) { $field.CurrentPage = $f.page }
    if ($f.pages) {
      $field.EnableMultiplePageItems = $true
      foreach ($item in $field.PivotItems()) {
        $item.Visible = ($f.pages -contains $item.Name)
      }
    }
    if ($f.sort) { $field.AutoSort(@{ ascending = 1; descending = 2 }[$f.sort], $field.SourceName) }
    if ($f.sortBy) { $field.AutoSort(@{ ascending = 1; descending = 2 }[$f.sortOrder], $f.sortBy) }
    if ($null -ne $f.subtotals) {
      # Automatic, Sum, Count, Average, Max, Min, Product, Count Nums, StdDev, StdDevp, Var, Varp
      $flags = @($false) * 12
      foreach ($k in $f.subtotals) { $flags[[int]$k] = $true }
      $field.Subtotals = $flags
    }
    if ($f.subtotalBottom) { $field.LayoutSubtotalLocation = 2 }
    if ($f.blankLine) { $field.LayoutBlankLine = $true }
    if ($f.showAll) { $field.ShowAllItems = $true }
    if ($f.repeatLabels) { $field.RepeatLabels = $true }
    if ($f.form) {
      if ($f.form -eq 'tabular') { $field.LayoutForm = 0 } else { $field.LayoutForm = 1 }
      if ($f.form -eq 'compact') { $field.LayoutCompactRow = $true }
      if ($f.form -eq 'outline') { $field.LayoutCompactRow = $false }
    }
    if ($f.caption) { $field.Caption = $f.caption }
    if ($f.rename) { foreach ($p in $f.rename.PSObject.Properties) { $field.PivotItems($p.Name).Caption = $p.Value } }
    if ($f.collapse) { foreach ($item in $f.collapse) { $field.PivotItems($item).ShowDetail = $false } }
    if ($f.group) {
      $cell = $table.PivotFields($f.name).DataRange.Cells.Item(1, 1)
      if ($f.group -eq 'numbers') {
        $cell.Group([double]$f.start, [double]$f.end, [double]$f.by) | Out-Null
      } else {
        # Seconds, Minutes, Hours, Days, Months, Quarters, Years
        $periods = @($false) * 7
        foreach ($k in $f.periods) { $periods[[int]$k] = $true }
        $cell.Group($true, $true, 1, $periods) | Out-Null
      }
    }
  }
  if ($null -ne $case.rowGrand) { $table.RowGrand = [bool]$case.rowGrand }
  if ($null -ne $case.colGrand) { $table.ColumnGrand = [bool]$case.colGrand }
  if ($case.grandName) { $table.GrandTotalName = $case.grandName }
  if ($case.rowHeader) { $table.CompactLayoutRowHeader = $case.rowHeader }
  if ($case.style) { $table.TableStyle2 = $case.style }
  if ($case.blankRows) { $table.RowAxisLayout(0); $table.LayoutBlankLine = $true }
  if ($case.repeatAll) { $table.RepeatAllLabels(2) }
  if ($case.emptyText) { $table.DisplayNullString = $true; $table.NullString = $case.emptyText }
  if ($case.errorText) { $table.DisplayErrorString = $true; $table.ErrorString = $case.errorText }
  if ($null -ne $case.headers) { $table.DisplayFieldCaptions = [bool]$case.headers }
  $table.ManualUpdate = $false
  $table.RefreshTable() | Out-Null
}

try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.ScreenUpdating = $false
  $excel.AutomationSecurity = 3
  $book = $excel.Workbooks.Add()
  $data = $book.Worksheets.Item(1)
  $data.Name = 'Data'
  for ($r = 0; $r -lt $spec.rows.Count; $r++) {
    $row = $spec.rows[$r]
    for ($c = 0; $c -lt $row.Count; $c++) { Set-Cell $data ($r + 1) ($c + 1) $row[$c] }
  }
  $index = 1
  foreach ($case in $spec.cases) {
    try { Build $book $case $index }
    catch { throw ('Case ' + $case.name + ': ' + $_.Exception.Message) }
    $index++
  }
  $data.Activate()
  $book.SaveAs($Out, 51)

  # What this Excel calls things, in its own language.
  $probe = $book.Worksheets.Item($spec.cases[0].name).PivotTables(1)
  $names = @{
    rowHeader = [string]$probe.CompactLayoutRowHeader
    colHeader = [string]$probe.CompactLayoutColumnHeader
    grandTotal = [string]$probe.GrandTotalName
    values = [string]$probe.DataPivotField.Caption
  }
  $names | ConvertTo-Json | Set-Content -LiteralPath $Captions -Encoding UTF8

  foreach ($edit in $spec.edits) { Set-Cell $data ([int]$edit.row) ([int]$edit.col) $edit.value }
  $book.RefreshAll()
  $excel.CalculateUntilAsyncQueriesDone()
  $book.SaveAs($Refreshed, 51)
  $book.Close($false)
}
finally {
  $excel.Quit()
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
}
