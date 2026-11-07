# Has Excel break each visible worksheet into pages and reads where: HPageBreaks and VPageBreaks,
# the page count and the zoom fitting chose. Excel measures with Microsoft Print to PDF as its
# active printer, which this sets for its own session and puts back afterwards. The workbook
# opens read-only with macros forced off and links not updated, and is never saved.
# With -List, a file of workbook paths one to a line goes through one Excel session, and each
# workbook's result is a line of JSON appended to -Out. -CountsOnly reads page counts only.
param(
  [string]$Workbook,
  [string]$List,
  [Parameter(Mandatory)][string]$Out,
  [switch]$IgnorePrintArea,
  [switch]$CountsOnly
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

function Start-Excel {
  $app = New-Object -ComObject Excel.Application
  $app.Visible = $false
  $app.DisplayAlerts = $false
  $app.AskToUpdateLinks = $false
  $app.AutomationSecurity = 3
  return $app
}

function Get-Book([string]$path) {
  $result = @{ printer = $printer; sheets = @() }
  $book = $excel.Workbooks.Open($path, 0, $true)
  try {
    # Excel takes a printer only once a workbook is open.
    if (-not $script:before) { $script:before = [string]$excel.ActivePrinter }
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
        if (-not $CountsOnly) {
          foreach ($kind in @(64, 65)) {
            for ($at = 1; $at -le 5000; $at++) {
              $value = $excel.ExecuteExcel4Macro("INDEX(GET.DOCUMENT($kind),$at)")
              if ($value -isnot [double]) { break }
              if ($kind -eq 64) { $entry.rows += [int]$value } else { $entry.cols += [int]$value }
            }
          }
        }
        $entry.pages = [int]$sheet.PageSetup.Pages.Count
        $zoom = $sheet.PageSetup.Zoom
        $entry.zoom = if ($zoom -is [bool]) { 0 } else { [int]$zoom }
        $excel.ActiveWindow.View = 1
      } catch { $entry.error = $_.Exception.Message }
      $result.sheets += $entry
    }
  } finally {
    $book.Close($false)
  }
  return $result
}

$script:before = $null
$excel = Start-Excel
try {
  if ($List) {
    foreach ($path in (Get-Content -Path $List -Encoding utf8 | Where-Object { $_ })) {
      try {
        $line = @{ book = $path; result = (Get-Book $path) }
      } catch {
        $line = @{ book = $path; error = $_.Exception.Message }
        # A workbook that brings Excel down takes the session with it, so the next starts anew.
        try { [void]$excel.Workbooks.Count } catch {
          try { [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null } catch {}
          $excel = Start-Excel
        }
      }
      Add-Content -Path $Out -Value ($line | ConvertTo-Json -Depth 6 -Compress) -Encoding utf8
    }
  } else {
    $result = Get-Book $Workbook
  }
} finally {
  if ($script:before) { try { $excel.ActivePrinter = $script:before } catch {} }
  try { $excel.Quit() } catch {}
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
  [CgsMessageFilter]::Revoke()
}
if ($List) { return }
$result | ConvertTo-Json -Depth 5 | Set-Content -Path $Out -Encoding utf8
