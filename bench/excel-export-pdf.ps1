# Has Excel export a workbook as a PDF with ExportAsFixedFormat, with Microsoft Print to PDF as its
# active printer for its session only. Generated workbooks only. The workbook opens read-only with
# macros forced off and links not updated, and is never saved.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][string]$Pdf
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
$devices = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Devices'
$port = ($devices.'Microsoft Print to PDF' -split ',')[1]
if (-not $port) { throw 'Microsoft Print to PDF is not installed.' }
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
  $excel.ActivePrinter = "Microsoft Print to PDF on $port"

  # Type 0 is PDF; standard quality, with the document's properties, honouring print areas.
  $book.ExportAsFixedFormat(0, $Pdf, 0, $true, $false)
  $book.Close($false)
} finally {
  if ($before) { try { $excel.ActivePrinter = $before } catch {} }
  $excel.Quit()
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
  [CgsMessageFilter]::Revoke()
}
