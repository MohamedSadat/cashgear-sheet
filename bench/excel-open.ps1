# Opens one workbook in Excel the way a person would, and says whether Excel had to repair it.
# Macros and links stay off, and the file opens read-only and is never saved. Generated and
# private workbooks only: public files never open in Excel.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][string]$Out
)
$ErrorActionPreference = 'Stop'
# Excel turns calls away while it is busy, such as refreshing pivot tables a file asks it to refresh
# when it opens. This filter has COM try a turned-away call again instead of failing.
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
  // Busy: try again in 200 ms, for up to two minutes.
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
  # Excel writes a log named error<digits>.xml or error<digits>_<digits>.xml when it repairs.
  Get-ChildItem -Path $logs -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^error\d+(_\d+)?\.xml$' } |
    ForEach-Object { $_.FullName }
}
$before = @(Get-RepairLogs)
$result = @{ opened = $false; repaired = $false; logged = $false; renamed = $false; error = $null; sheets = 0; cells = 0 }
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  # With alerts on, a repair would wait for an answer nobody is there to give. Off, Excel repairs
  # or refuses by itself, and the signs below show which it did.
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  try {
    $book = $excel.Workbooks.Open($Workbook, 0, $true)
    $result.opened = $true
    $expected = [System.IO.Path]::GetFileName($Workbook)
    # A repaired book opens under another name, in Excel's own language.
    $result.renamed = $book.Name -ne $expected
    $result.sheets = $book.Sheets.Count
    foreach ($sheet in $book.Worksheets) {
      $result.cells += [int]$excel.WorksheetFunction.CountA($sheet.UsedRange)
    }
    $book.Close($false)
  } catch {
    $result.error = $_.Exception.GetType().Name
  }
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
$new = @(Get-RepairLogs | Where-Object { $before -notcontains $_ })
$result.logged = $new.Count -gt 0
foreach ($path in $new) { Remove-Item -LiteralPath $path -ErrorAction SilentlyContinue }
$result.repaired = $result.logged -or $result.renamed
[System.IO.File]::WriteAllText($Out, (ConvertTo-Json -InputObject $result -Compress), [System.Text.UTF8Encoding]::new($false))
