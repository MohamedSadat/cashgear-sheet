# Reads back how Excel understands a saved workbook's formatting, cell by cell, and saves a copy of
# it as Excel writes it, for our reader to read in turn. Generated and private workbooks only: public
# files never open in Excel. The file opens read-only with macros and links off, and the copy goes where
# the caller says, which is a temporary folder. The file it was given is never saved.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][string]$Cells,
  [Parameter(Mandatory)][string]$Copy,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
$wanted = Get-Content -Raw -LiteralPath $Cells | ConvertFrom-Json
$excel = New-Object -ComObject Excel.Application
$result = @{ opened = $false; error = $null; cells = @(); rows = @(); cols = @(); sheets = @() }

function Line($range, [int]$index) {
  $border = $range.Borders.Item($index)
  @{ s = [int]$border.LineStyle; w = [int]$border.Weight; c = [double]$border.Color }
}

try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  try {
    $book = $excel.Workbooks.Open($Workbook, 0, $true)
    $result.opened = $true
    foreach ($entry in $wanted.cells) {
      $sheet = $book.Worksheets.Item([int]$entry.sheet + 1)
      $cell = $sheet.Cells.Item([int]$entry.row, [int]$entry.col)
      $font = $cell.Font
      $inside = $cell.Interior
      $result.cells += @{
        sheet = [int]$entry.sheet
        row = [int]$entry.row
        col = [int]$entry.col
        text = [string]$cell.Text
        fontName = [string]$font.Name
        fontSize = [double]$font.Size
        bold = [bool]$font.Bold
        italic = [bool]$font.Italic
        strike = [bool]$font.Strikethrough
        underline = [int]$font.Underline
        fontColor = [double]$font.Color
        pattern = [int]$inside.Pattern
        fillColor = [double]$inside.Color
        patternColor = [double]$inside.PatternColor
        horizontal = [int]$cell.HorizontalAlignment
        vertical = [int]$cell.VerticalAlignment
        wrap = [bool]$cell.WrapText
        shrink = [bool]$cell.ShrinkToFit
        indent = [int]$cell.IndentLevel
        orientation = [int]$cell.Orientation
        readingOrder = [int]$cell.ReadingOrder
        merged = [bool]$cell.MergeCells
        mergeArea = [string]$cell.MergeArea.Address($false, $false)
        left = (Line $cell 7)
        top = (Line $cell 8)
        bottom = (Line $cell 9)
        right = (Line $cell 10)
        diagonalDown = (Line $cell 5)
        diagonalUp = (Line $cell 6)
      }
    }
    foreach ($entry in $wanted.rows) {
      $sheet = $book.Worksheets.Item([int]$entry.sheet + 1)
      $line = $sheet.Rows.Item([int]$entry.line)
      $result.rows += @{
        sheet = [int]$entry.sheet
        line = [int]$entry.line
        height = [double]$line.RowHeight
        hidden = [bool]$line.Hidden
      }
    }
    foreach ($entry in $wanted.cols) {
      $sheet = $book.Worksheets.Item([int]$entry.sheet + 1)
      $line = $sheet.Columns.Item([int]$entry.line)
      $result.cols += @{
        sheet = [int]$entry.sheet
        line = [int]$entry.line
        # In points, whatever the file's characters come to: it says what's on screen.
        width = [double]$line.Width
        hidden = [bool]$line.Hidden
      }
    }
    # Panes belong to a window, which shows one sheet at a time.
    foreach ($sheet in $book.Worksheets) {
      $sheet.Activate()
      $window = $excel.ActiveWindow
      $result.sheets += @{
        name = [string]$sheet.Name
        frozen = [bool]$window.FreezePanes
        splitRow = [int]$window.SplitRow
        splitColumn = [int]$window.SplitColumn
        gridlines = [bool]$window.DisplayGridlines
        headings = [bool]$window.DisplayHeadings
      }
    }
    $book.SaveAs($Copy, 51)
    $book.Close($false)
  } catch {
    $result.error = $_.Exception.Message
  }
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
[System.IO.File]::WriteAllText($Out, (ConvertTo-Json -InputObject $result -Depth 5 -Compress), [System.Text.UTF8Encoding]::new($false))
