# Opens a workbook the app saved, says whether Excel had to repair it, then has Excel refresh every
# pivot table and reads back what each one shows, for comparing with what the app wrote. Generated
# workbooks only. The file opens read-only with macros and links off, and is never saved.
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
  Get-ChildItem -Path $logs -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^error\d+(_\d+)?\.xml$' } |
    ForEach-Object { $_.FullName }
}
$before = @(Get-RepairLogs)
$result = @{ opened = $false; repaired = $false; error = $null; pivots = @() }
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  try {
    $book = $excel.Workbooks.Open($Workbook, 0, $true)
    $result.opened = $true
    $expected = [System.IO.Path]::GetFileName($Workbook)
    if ($book.Name -ne $expected) { $result.repaired = $true }
    $after = @(Get-RepairLogs)
    if ($after.Count -gt $before.Count) { $result.repaired = $true }
    foreach ($sheet in $book.Worksheets) {
      foreach ($table in $sheet.PivotTables()) {
        $entry = @{ sheet = [string]$sheet.Name; name = [string]$table.Name; error = $null; top = 0; left = 0; rows = @() }
        try {
          $table.RefreshTable() | Out-Null
          $range = $table.TableRange2
          $entry.top = [int]$range.Row
          $entry.left = [int]$range.Column
          $values = $range.Value2
          $rows = [int]$range.Rows.Count
          $cols = [int]$range.Columns.Count
          for ($r = 1; $r -le $rows; $r++) {
            $line = @()
            for ($c = 1; $c -le $cols; $c++) {
              $v = if ($rows -eq 1 -and $cols -eq 1) { $values } else { $values[$r, $c] }
              if ($null -eq $v) { $line += $null }
              elseif ($v -is [int]) { $line += @{ e = [int]$v } }
              else { $line += $v }
            }
            $entry.rows += , $line
          }
        } catch { $entry.error = $_.Exception.Message }
        $result.pivots += $entry
      }
    }
    $book.Close($false)
  } catch {
    $result.error = $_.Exception.Message
  }
} finally {
  $excel.Quit()
  [CgsMessageFilter]::Revoke()
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
}
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $Out -Encoding UTF8
