# Has Excel break each visible worksheet into pages and reads where: HPageBreaks and VPageBreaks,
# the page count and the zoom fitting chose. Excel measures with Microsoft Print to PDF as its
# active printer, which this sets for its own session and puts back afterwards. The workbook
# opens read-only with macros forced off and links not updated, and is never saved.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][string]$Out,
  [switch]$IgnorePrintArea
)
$ErrorActionPreference = 'Stop'
# Excel turns calls away while it is busy; this filter has COM try a turned-away call again.
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

# Excel names a printer with its port: "Microsoft Print to PDF on Ne01:".
$devices = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Devices'
$port = ($devices.'Microsoft Print to PDF' -split ',')[1]
if (-not $port) { throw 'Microsoft Print to PDF is not installed.' }
$printer = "Microsoft Print to PDF on $port"

$result = @{ printer = $printer; sheets = @() }
$excel = New-Object -ComObject Excel.Application
$before = $null
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  $book = $excel.Workbooks.Open($Workbook, 0, $true)
  # Excel takes a printer only once a workbook is open.
  $before = [string]$excel.ActivePrinter
  $excel.ActivePrinter = $printer
  $index = 0
  foreach ($sheet in $book.Worksheets) {
    $entry = @{ sheet = $index; name = [string]$sheet.Name; rows = @(); cols = @(); manualRows = @(); manualCols = @(); pages = 0; zoom = 100; error = $null }
    $index++
    if ($sheet.Visible -ne -1) { continue }
    try {
      $sheet.Activate() | Out-Null
      if ($IgnorePrintArea) { $sheet.PageSetup.PrintArea = '' }
      # Excel works breaks out fully only in Page Break Preview.
      $excel.ActiveWindow.View = 2
      # Excel 4's GET.DOCUMENT lists every break of the active sheet: the HPageBreaks and
      # VPageBreaks collections can leave breaks out or refuse an index their Count gave. Over
      # COM it gives only an array's first item, so INDEX reads them one at a time.
      foreach ($kind in @(64, 65)) {
        for ($at = 1; $at -le 5000; $at++) {
          $value = $excel.ExecuteExcel4Macro("INDEX(GET.DOCUMENT($kind),$at)")
          if ($value -isnot [double]) { break }
          if ($kind -eq 64) { $entry.rows += [int]$value } else { $entry.cols += [int]$value }
        }
      }
      $entry.pages = [int]$sheet.PageSetup.Pages.Count
      $zoom = $sheet.PageSetup.Zoom
      $entry.zoom = if ($zoom -is [bool]) { 0 } else { [int]$zoom }
      $excel.ActiveWindow.View = 1
    } catch { $entry.error = $_.Exception.Message }
    $result.sheets += $entry
  }
  $book.Close($false)
} finally {
  if ($before) { try { $excel.ActivePrinter = $before } catch {} }
  $excel.Quit()
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
  [CgsMessageFilter]::Revoke()
}
$result | ConvertTo-Json -Depth 5 | Set-Content -Path $Out -Encoding utf8
