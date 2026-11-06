# Asks Excel how it scales chart axes on its own. Builds each case's chart in a new, unsaved
# workbook and reads the axes' automatic minimum, maximum and major unit. Opens no file.
# Input: a JSON array of cases { type, width, height, values: [..], x: [..] }.
# Output: the same cases with { min, max, unit, xMin, xMax, xUnit }.
param(
  [Parameter(Mandatory)][string]$Cases,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
$list = Get-Content -Raw -Encoding UTF8 $Cases | ConvertFrom-Json
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.ScreenUpdating = $false
  $book = $excel.Workbooks.Add()
  $sheet = $book.Worksheets.Item(1)
  $results = New-Object System.Collections.Generic.List[object]
  foreach ($case in $list) {
    $sheet.Cells.Clear()
    foreach ($shape in @($sheet.Shapes)) { $shape.Delete() }
    $count = $case.values.Count
    $series = 1
    if ($case.series) { $series = [int]$case.series }
    for ($i = 0; $i -lt $count; $i++) {
      $sheet.Cells.Item($i + 1, 1).Value2 = [double]$(if ($case.x) { $case.x[$i] } else { $i + 1 })
      for ($s = 0; $s -lt $series; $s++) {
        $value = [double]$case.values[$i]
        if ($s -gt 0) { $value = [double]$case.values[($i + $s) % $count] }
        $sheet.Cells.Item($i + 1, 2 + $s).Value2 = $value
      }
    }
    $shape = $sheet.Shapes.AddChart2(-1, [int]$case.type, 10, 10, [double]$case.width, [double]$case.height)
    $chart = $shape.Chart
    while ($chart.SeriesCollection().Count -gt 0) { $chart.SeriesCollection(1).Delete() }
    for ($s = 0; $s -lt $series; $s++) {
      $new = $chart.SeriesCollection().NewSeries()
      $new.Values = $sheet.Range($sheet.Cells.Item(1, 2 + $s), $sheet.Cells.Item($count, 2 + $s))
      if ($case.x) { $new.XValues = $sheet.Range($sheet.Cells.Item(1, 1), $sheet.Cells.Item($count, 1)) }
    }
    $chart.Refresh()
    $value = $chart.Axes(2)
    $row = [ordered]@{ id = $case.id; min = [double]$value.MinimumScale; max = [double]$value.MaximumScale; unit = [double]$value.MajorUnit }
    if ($case.x) {
      $axis = $chart.Axes(1)
      $row.xMin = [double]$axis.MinimumScale
      $row.xMax = [double]$axis.MaximumScale
      $row.xUnit = [double]$axis.MajorUnit
    }
    $row.plotHeight = [double]$chart.PlotArea.InsideHeight
    $row.plotWidth = [double]$chart.PlotArea.InsideWidth
    $results.Add($row)
  }
  $book.Close($false)
  [System.IO.File]::WriteAllText($Out, (ConvertTo-Json -InputObject $results.ToArray() -Compress -Depth 4), [System.Text.UTF8Encoding]::new($false))
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
