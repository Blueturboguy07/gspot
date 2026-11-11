# Google-Docs-like page in Edge: text is painted on a <canvas>; the only editable element is an
# empty hidden contenteditable in an iframe (how Docs takes keystrokes). Click in, run GSpot's
# spot check. Bug PRESENT = GSpot says "The text field is empty." about a page that shows text.
$ErrorActionPreference = "Continue"
$Out = (New-Item -ItemType Directory -Force "lab-out").FullName
$Port = 47821
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public static class L {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte s, uint f, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public static void Click(int x, int y) { SetCursorPos(x, y); System.Threading.Thread.Sleep(150); mouse_event(2,0,0,0,UIntPtr.Zero); System.Threading.Thread.Sleep(40); mouse_event(4,0,0,0,UIntPtr.Zero); }
}
"@
[void][L]::SetProcessDPIAware()
function Probe($p, $b = @{}) {
  try { return Invoke-RestMethod -Uri "http://127.0.0.1:$Port$p" -Method Post -Body (ConvertTo-Json -InputObject $b -Compress) -ContentType "application/json" -TimeoutSec 60 }
  catch { $m = $_.ErrorDetails.Message; if (-not $m) { $m = "$_" }; return [pscustomobject]@{ error = $m } }
}
function Shot($n) { try { $b=[System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height; $g=[System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Left,$b.Top,0,0,$bmp.Size); $bmp.Save("$Out\$n.png"); } catch {} }
Add-Type -AssemblyName System.Drawing

$exe = Get-ChildItem src-tauri/target/release/*.exe | Where-Object { $_.Name -notlike "*build*" } | Select-Object -First 1
if (-not $exe) { Write-Host "ORACLE_CANNOT_RUN no exe"; exit 2 }
$env:GRAMMARLAMA_DEBUG_PORT = "$Port"
$app = Start-Process $exe.FullName -PassThru -RedirectStandardError "$Out\gspot.log" -RedirectStandardOutput "$Out\gspot-out.log"
$up = $false
for ($i=0; $i -lt 60 -and -not $up; $i++) { Start-Sleep 1; try { $h = Invoke-RestMethod "http://127.0.0.1:$Port/health" -Method Post -Body "{}" -ContentType application/json; $up = $true } catch {} }
if (-not $up) { Write-Host "ORACLE_CANNOT_RUN gspot probe never came up"; exit 2 }

$edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { Write-Host "ORACLE_CANNOT_RUN no edge"; exit 2 }
$inner = "<!doctype html><body contenteditable=true style='margin:0'></body>"
$page = @"
<!doctype html><title>Docs-like</title><body style='margin:0'>
<canvas id=c width=860 height=300 style='position:absolute;left:0;top:0'></canvas>
<iframe id=f style='position:absolute;left:10px;top:10px;width:700px;height:200px;border:0;opacity:0.01' srcdoc="$($inner.Replace('"','&quot;'))"></iframe>
<script>
var x=document.getElementById('c').getContext('2d'); x.font='20px sans-serif';
x.fillText('I will recieve teh files tomorow. My essay is about the history of Rome.',20,60);
x.fillStyle='#bcd'; x.fillRect(20,70,300,24);
x.fillStyle='#000'; x.fillText('This highlighted sentence is drawn, not typed.',24,88);
setTimeout(function(){document.getElementById('f').contentWindow.document.body.focus();},500);
</script></body>
"@
$html = Join-Path $Out "docs-like.html"
[System.IO.File]::WriteAllText($html, $page)
$prof = Join-Path $env:TEMP "lab-edge"
Start-Process $edge -ArgumentList "--user-data-dir=`"$prof`"","--no-first-run","--no-default-browser-check","--force-renderer-accessibility","--window-position=100,60","--window-size=900,520","--app=file:///$($html -replace '\\','/')" | Out-Null
Start-Sleep 8
$ew = $null
for ($i=0; $i -lt 30 -and -not $ew; $i++) { $ew = Get-Process msedge -EA SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1; Start-Sleep 1 }
if (-not $ew) { Write-Host "ORACLE_CANNOT_RUN edge window"; exit 2 }
[L]::keybd_event(0x12,0,0,[UIntPtr]::Zero); [L]::keybd_event(0x12,0,2,[UIntPtr]::Zero)
[void][L]::SetForegroundWindow($ew.MainWindowHandle)
Start-Sleep 1
[L]::Click(300, 140)          # inside the page's editable area
Start-Sleep 2
# Select all, as a user "highlighting text" would
[System.Windows.Forms.SendKeys]::SendWait("^a")
Start-Sleep 3
Shot "docs-like"
$s = Probe "/state"
Write-Host ("state doc: " + (ConvertTo-Json $s.doc -Compress -Depth 4))
$r = Probe "/feature" @{ name = "spot_check"; args = @{} }
$msg = ConvertTo-Json $r -Compress -Depth 4
Write-Host "spot_check -> $msg"
Get-Content "$Out\gspot.log" -EA SilentlyContinue | Select-String "focused field" | Select-Object -Last 5 | ForEach-Object { Write-Host $_.Line }
Get-Process msedge,gspot* -EA SilentlyContinue | Stop-Process -Force -EA SilentlyContinue
if ($msg -match "text field is empty") { Write-Host "BUGFIX_LAB_PRESENT"; exit 1 }
if ($msg -match "Click into a text field") { Write-Host "ORACLE_CANNOT_RUN gspot never connected to the fixture"; exit 2 }
Write-Host "BUGFIX_LAB_ABSENT"; exit 0
