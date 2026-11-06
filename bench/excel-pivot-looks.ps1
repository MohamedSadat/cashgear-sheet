# Reads what Excel shows on every cell of every pivot table in a workbook: its fill, font colour
# and weight, and its edges, from DisplayFormat, which includes the pivot style. Generated workbooks
# only. The file opens read-only with macros and links off, and is never saved.
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
$excel = New-Object -ComObject Excel.Application
$result = @{ error = $null; pivots = @() }

function Rgb([double]$bgr) {
  $value = [int64]$bgr
  return ('{0:X2}{1:X2}{2:X2}' -f ($value -band 0xFF), (($value -shr 8) -band 0xFF), (($value -shr 16) -band 0xFF))
}

# A number COM gives, or the default when it has none (DBNull for a mixed range).
function Num($value, $default) {
  if ($null -eq $value -or $value -is [DBNull]) { return $default }
  return $value
}

function Edge($borders, [int]$index) {
  $border = $borders.Item($index)
  $style = Num $border.LineStyle -4142
  if ([int]$style -eq -4142) { return $null }
  return @{ style = [int]$style; weight = [int](Num $border.Weight 2); color = (Rgb ([double](Num $border.Color 0))) }
}

try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  $book = $excel.Workbooks.Open($Workbook, 0, $true)
  foreach ($sheet in $book.Worksheets) {
    foreach ($table in $sheet.PivotTables()) {
      $range = $table.TableRange2
      $entry = @{ sheet = [string]$sheet.Name; name = [string]$table.Name; style = [string]$table.TableStyle2.Name; cells = @() }
      foreach ($cell in $range.Cells) {
        $shown = $cell.DisplayFormat
        $fill = $null
        if ([int](Num $shown.Interior.Pattern -4142) -ne -4142) { $fill = Rgb ([double](Num $shown.Interior.Color 16777215)) }
        $entry.cells += @{
          row = [int]$cell.Row
          col = [int]$cell.Column
          fill = $fill
          color = (Rgb ([double](Num $shown.Font.Color 0)))
          bold = [bool](Num $shown.Font.Bold $false)
          top = (Edge $shown.Borders 8)
          bottom = (Edge $shown.Borders 9)
          left = (Edge $shown.Borders 7)
          right = (Edge $shown.Borders 10)
        }
      }
      $result.pivots += $entry
    }
  }
  $book.Close($false)
} catch {
  $result.error = $_.Exception.Message
} finally {
  $excel.Quit()
  [System.Runtime.InteropServices.Marshal]::ReleaseComObject($excel) | Out-Null
}
$result | ConvertTo-Json -Depth 8 -Compress | Set-Content -LiteralPath $Out -Encoding UTF8
