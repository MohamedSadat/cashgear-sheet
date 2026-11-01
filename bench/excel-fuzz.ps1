# Applies random structural edits in Excel over COM and writes what Excel shows after
# each one, for bench/harness/src/bin/structural.rs to compare with the engines. Opens only
# the generated seed workbook, read-only with macros and links off, and never saves it.
param(
    [Parameter(Mandatory = $true)][string]$Seed,
    [Parameter(Mandatory = $true)][string]$Ops,
    [Parameter(Mandatory = $true)][string]$Out
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
}

function Quote([string]$text) {
    '"' + [System.Web.HttpUtility]::JavaScriptStringEncode($text) + '"'
}

function Convert-Cell($value) {
    if ($value -is [double]) { return '{"n":' + $value.ToString('R', [Globalization.CultureInfo]::InvariantCulture) + '}' }
    if ($value -is [bool]) { return '{"b":' + $value.ToString().ToLowerInvariant() + '}' }
    if ($value -is [int]) { return '{"e":' + (Quote $errorText[$value]) + '}' }
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
                if ($null -eq $value) { continue }
                $formula = [string]$formulas[$r, $c]
                $text = if ($formula.StartsWith('=')) { ',"f":' + (Quote $formula) } else { '' }
                $cells.Add('{"r":' + ($top + $r - 1) + ',"c":' + ($left + $c - 1) + ',"v":' + (Convert-Cell $value) + $text + '}')
            }
        }
    } elseif ($null -ne $values) {
        $formula = [string]$formulas
        $text = if ($formula.StartsWith('=')) { ',"f":' + (Quote $formula) } else { '' }
        $cells.Add('{"r":' + $top + ',"c":' + $left + ',"v":' + (Convert-Cell $values) + $text + '}')
    }
    return '[' + ($cells -join ',') + ']'
}

$episodes = Get-Content -Raw $Ops | ConvertFrom-Json
$excel = New-Object -ComObject Excel.Application
$writer = New-Object System.IO.StreamWriter($Out, $false, [Text.UTF8Encoding]::new($false))
try {
    $excel.Visible = $false
    $excel.DisplayAlerts = $false
    $excel.AutomationSecurity = 3
    $excel.ScreenUpdating = $false
    foreach ($episode in $episodes) {
        $book = $excel.Workbooks.Open($Seed, 0, $true)
        $step = 0
        foreach ($op in $episode.ops) {
            $sheet = $book.Worksheets.Item([int]$op.sheet + 1)
            $last = [int]$op.at + [int]$op.count - 1
            switch ($op.kind) {
                'InsertRows' { [void]$sheet.Rows("$($op.at):$last").Insert() }
                'DeleteRows' { [void]$sheet.Rows("$($op.at):$last").Delete() }
                'InsertCols' { [void]$sheet.Range($sheet.Cells.Item(1, [int]$op.at), $sheet.Cells.Item(1, $last)).EntireColumn.Insert() }
                'DeleteCols' { [void]$sheet.Range($sheet.Cells.Item(1, [int]$op.at), $sheet.Cells.Item(1, $last)).EntireColumn.Delete() }
            }
            $excel.Calculate()
            $dump = @()
            foreach ($target in $book.Worksheets) { $dump += (Convert-Sheet $target) }
            $writer.WriteLine('{"episode":' + $episode.episode + ',"step":' + $step + ',"sheets":[' + ($dump -join ',') + ']}')
            $step++
        }
        $book.Close($false)
    }
} finally {
    $writer.Dispose()
    $excel.Quit()
    [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
