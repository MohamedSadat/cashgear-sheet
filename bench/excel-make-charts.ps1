# Has Excel make a workbook of charts, as Excel 2016 makes them, and export each chart as a PNG.
# Builds a new workbook, saves it to -Out, and writes PNGs to -Pictures. Opens no existing file.
param(
  [Parameter(Mandatory)][string]$Out,
  [Parameter(Mandatory)][string]$Pictures
)
$ErrorActionPreference = 'Stop'
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.ScreenUpdating = $false
  $book = $excel.Workbooks.Add()
  $sheet = $book.Worksheets.Item(1)
  $sheet.Name = 'Data'
  $rows = @(
    @('', 'Sales', 'Costs', 'Profit'),
    @('Jan', 120, 80, 40),
    @('Feb', 150, 90, 60),
    @('Mar', 170, 120, 50),
    @('Apr', 140, 100, 40),
    @('May', 190, 110, 80),
    @('Jun', 210, 130, 80)
  )
  for ($r = 0; $r -lt $rows.Count; $r++) {
    for ($c = 0; $c -lt 4; $c++) {
      $value = $rows[$r][$c]
      if ($value -is [int]) { $sheet.Cells.Item($r + 1, $c + 1).Value2 = [double]$value }
      elseif ($value -ne '') { $sheet.Cells.Item($r + 1, $c + 1).Value2 = [string]$value }
    }
  }
  # Height and weight, for the scatter chart.
  $sheet.Cells.Item(1, 6).Value2 = 'Height'
  $sheet.Cells.Item(1, 7).Value2 = 'Weight'
  $pairs = @(@(150, 52), @(158, 57), @(163, 61), @(168, 66), @(172, 70), @(177, 77), @(181, 79), @(186, 88))
  for ($i = 0; $i -lt $pairs.Count; $i++) {
    $sheet.Cells.Item($i + 2, 6).Value2 = [double]$pairs[$i][0]
    $sheet.Cells.Item($i + 2, 7).Value2 = [double]$pairs[$i][1]
  }
  $months = $sheet.Range('A1:D7')
  # Name, Excel chart type, source, and how to finish it.
  $charts = @(
    @('Clustered column', 51, 'A1:D7', ''),
    @('Clustered bar', 57, 'A1:D7', ''),
    @('Line with markers', 65, 'A1:D7', ''),
    @('Pie', 5, 'A1:B7', ''),
    @('Scatter', -4169, 'F1:G9', ''),
    @('Combo', 51, 'A1:D7', 'combo'),
    @('Stacked area', 76, 'A1:C7', ''),
    @('Doughnut', -4120, 'A1:B7', ''),
    @('Stacked column', 52, 'A1:D7', '')
  )
  $index = 0
  foreach ($entry in $charts) {
    $left = 400 + ($index % 3) * 380
    $top = 10 + [math]::Floor($index / 3) * 230
    $shape = $sheet.Shapes.AddChart2(-1, [int]$entry[1], $left, $top, 360, 216)
    $shape.Name = $entry[0]
    $chart = $shape.Chart
    $chart.SetSourceData($sheet.Range($entry[2]))
    if ($entry[3] -eq 'combo') {
      $series = $chart.SeriesCollection(3)
      $series.ChartType = 65
      $series.AxisGroup = 2
    }
    $index++
  }
  $book.SaveAs($Out, 51)
  [System.IO.Directory]::CreateDirectory($Pictures) | Out-Null
  $index = 0
  foreach ($object in $sheet.ChartObjects()) {
    $path = Join-Path $Pictures ('chart-' + $index + '-excel.png')
    [void]$object.Chart.Export($path, 'PNG')
    $index++
  }
  $book.Close($false)
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
