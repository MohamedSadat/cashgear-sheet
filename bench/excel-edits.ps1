# Drives Excel over COM for bench/run-edit-fuzz.mjs. Three modes:
#   seed: builds the generated seed workbook and saves it. Macros stay off.
#   edits: applies each episode's edits to that seed and dumps every sheet after each. The seed
#     opens read-only and is never saved.
#   saved: opens each file a save made, which is where a repair would show, and dumps it.
# Only generated workbooks and what we saved from them come through here.
param(
  [Parameter(Mandatory)][ValidateSet('seed', 'edits', 'saved')][string]$Mode,
  [string]$Seed,
  [string]$Ops,
  [string]$Out,
  [string]$Folder
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Web

$errorText = @{
  -2146826288 = '#NULL!'
  -2146826281 = '#DIV/0!'
  -2146826273 = '#VALUE!'
  -2146826265 = '#REF!'
  -2146826259 = '#NAME?'
  -2146826252 = '#NUM!'
  -2146826246 = '#N/A'
  -2146826238 = '#GETTING_DATA'
  -2146826237 = '#SPILL!'
  -2146826236 = '#CONNECT!'
  -2146826235 = '#BLOCKED!'
  -2146826234 = '#UNKNOWN!'
  -2146826233 = '#FIELD!'
  -2146826232 = '#CALC!'
}

function Quote([string]$text) {
  '"' + [System.Web.HttpUtility]::JavaScriptStringEncode($text) + '"'
}

function Convert-Cell($value) {
  if ($value -is [double]) { return '{"n":' + $value.ToString('R', [Globalization.CultureInfo]::InvariantCulture) + '}' }
  if ($value -is [bool]) { return '{"b":' + $value.ToString().ToLowerInvariant() + '}' }
  if ($value -is [int]) {
    $name = $errorText[$value]
    if (-not $name) { $name = "#ERR$value" }
    return '{"e":' + (Quote $name) + '}'
  }
  return '{"s":' + (Quote ([string]$value)) + '}'
}

function Convert-Sheet($sheet) {
  $used = $sheet.UsedRange
  $top = [int]$used.Row
  $left = [int]$used.Column
  $values = $used.Value2
  $formulas = $used.Formula
  $cells = New-Object System.Collections.Generic.List[string]
  if ($values -is [array]) {
    $rows = $values.GetLength(0)
    $cols = $values.GetLength(1)
    for ($r = 1; $r -le $rows; $r++) {
      for ($c = 1; $c -le $cols; $c++) {
        $value = $values[$r, $c]
        $formula = [string]$formulas[$r, $c]
        if ($null -eq $value -and -not $formula.StartsWith('=')) { continue }
        if ($null -eq $value) { $value = '' }
        $text = if ($formula.StartsWith('=')) { ',"f":' + (Quote $formula) } else { '' }
        $cells.Add('{"r":' + ($top + $r - 1) + ',"c":' + ($left + $c - 1) + ',"v":' + (Convert-Cell $value) + $text + '}')
      }
    }
  } elseif ($null -ne $values) {
    $formula = [string]$formulas
    $text = if ($formula.StartsWith('=')) { ',"f":' + (Quote $formula) } else { '' }
    $cells.Add('{"r":' + $top + ',"c":' + $left + ',"v":' + (Convert-Cell $values) + $text + '}')
  }
  return '{"name":' + (Quote $sheet.Name) + ',"cells":[' + ($cells -join ',') + ']}'
}

function Convert-Book($book) {
  $dump = @()
  foreach ($target in $book.Worksheets) { $dump += (Convert-Sheet $target) }
  return '[' + ($dump -join ',') + ']'
}

function Area($sheet, $area) {
  # The comma keeps PowerShell from unrolling the range into its cells.
  return , $sheet.Range($sheet.Cells.Item([int]$area.top, [int]$area.left), $sheet.Cells.Item([int]$area.bottom, [int]$area.right))
}

# Does one edit. Sheets are named by position at the time, from 1.
function Apply-Op($book, $op) {
  switch ($op.kind) {
    'input' {
      $sheet = $book.Worksheets.Item([int]$op.pos)
      $sheet.Cells.Item([int]$op.row, [int]$op.col).Formula = [string]$op.text
    }
    'clear' {
      $sheet = $book.Worksheets.Item([int]$op.pos)
      foreach ($area in $op.areas) { [void](Area $sheet $area).ClearContents() }
    }
    'fill' {
      $sheet = $book.Worksheets.Item([int]$op.pos)
      $source = Area $sheet $op.from
      $target = Area $sheet $op.to
      [void]$source.AutoFill($target, 0)
    }
    'paste' {
      $from = $book.Worksheets.Item([int]$op.fromPos)
      $to = $book.Worksheets.Item([int]$op.pos)
      $source = Area $from $op.from
      $target = Area $to $op.to
      [void]$source.Copy($target)
      $book.Application.CutCopyMode = 0
    }
    'shiftCells' {
      $sheet = $book.Worksheets.Item([int]$op.pos)
      $range = Area $sheet $op.area
      if ($op.delete) {
        # xlUp and xlToLeft.
        $direction = if ($op.down) { -4162 } else { -4159 }
        [void]$range.Delete($direction)
      } else {
        # xlDown and xlToRight.
        $direction = if ($op.down) { -4121 } else { -4161 }
        [void]$range.Insert($direction)
      }
    }
    'move' {
      $from = $book.Worksheets.Item([int]$op.fromPos)
      $to = $book.Worksheets.Item([int]$op.pos)
      $source = Area $from $op.from
      $target = $to.Cells.Item([int]$op.row, [int]$op.col)
      [void]$source.Cut($target)
    }
    'axis' {
      $sheet = $book.Worksheets.Item([int]$op.pos)
      $last = [int]$op.at + [int]$op.count - 1
      if ($op.axis -eq 'row') {
        $range = $sheet.Rows("$($op.at):$last")
      } else {
        $range = $sheet.Range($sheet.Cells.Item(1, [int]$op.at), $sheet.Cells.Item(1, $last)).EntireColumn
      }
      if ($op.delete) { [void]$range.Delete() } else { [void]$range.Insert() }
    }
    'insertSheet' {
      $count = $book.Worksheets.Count
      if ([int]$op.position -ge $count) {
        $sheet = $book.Worksheets.Add([Type]::Missing, $book.Worksheets.Item($count))
      } else {
        $sheet = $book.Worksheets.Add($book.Worksheets.Item([int]$op.position + 1))
      }
      $sheet.Name = [string]$op.name
    }
    'deleteSheet' { [void]$book.Worksheets.Item([int]$op.pos).Delete() }
    'renameSheet' { $book.Worksheets.Item([int]$op.pos).Name = [string]$op.name }
    default { throw "unknown op $($op.kind)" }
  }
}

function Open-Excel {
  $excel = New-Object -ComObject Excel.Application
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  $excel.ScreenUpdating = $false
  # Typing beside a table doesn't grow it, and a formula in one doesn't fill its column.
  $excel.AutoCorrect.AutoExpandListRange = $false
  $excel.AutoCorrect.AutoFillFormulasInLists = $false
  return $excel
}

function Build-Seed($excel, $target) {
  $book = $excel.Workbooks.Add()
  while ($book.Worksheets.Count -lt 3) { [void]$book.Worksheets.Add([Type]::Missing, $book.Worksheets.Item($book.Worksheets.Count)) }
  $data = $book.Worksheets.Item(1); $data.Name = 'Data'
  $calc = $book.Worksheets.Item(2); $calc.Name = 'Calc Sheet'
  $spare = $book.Worksheets.Item(3); $spare.Name = 'Spare'
  $headers = 'Id', 'Name', 'Qty', 'Price', 'Total'
  for ($c = 1; $c -le 5; $c++) { $data.Cells.Item(1, $c).Value2 = $headers[$c - 1] }
  $names = 'pen', 'book', 'ink', 'desk', 'lamp', 'cup', 'mug', 'bag', 'box', 'map', 'cap', 'hat'
  for ($r = 2; $r -le 13; $r++) {
    $data.Cells.Item($r, 1).Value2 = [double]($r - 1)
    $data.Cells.Item($r, 2).Value2 = $names[$r - 2]
    $data.Cells.Item($r, 3).Value2 = [double](($r * 3) % 7 + 1)
    $data.Cells.Item($r, 4).Value2 = [double](($r * 5) % 11) + 0.5
    $data.Cells.Item($r, 5).Formula = "=C$r*D$r"
  }
  # A table with its own references, on the same sheet further along.
  $data.Cells.Item(1, 8).Value2 = 'Item'
  $data.Cells.Item(1, 9).Value2 = 'Cost'
  for ($r = 2; $r -le 6; $r++) {
    $data.Cells.Item($r, 8).Value2 = "t$r"
    $data.Cells.Item($r, 9).Value2 = [double]($r * 10)
  }
  $table = $data.ListObjects.Add(1, $data.Range('H1:I6'), [Type]::Missing, 1)
  $table.Name = 'Sales'
  [void]$book.Names.Add('Rate', "='Calc Sheet'!`$B`$1")
  [void]$book.Names.Add('Prices', '=Data!$D$2:$D$13')
  $calc.Range('A1').Value2 = 'Rate'
  $calc.Range('B1').Value2 = 0.25
  $calc.Range('A2').Value2 = 'Sum'
  $calc.Range('B2').Formula = '=SUM(Data!E2:E13)'
  $calc.Range('A3').Value2 = 'Tax'
  $calc.Range('B3').Formula = '=B2*Rate'
  $calc.Range('A4').Value2 = 'Lookup'
  $calc.Range('B4').Formula = '=VLOOKUP(5,Data!A2:E13,2,FALSE)'
  $calc.Range('A5').Value2 = 'Index'
  $calc.Range('B5').Formula = '=INDEX(Data!C2:C13,MATCH("ink",Data!B2:B13,0))'
  $calc.Range('A6').Value2 = 'Count'
  $calc.Range('B6').Formula = '=COUNTIF(Data!C2:C13,">3")'
  $calc.Range('A7').Value2 = 'Table'
  $calc.Range('B7').Formula = '=SUM(Sales[Cost])'
  $calc.Range('A8').Value2 = 'Names'
  $calc.Range('B8').Formula = '=SUM(Prices)'
  $calc.Range('A9').Value2 = 'Text'
  $calc.Range('B9').Formula = '=TEXT(B2,"0.00")&" total"'
  $calc.Range('A10').Value2 = 'Row'
  $calc.Range('B10').Formula = '=ROW(Data!A5)+COLUMN(Data!C1)'
  $calc.Range('A11').Value2 = 'Absolute'
  $calc.Range('B11').Formula = '=SUM(Data!$C$2:C5)+Data!$E$3'
  $calc.Range('A12').Value2 = 'Whole'
  $calc.Range('B12').Formula = '=SUM(Data!C:C)+COUNTA(Data!2:2)'
  # Legacy arrays. The Excel on the reference machine is 2016, which has no dynamic ones.
  $calc.Range('H1').Value2 = 'Double'
  $calc.Range('H2:H4').FormulaArray = '=Data!A2:A4*2'
  $calc.Range('J2').FormulaArray = '=SUM(Data!C2:C6*2)'
  $spare.Range('A1').Value2 = [double]1
  $spare.Range('A2').Value2 = [double]2
  $spare.Range('A3').Formula = '=A1+A2+Data!A3'
  $spare.Range('B1').Formula = "='Calc Sheet'!B2*2"
  $data.Range('L1:M1').Merge()
  $data.Range('L1').Value2 = 'merged'
  $excel.CalculateFullRebuild()
  $book.SaveAs($target, 51)
  $book.Close($false)
}

$excel = Open-Excel
try {
  if ($Mode -eq 'seed') {
    Build-Seed $excel $Out
  } elseif ($Mode -eq 'edits') {
    $episodes = Get-Content -Raw -Encoding UTF8 $Ops | ConvertFrom-Json
    $writer = New-Object System.IO.StreamWriter($Out, $false, [Text.UTF8Encoding]::new($false))
    try {
      foreach ($episode in $episodes) {
        $step = 0
        $book = $null
        foreach ($action in $episode.steps) {
          $refused = $false
          $problem = ''
          # An undo in the list means Excel has to get back to an earlier state. Its own undo stack
          # doesn't hold edits made over COM, so it opens the seed again and replays what remains.
          if ($null -ne $action.replay -or $null -eq $book) {
            if ($null -ne $book) { $book.Close($false) }
            $book = $excel.Workbooks.Open($Seed, 0, $true)
            foreach ($op in $action.replay) {
              try { Apply-Op $book $op } catch { }
            }
          }
          if ($null -ne $action.op) {
            try { Apply-Op $book $action.op } catch { $refused = $true; $problem = $_.Exception.Message }
          }
          $excel.Calculate()
          # What Excel does with a circle depends on how many times it has calculated, so a chain
          # of them says nothing about us.
          $circular = $false
          foreach ($target in $book.Worksheets) {
            if ($null -ne $target.CircularReference) { $circular = $true }
          }
          $writer.WriteLine('{"episode":' + $episode.episode + ',"step":' + $step + ',"refused":' + $refused.ToString().ToLowerInvariant() + ',"circular":' + $circular.ToString().ToLowerInvariant() + ',"problem":' + (Quote $problem) + ',"sheets":' + (Convert-Book $book) + '}')
          $step++
        }
        if ($null -ne $book) { $book.Close($false) }
      }
    } finally {
      $writer.Dispose()
    }
  } else {
    $writer = New-Object System.IO.StreamWriter($Out, $false, [Text.UTF8Encoding]::new($false))
    try {
      foreach ($file in Get-ChildItem -LiteralPath $Folder -Filter '*.xlsx' | Sort-Object Name) {
        $opened = $true
        $dump = '[]'
        $renamed = $false
        try {
          $book = $excel.Workbooks.Open($file.FullName, 0, $true)
          $renamed = $book.Name -ne $file.Name
          $excel.CalculateFull()
          $dump = Convert-Book $book
          $book.Close($false)
        } catch {
          $opened = $false
        }
        $writer.WriteLine('{"file":' + (Quote $file.Name) + ',"opened":' + $opened.ToString().ToLowerInvariant() + ',"renamed":' + $renamed.ToString().ToLowerInvariant() + ',"sheets":' + $dump + '}')
      }
    } finally {
      $writer.Dispose()
    }
  }
} catch {
  throw ($_.Exception.Message + ' at ' + $_.InvocationInfo.ScriptLineNumber + ': ' + $_.InvocationInfo.Line)
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
