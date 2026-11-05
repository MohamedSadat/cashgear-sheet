# Has Excel draw a range of a workbook as it shows on screen, and saves the picture as a PNG, for
# comparing with the app's own by eye. Generated workbooks only. The file opens read-only with macros
# and links off, and is never saved. Run with -STA, as the clipboard needs it.
param(
  [Parameter(Mandatory)][string]$Workbook,
  [Parameter(Mandatory)][int]$Sheet,
  [Parameter(Mandatory)][string]$Range,
  [Parameter(Mandatory)][string]$Png
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$excel = New-Object -ComObject Excel.Application
try {
  $excel.Visible = $false
  $excel.DisplayAlerts = $false
  $excel.AskToUpdateLinks = $false
  $excel.AutomationSecurity = 3
  $book = $excel.Workbooks.Open($Workbook, 0, $true)
  $target = $book.Worksheets.Item($Sheet + 1)
  $target.Activate()
  [System.Windows.Forms.Clipboard]::Clear()
  # As it shows on screen, as a bitmap: xlScreen and xlBitmap.
  $target.Range($Range).CopyPicture(1, 2) | Out-Null
  $image = $null
  for ($try = 0; $try -lt 20 -and -not $image; $try++) {
    Start-Sleep -Milliseconds 100
    $image = [System.Windows.Forms.Clipboard]::GetImage()
  }
  if (-not $image) { throw 'Excel put no picture on the clipboard.' }
  $image.Save($Png, [System.Drawing.Imaging.ImageFormat]::Png)
  $book.Close($false)
} finally {
  $excel.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($excel)
}
