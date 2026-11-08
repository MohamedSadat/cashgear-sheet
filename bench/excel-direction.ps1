# Reads each worksheet's direction as Excel understands it (Sheet Right-to-Left) and where its
# drawings sit, says whether the file needed a repair, and saves Excel's own copy for the app to
# read back. Generated workbooks only: the file opens read-only with macros forced off and links not
# updated, and only the copy is written.
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
$result = @{ opened = $false; repaired = $false; error = $null; sheets = @() }
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
    $sheets = @()
    foreach ($sheet in $book.Worksheets) {
      $shapes = @()
      foreach ($shape in $sheet.Shapes) {
        # Comments' boxes are shapes too; only drawings count.
        if ([int]$shape.Type -eq 4) { continue }
        $shapes += [string]$shape.TopLeftCell.Address($false, $false)
      }
      $sheets += @{ name = [string]$sheet.Name; rightToLeft = [bool]$sheet.DisplayRightToLeft; drawings = $shapes }
    }
    $result.sheets = $sheets
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
