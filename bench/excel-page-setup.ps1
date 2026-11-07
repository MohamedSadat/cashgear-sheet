# Reads the first worksheet's page setup as Excel understands it, says whether the file needed a
# repair, and saves Excel's own copy for the app to read back. Generated workbooks only: the file
# opens read-only with macros forced off and links not updated, and only the copy is written.
param(
  [Parameter(Mandatory)][string]$Workbook,
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
$before = @(Get-RepairLogs)
$result = @{ opened = $false; repaired = $false; error = $null; setup = $null }
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
    $sheet = $book.Worksheets.Item(1)
    $ps = $sheet.PageSetup
    $zoom = $ps.Zoom
    $breaks = @()
    foreach ($break in $sheet.HPageBreaks) {
      if ([int]$break.Type -eq -4135) { $breaks += [int]$break.Location.Row }
    }
    $colBreaks = @()
    foreach ($break in $sheet.VPageBreaks) {
      if ([int]$break.Type -eq -4135) { $colBreaks += [int]$break.Location.Column }
    }
    $result.setup = @{
      orientation = [int]$ps.Orientation
      paper = [int]$ps.PaperSize
      zoom = if ($zoom -is [bool]) { 0 } else { [int]$zoom }
      fitWide = if ($ps.FitToPagesWide -is [bool]) { -1 } else { [int]$ps.FitToPagesWide }
      fitTall = if ($ps.FitToPagesTall -is [bool]) { -1 } else { [int]$ps.FitToPagesTall }
      margins = @([double]$ps.LeftMargin, [double]$ps.RightMargin, [double]$ps.TopMargin, [double]$ps.BottomMargin, [double]$ps.HeaderMargin, [double]$ps.FooterMargin)
      centerH = [bool]$ps.CenterHorizontally
      centerV = [bool]$ps.CenterVertically
      gridlines = [bool]$ps.PrintGridlines
      headings = [bool]$ps.PrintHeadings
      blackAndWhite = [bool]$ps.BlackAndWhite
      draft = [bool]$ps.Draft
      comments = [int]$ps.PrintComments
      errors = [int]$ps.PrintErrors
      order = [int]$ps.Order
      firstPage = [string]$ps.FirstPageNumber
      header = @([string]$ps.LeftHeader, [string]$ps.CenterHeader, [string]$ps.RightHeader)
      footer = @([string]$ps.LeftFooter, [string]$ps.CenterFooter, [string]$ps.RightFooter)
      oddAndEven = [bool]$ps.OddAndEvenPagesHeaderFooter
      differentFirst = [bool]$ps.DifferentFirstPageHeaderFooter
      evenFooter = [string]$ps.EvenPage.CenterFooter.Text
      firstHeader = [string]$ps.FirstPage.CenterHeader.Text
      printArea = [string]$ps.PrintArea
      titleRows = [string]$ps.PrintTitleRows
      titleCols = [string]$ps.PrintTitleColumns
      rowBreaks = $breaks
      colBreaks = $colBreaks
    }
    # Excel's own save of what it read, for the app to read back.
    $book.SaveCopyAs($Copy)
    $book.Close($false)
  } catch { $result.error = $_.Exception.Message }
} finally {
  $excel.Quit()
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
  [CgsMessageFilter]::Revoke()
}
$result | ConvertTo-Json -Depth 5 | Set-Content -Path $Out -Encoding utf8
