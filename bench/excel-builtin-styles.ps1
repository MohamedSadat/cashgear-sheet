# Dumps Excel's built-in table styles and cell styles over COM, as raw property values for
# run-builtin-styles.mjs to make sense of. Excel starts hidden with macros off and works only on a new
# workbook. That workbook is then given every built-in cell style, a table, a copy of each built-in
# table style and a probe style with one distinct fill per element, and saved once as xlsx to the
# folder the caller names, so the caller can read Excel's own styles.xml. No existing file is opened.
param(
  [Parameter(Mandatory)][string]$Out,
  [Parameter(Mandatory)][string]$Scratch
)
$ErrorActionPreference = 'Stop'

# A property's value, or null when COM has none (DBNull) or refuses to read it.
function Value($object, [string]$name) {
  try {
    $value = $object.$name
    if ($value -is [DBNull]) { return $null }
    return $value
  } catch { return $null }
}

function Colour($object, [string]$prefix = '') {
  [ordered]@{
    colorIndex = Value $object ($prefix + 'ColorIndex')
    color = Value $object ($prefix + 'Color')
    themeColor = Value $object ($prefix + 'ThemeColor')
    tint = Value $object ($prefix + 'TintAndShade')
  }
}

function Font($font) {
  $out = Colour $font
  foreach ($name in 'Name', 'Size', 'Bold', 'Italic', 'Underline', 'Strikethrough', 'ThemeFont') {
    $out[$name.Substring(0, 1).ToLower() + $name.Substring(1)] = Value $font $name
  }
  $out
}

function Interior($interior) {
  $out = Colour $interior
  $out.pattern = Value $interior 'Pattern'
  $out.patternColor = Colour $interior 'Pattern'
  $out.patternColor.tint = Value $interior 'PatternTintAndShade'
  if ($out.pattern -eq 4000 -or $out.pattern -eq 4001) {
    $gradient = $interior.Gradient
    $out.gradient = [ordered]@{ degree = Value $gradient 'Degree'; stops = @() }
    foreach ($stop in $gradient.ColorStops) {
      $out.gradient.stops += [ordered]@{ position = $stop.Position; color = $stop.Color; themeColor = (Value $stop 'ThemeColor'); tint = (Value $stop 'TintAndShade') }
    }
  }
  $out
}

# Borders by XlBordersIndex: 5 and 6 are the diagonals, 7 to 10 the edges, 11 and 12 the insides. A
# Style's edges answer only to xlLeft, xlTop, xlBottom and xlRight (-4131, -4160, -4107, -4152).
function Borders($borders, [int[]]$indices) {
  $out = [ordered]@{}
  foreach ($index in $indices) {
    $border = $borders.Item($index)
    $line = Colour $border
    $line.lineStyle = Value $border 'LineStyle'
    $line.weight = Value $border 'Weight'
    $out[[string]$index] = $line
  }
  $out
}

$excel = New-Object -ComObject Excel.Application
$result = [ordered]@{ version = $null; defaultTableStyle = $null; defaultPivotTableStyle = $null; tableStyles = @(); pivotStyles = @(); cellStyles = @(); saved = $null }
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AutomationSecurity = 3
  $result.version = $excel.Version
  $book = $excel.Workbooks.Add()
  $result.defaultTableStyle = $book.DefaultTableStyle.Name
  $result.defaultPivotTableStyle = $book.DefaultPivotTableStyle.Name

  # Elements 0 to 12 are the table ones of XlTableStyleElementType. The rest are pivot and slicer.
  $tableStyles = @()
  foreach ($style in $book.TableStyles) {
    if (-not $style.BuiltIn -or $style.Name -notlike 'TableStyle*') { continue }
    $tableStyles += $style
    $entry = [ordered]@{
      name = $style.Name
      showAsAvailableTableStyle = $style.ShowAsAvailableTableStyle
      showAsAvailablePivotTableStyle = $style.ShowAsAvailablePivotTableStyle
      elements = @()
    }
    foreach ($type in 0..12) {
      $element = $style.TableStyleElements.Item($type)
      if (-not $element.HasFormat) { continue }
      $entry.elements += [ordered]@{
        type = $type
        stripeSize = Value $element 'StripeSize'
        font = Font $element.Font
        interior = Interior $element.Interior
        borders = Borders $element.Borders @(7, 8, 9, 10, 11, 12)
      }
    }
    $result.tableStyles += $entry
  }

  # Pivot styles use elements 0 to 27: the table ones, then subtotals, subheadings, blank rows and
  # report filters.
  $pivotStyles = @()
  foreach ($style in $book.TableStyles) {
    if (-not $style.BuiltIn -or $style.Name -notlike 'PivotStyle*') { continue }
    $pivotStyles += $style
    $entry = [ordered]@{ name = $style.Name; elements = @() }
    foreach ($type in 0..27) {
      $element = $style.TableStyleElements.Item($type)
      if (-not $element.HasFormat) { continue }
      $entry.elements += [ordered]@{
        type = $type
        stripeSize = Value $element 'StripeSize'
        font = Font $element.Font
        interior = Interior $element.Interior
        borders = Borders $element.Borders @(7, 8, 9, 10, 11, 12)
      }
    }
    $result.pivotStyles += $entry
  }

  $sheet = $book.Worksheets.Item(1)
  $row = 1
  foreach ($style in $book.Styles) {
    if (-not $style.BuiltIn) { continue }
    $alignment = [ordered]@{}
    foreach ($name in 'HorizontalAlignment', 'VerticalAlignment', 'WrapText', 'IndentLevel', 'Orientation', 'ShrinkToFit', 'ReadingOrder') {
      $alignment[$name.Substring(0, 1).ToLower() + $name.Substring(1)] = Value $style $name
    }
    $include = [ordered]@{}
    foreach ($name in 'IncludeNumber', 'IncludeFont', 'IncludeAlignment', 'IncludeBorder', 'IncludePatterns', 'IncludeProtection') {
      $include[$name.Substring(7, 1).ToLower() + $name.Substring(8)] = Value $style $name
    }
    $result.cellStyles += [ordered]@{
      name = $style.Name
      nameLocal = $style.NameLocal
      row = $row
      include = $include
      numberFormat = Value $style 'NumberFormat'
      numberFormatLocal = Value $style 'NumberFormatLocal'
      font = Font $style.Font
      interior = Interior $style.Interior
      borders = Borders $style.Borders @(5, 6, -4131, -4160, -4107, -4152)
      alignment = $alignment
      locked = Value $style 'Locked'
      formulaHidden = Value $style 'FormulaHidden'
    }
    # The cell keeps the style in use, so the saved file lists it with its builtinId.
    $sheet.Cells.Item($row, 1).Style = $style
    $row++
  }

  # What the saved file holds for these: a table on a built-in style, a copy of each built-in style,
  # and a probe whose element N is filled with RGB(N + 1, 0, 0), to check the element types.
  $sheet.Cells.Item(1, 3).Value2 = 'A'
  $sheet.Cells.Item(1, 4).Value2 = 'B'
  $sheet.Cells.Item(2, 3).Value2 = 1
  $sheet.Cells.Item(2, 4).Value2 = 2
  $table = $sheet.ListObjects.Add(1, $sheet.Range('C1:D2'), $null, 1)
  $table.TableStyle = $result.defaultTableStyle
  foreach ($style in $tableStyles) { [void]$style.Duplicate('cgs copy ' + $style.Name) }
  foreach ($style in $pivotStyles) { [void]$style.Duplicate('cgs copy ' + $style.Name) }
  $probe = $book.TableStyles.Add('cgs probe')
  foreach ($type in 0..12) {
    $probe.TableStyleElements.Item($type).Interior.Color = $type + 1
  }
  $pivotProbe = $book.TableStyles.Add('cgs pivot probe')
  $pivotProbe.ShowAsAvailablePivotTableStyle = $true
  $pivotProbe.ShowAsAvailableTableStyle = $false
  foreach ($type in 0..27) {
    $pivotProbe.TableStyleElements.Item($type).Interior.Color = $type + 1
  }
  $path = Join-Path $Scratch 'styles.xlsx'
  $book.SaveAs($path, 51)
  $result.saved = $path
  $book.Close($false)
  $json = ConvertTo-Json $result -Depth 12 -Compress
  [IO.File]::WriteAllText($Out, $json, (New-Object Text.UTF8Encoding $false))
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
  [GC]::Collect()
  [GC]::WaitForPendingFinalizers()
}
