# Phase 13's Excel check: opens a workbook the app saved, says whether Excel needed to repair it,
# reads what the probe asks for as Excel understands it, and saves Excel's own copy for the app to
# read back. Generated workbooks only: the file opens read-only with macros forced off and links not
# updated, and only the copy is written.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][string]$Probe,
  [Parameter(Mandatory)][string]$Copy,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
if (-not ('CgsMessageFilter' -as [type])) {
  Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
[ComImport, Guid("00000016-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface ICgsOleMessageFilter {
  [PreserveSig] int HandleInComingCall(int callType, IntPtr caller, int tickCount, IntPtr interfaceInfo);
  [PreserveSig] int RetryRejectedCall(IntPtr callee, int tickCount, int rejectType);
  [PreserveSig] int MessagePending(IntPtr callee, int tickCount, int pendingType);
}
public class CgsMessageFilter : ICgsOleMessageFilter {
  [DllImport("Ole32.dll")]
  static extern int CoRegisterMessageFilter(ICgsOleMessageFilter filter, out ICgsOleMessageFilter old);
  public static void Register() { ICgsOleMessageFilter old; CoRegisterMessageFilter(new CgsMessageFilter(), out old); }
  public static void Revoke() { ICgsOleMessageFilter old; CoRegisterMessageFilter(null, out old); }
  int ICgsOleMessageFilter.HandleInComingCall(int callType, IntPtr caller, int tickCount, IntPtr interfaceInfo) { return 0; }
  int ICgsOleMessageFilter.RetryRejectedCall(IntPtr callee, int tickCount, int rejectType) {
    if (rejectType == 2 && tickCount < 120000) { return 200; }
    return -1;
  }
  int ICgsOleMessageFilter.MessagePending(IntPtr callee, int tickCount, int pendingType) { return 2; }
}
"@
}
[CgsMessageFilter]::Register()
$logs = Join-Path $env:TEMP '*.xml'
function Get-RepairLogs {
  Get-ChildItem -Path $logs -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^error\d+(_\d+)?\.xml$' } |
    ForEach-Object { $_.FullName }
}
# Not $probe: names ignore case, and the [string] parameter would turn it back into text.
$spec = Get-Content -Raw $Probe | ConvertFrom-Json
$before = @(Get-RepairLogs)
$result = [ordered]@{ opened = $false; repaired = $false; error = $null; sheets = @(); cells = @(); book = $null }
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  try {
    $book = $excel.Workbooks.Open($Workbook, 0, $true)
    $result.opened = $true
    if ($book.Name -ne [System.IO.Path]::GetFileName($Workbook)) { $result.repaired = $true }
    if (@(Get-RepairLogs).Count -gt $before.Count) { $result.repaired = $true }
    $window = $book.Windows.Item(1)
    $active = [string]$book.ActiveSheet.Name
    $sheets = @()
    foreach ($sheet in $book.Worksheets) {
      $entry = [ordered]@{
        name = [string]$sheet.Name
        visible = [int]$sheet.Visible
        active = ([string]$sheet.Name -eq $active)
        protected = [bool]$sheet.ProtectContents
        objects = [bool]$sheet.ProtectDrawingObjects
        scenarios = [bool]$sheet.ProtectScenarios
        selection = [int]$sheet.EnableSelection
      }
      $allow = $sheet.Protection
      $entry.allow = [ordered]@{
        formatCells = [bool]$allow.AllowFormattingCells
        formatColumns = [bool]$allow.AllowFormattingColumns
        formatRows = [bool]$allow.AllowFormattingRows
        insertColumns = [bool]$allow.AllowInsertingColumns
        insertRows = [bool]$allow.AllowInsertingRows
        insertHyperlinks = [bool]$allow.AllowInsertingHyperlinks
        deleteColumns = [bool]$allow.AllowDeletingColumns
        deleteRows = [bool]$allow.AllowDeletingRows
        sort = [bool]$allow.AllowSorting
        autoFilter = [bool]$allow.AllowFiltering
        pivotTables = [bool]$allow.AllowUsingPivotTables
      }
      # Zoom is the window's, for the sheet showing in it.
      if ([int]$sheet.Visible -eq -1) {
        $sheet.Activate()
        $entry.zoom = [int]$window.Zoom
      }
      $sheets += , $entry
    }
    # Back to the sheet the file opened on, for the copy.
    $book.Worksheets.Item($active).Activate()
    $result.sheets = $sheets
    $cells = @()
    foreach ($ask in @($spec.cells)) {
      if ($null -eq $ask) { continue }
      $range = $book.Worksheets.Item([string]$ask.sheet).Range([string]$ask.cell)
      $cell = [ordered]@{
        sheet = [string]$ask.sheet
        cell = [string]$ask.cell
        locked = [bool]$range.Locked
        hidden = [bool]$range.FormulaHidden
        formula = [string]$range.Formula
        links = [int]$range.Hyperlinks.Count
        underline = [int]$range.Font.Underline
        style = [string]$range.Style.Name
        bold = [bool]$range.Font.Bold
        italic = [bool]$range.Font.Italic
        number = [string]$range.NumberFormat
        value = [string]$range.Value2
        note = $null
      }
      # Error checking: the rules Excel finds the cell breaks, and those `ignoredErrors` turns off.
      $rules = @{ 1 = 'evaluateToError'; 2 = 'textDate'; 3 = 'numberAsText'; 4 = 'inconsistentFormula'; 5 = 'omittedCells'; 6 = 'unlockedFormula'; 7 = 'emptyReference' }
      $found = @(); $ignored = @()
      foreach ($n in 1..7) {
        $check = $range.Errors.Item($n)
        if ($check.Value) { $found += $rules[$n] }
        if ($check.Ignore) { $ignored += $rules[$n] }
      }
      $cell.errors = ($found -join ',')
      # Each character's look, as `Characters` reads it: b, i, ^ for superscript, _ for subscript.
      $looks = @()
      $count = [int]$range.Characters().Count
      if ($ask.chars -and $count -le 200) {
        for ($at = 1; $at -le $count; $at++) {
          $font = $range.Characters($at, 1).Font
          $code = ''
          if ($font.Bold) { $code += 'b' }
          if ($font.Italic) { $code += 'i' }
          if ($font.Superscript) { $code += '^' }
          if ($font.Subscript) { $code += '_' }
          $looks += $code
        }
      }
      $cell.chars = ($looks -join '|')
      $cell.ignored = ($ignored -join ',')
      $comment = $range.Comment
      if ($null -ne $comment) {
        $cell.note = [ordered]@{
          author = [string]$comment.Author
          text = [string]$comment.Text()
          visible = [bool]$comment.Visible
          width = [double]$comment.Shape.Width
          height = [double]$comment.Shape.Height
        }
      }
      $cells += , $cell
    }
    $result.cells = $cells
    # Excel's own copy, before trying passwords changes anything.
    $book.SaveCopyAs($Copy)
    $structure = [bool]$book.ProtectStructure
    # Passwords, tried last since unprotecting changes the open copy.
    $unprotected = @()
    foreach ($try in @($spec.unprotect)) {
      if ($null -eq $try) { continue }
      $ok = $true
      try {
        if ($try.sheet) { $book.Worksheets.Item([string]$try.sheet).Unprotect([string]$try.password) }
        else { $book.Unprotect([string]$try.password) }
      } catch { $ok = $false }
      if ($try.sheet) { $still = [bool]$book.Worksheets.Item([string]$try.sheet).ProtectContents }
      else { $still = [bool]$book.ProtectStructure }
      $unprotected += , [ordered]@{ sheet = [string]$try.sheet; password = [string]$try.password; unprotected = ($ok -and -not $still) }
    }
    # Pivot tables: each sheet's, with its report filter's item and its first column as Excel
    # shows it once it has refreshed the table itself. Tables in the file are counted too.
    $pivots = @()
    if ($spec.pivots) {
      foreach ($sheet in $book.Worksheets) {
        $count = [int]$sheet.PivotTables().Count
        for ($at = 1; $at -le $count; $at++) {
          $pt = $sheet.PivotTables($at)
          [void]$pt.RefreshTable()
          $first = @()
          foreach ($c in $pt.TableRange1.Columns.Item(1).Cells) { $first += [string]$c.Text }
          $page = ''
          foreach ($pf in $pt.PageFields()) { $page = [string]$pf.CurrentPage.Name }
          $pivots += , [ordered]@{ sheet = [string]$sheet.Name; name = [string]$pt.Name; page = $page; labels = ($first -join '|') }
        }
      }
    }
    $lists = @()
    foreach ($sheet in $book.Worksheets) {
      foreach ($lo in $sheet.ListObjects) {
        $lists += , [ordered]@{ sheet = [string]$sheet.Name; name = [string]$lo.Name; style = [string]$lo.TableStyle.Name; range = [string]$lo.Range.Address() }
      }
    }
    # Named styles: what each includes, as the Style dialog shows it.
    $styles = @()
    foreach ($name in @($spec.styles)) {
      if ($null -eq $name) { continue }
      $style = $book.Styles.Item([string]$name)
      $styles += , [ordered]@{
        name = [string]$name
        builtIn = [bool]$style.BuiltIn
        number = [bool]$style.IncludeNumber
        font = [bool]$style.IncludeFont
        fill = [bool]$style.IncludePatterns
        border = [bool]$style.IncludeBorder
        bold = [bool]$style.Font.Bold
        italic = [bool]$style.Font.Italic
      }
    }
    $result.book = [ordered]@{ structure = $structure; unprotected = $unprotected; styles = $styles; pivots = $pivots; tables = $lists }
    $book.Close($false)
  } catch {
    $result.error = $_.Exception.Message
  }
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
  [CgsMessageFilter]::Revoke()
}
$result | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 $Out
