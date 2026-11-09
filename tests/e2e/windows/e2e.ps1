<#
.SYNOPSIS
  Windows end-to-end gate for GSpot. Installs the NSIS installer, starts GSpot with
  its debug probe, and drives it against real Windows text fields: Notepad (Win32
  edit control), WordPad (rich edit) when the OS still has it, and a text area in
  Microsoft Edge. Takes a desktop screenshot at each step.

  It types, moves the pointer and clicks, so run it only on a disposable machine
  (a GitHub Actions Windows runner), never on a computer someone is using.
  Written for Windows PowerShell 5.1 (UI Automation from .NET Framework).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tests\e2e\windows\e2e.ps1 -Installer GSpot_0.2.0_x64-setup.exe -Out e2e-out
#>
param(
  [Parameter(Mandatory = $true)] [string] $Installer,
  [string] $Out = "e2e-out",
  [int] $Port = 47821,
  # Also run the built app's publik self-test (provisions a real publik install).
  [switch] $SelfTest
)

$ErrorActionPreference = "Stop"
$Out = (New-Item -ItemType Directory -Force $Out).FullName
$Installer = (Resolve-Path $Installer).Path
$script:results = New-Object System.Collections.ArrayList
$script:hardFails = 0

Write-Host "e2e: start $(Get-Date -Format o)"
Add-Type -AssemblyName System.Drawing, System.Windows.Forms, UIAutomationClient, UIAutomationTypes
Write-Host "e2e: assemblies loaded"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, System.Text.StringBuilder l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, int m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
  public static string ClassOf(IntPtr h) { var sb = new System.Text.StringBuilder(128); GetClassName(h, sb, 128); return sb.ToString(); }
  // WM_GETTEXT of a window (the system copies the text across processes).
  public static string GetText(IntPtr h) {
    int n = (int)SendMessage(h, 0x000E, IntPtr.Zero, IntPtr.Zero);
    var sb = new System.Text.StringBuilder(n + 1);
    SendMessage(h, 0x000D, (IntPtr)(n + 1), sb);
    return sb.ToString();
  }
  static int[] Pos(IntPtr h, int i) {
    long r = (long)SendMessage(h, 0x00D6, (IntPtr)i, IntPtr.Zero);
    if (r == -1) return null;
    var p = new POINT { X = (short)(r & 0xFFFF), Y = (short)((r >> 16) & 0xFFFF) };
    ClientToScreen(h, ref p);
    return new int[] { p.X, p.Y };
  }
  // Screen rect {x, y, w, h} of characters i..i+n of a classic edit control, by EM_POSFROMCHAR.
  public static int[] EditRect(IntPtr h, int i, int n) {
    var a = Pos(h, i); var b = Pos(h, i + n);
    if (a == null) return null;
    int lh = 16;
    int l1 = (int)SendMessage(h, 0x00BB, (IntPtr)1, IntPtr.Zero);
    if (l1 > 0) { var p0 = Pos(h, 0); var p1 = Pos(h, l1); if (p0 != null && p1 != null && p1[1] > p0[1]) lh = p1[1] - p0[1]; }
    int w = (b != null && b[1] == a[1]) ? b[0] - a[0] : 8 * n;
    return new int[] { a[0], a[1], w, lh };
  }
  // The first descendant window whose class is a Win32 edit or rich-edit control.
  public static IntPtr FindEditor(IntPtr top) {
    IntPtr found = IntPtr.Zero;
    EnumChildWindows(top, (h, l) => {
      var sb = new System.Text.StringBuilder(128); GetClassName(h, sb, 128);
      var c = sb.ToString();
      if (c == "Edit" || c.StartsWith("RichEdit", StringComparison.OrdinalIgnoreCase)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static uint FgPid() { uint p; GetWindowThreadProcessId(GetForegroundWindow(), out p); return p; }
  public static void Click(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(120);
    mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(40);
    mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
  }
  // Windows lets only the foreground process move the foreground; an Alt tap unlocks it.
  public static bool Focus(IntPtr h) {
    keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x12, 0, 2, UIntPtr.Zero);
    ShowWindow(h, 9);
    return SetForegroundWindow(h);
  }
}
"@
[void][W]::SetProcessDPIAware()

$UIA = [System.Windows.Automation.AutomationElement]
$CT = [System.Windows.Automation.ControlType]

function Log([string]$m) { Write-Host ("[{0:HH:mm:ss}] {1}" -f (Get-Date), $m) }
function Record([string]$status, [string]$name, [string]$detail, [bool]$hard) {
  [void]$script:results.Add([pscustomobject]@{ status = $status; check = $name; detail = $detail; hard = $hard })
  Write-Host ("{0,-5} {1} - {2}" -f $status, $name, $detail)
  if ($status -eq "FAIL" -and $hard) { $script:hardFails++ }
}
function Pass([string]$name, [string]$detail = "") { Record "PASS" $name $detail $true }
function Fail([string]$name, [string]$detail = "") { Record "FAIL" $name $detail $true }
function SoftFail([string]$name, [string]$detail = "") { Record "FAIL" $name $detail $false }
function Note([string]$name, [string]$detail = "") { Record "NOTE" $name $detail $false }

function Shot([string]$name) {
  try {
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
    $p = Join-Path $Out "$name.png"
    $bmp.Save($p, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Log "screenshot $name.png"
  } catch { Log "screenshot $name failed: $_" }
}

function Probe([string]$Path, $Body = @{}) {
  $json = ConvertTo-Json -InputObject $Body -Compress -Depth 10
  try {
    return Invoke-RestMethod -Uri "http://127.0.0.1:$Port$Path" -Method Post -Body $json -ContentType "application/json" -TimeoutSec 90
  } catch {
    $msg = $_.ErrorDetails.Message
    if (-not $msg) { $msg = "$_" }
    return [pscustomobject]@{ error = $msg }
  }
}

function Wait-Until([scriptblock]$Cond, [int]$Seconds = 30) {
  $deadline = (Get-Date).AddSeconds($Seconds)
  while ((Get-Date) -lt $deadline) {
    try { $v = & $Cond; if ($v) { return $v } } catch { }
    Start-Sleep -Milliseconds 400
  }
  return $null
}

function State { return Probe "/state" }

# The editor of a top-level window: its Win32 edit / rich-edit child (with its UI
# Automation element), else the first Edit or Document element UI Automation finds.
# A classic "Edit" child is read and measured with its own window messages: the
# managed UI Automation client in Windows PowerShell sees it as a bare pane.
function Find-Editor([IntPtr]$hwnd) {
  $child = [W]::FindEditor($hwnd)
  if ($child -ne [IntPtr]::Zero) {
    return [pscustomobject]@{ Hwnd = $child; Class = [W]::ClassOf($child); El = $UIA::FromHandle($child) }
  }
  $root = $UIA::FromHandle($hwnd)
  $cond = New-Object System.Windows.Automation.OrCondition(
    (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Edit)),
    (New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $CT::Document)))
  $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
  if (-not $el) { return $null }
  return [pscustomobject]@{ Hwnd = [IntPtr]::Zero; Class = $el.Current.ClassName; El = $el }
}

function Get-EditorText($ed) {
  if ($ed.Class -eq "Edit") { return [W]::GetText($ed.Hwnd) }
  $el = $ed.El
  $tp = $null
  if ($el.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref]$tp)) { return $tp.DocumentRange.GetText(-1) }
  $vp = $null
  if ($el.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$vp)) { return $vp.Current.Value }
  return $null
}

# Screen rectangle of the first occurrence of $word, measured by the editor's own text provider.
function Find-WordRect($ed, [string]$word) {
  if ($ed.Class -eq "Edit") {
    $i = ([W]::GetText($ed.Hwnd)).IndexOf($word)
    if ($i -lt 0) { return $null }
    $r = [W]::EditRect($ed.Hwnd, $i, $word.Length)
    if (-not $r) { return $null }
    return [pscustomobject]@{ X = $r[0]; Y = $r[1]; Width = $r[2]; Height = $r[3] }
  }
  $el = $ed.El
  $tp = $null
  if (-not $el.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref]$tp)) { return $null }
  $r = $tp.DocumentRange.FindText($word, $false, $false)
  if ($null -eq $r) { return $null }
  $rects = $r.GetBoundingRectangles()
  if ($rects.Length -lt 1) { return $null }
  return $rects[0]
}

function Find-Window([string]$processName, [int]$Seconds = 20) {
  return Wait-Until {
    $p = Get-Process -Name $processName -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } | Select-Object -First 1
    if ($p) { return $p }
  } $Seconds
}

function Alert-By([string]$original) {
  $s = State
  return @($s.alerts | Where-Object { $_.original -eq $original -and @($_.suggestions).Count -gt 0 }) | Select-Object -First 1
}

# Share of pixels along a horizontal line of the screen that are clearly colored
# (an underline), not the white page or black text.
function Colored-Share([int]$x, [int]$y, [int]$w) {
  $bmp = New-Object System.Drawing.Bitmap $w, 1
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
  $n = 0
  for ($i = 0; $i -lt $w; $i++) { $c = $bmp.GetPixel($i, 0); if ($c.GetSaturation() -gt 0.35 -and $c.GetBrightness() -gt 0.2 -and $c.GetBrightness() -lt 0.9) { $n++ } }
  $g.Dispose(); $bmp.Dispose()
  return [math]::Round($n / [math]::Max(1, $w), 2)
}

# The best "Accept"/"Dismiss" button in GSpot's own windows, nearest a screen point.
# (Web content in WebView2 belongs to msedgewebview2.exe, so search under GSpot's
# top-level windows rather than by process id.)
function Find-CardButton([string]$name, [double]$nearX, [double]$nearY) {
  $wins = $UIA::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, (New-Object System.Windows.Automation.PropertyCondition($UIA::ProcessIdProperty, $app.Id)))
  $named = New-Object System.Windows.Automation.PropertyCondition($UIA::NameProperty, $name)
  $all = foreach ($w in $wins) { $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, $named) }
  return @($all | Where-Object { $_ } | Sort-Object { $r = $_.Current.BoundingRectangle; [math]::Abs($r.Left - $nearX) + [math]::Abs($r.Top - $nearY) }) | Select-Object -First 1
}

# Hover an alert's underline with the real pointer; returns the state once the card is open.
function Hover-Alert($alert) {
  $s = State
  $ur = @($s.geometry.rects | Where-Object { $_.id -eq $alert.id }) | Select-Object -First 1
  if (-not $ur) { return $null }
  [void][W]::SetCursorPos([int]($s.geometry.origin_x + $ur.x + $ur.w / 2), [int]($s.geometry.origin_y + $ur.y + $ur.h / 2))
  return Wait-Until { $t = State; if ($t.geometry.hovered -eq $alert.id -and @($t.card_rects).Count -gt 0) { return $t } } 8
}

function Click-Element($el) {
  $bb = $el.Current.BoundingRectangle
  [W]::Click([int]($bb.Left + $bb.Width / 2), [int]($bb.Top + $bb.Height / 2))
}

function Focus-Editor($proc, $editor) {
  [void][W]::Focus($proc.MainWindowHandle)
  Start-Sleep -Milliseconds 300
  try { $editor.El.SetFocus() } catch { }
  $b = $editor.El.Current.BoundingRectangle
  [W]::Click([int]($b.Left + 30), [int]($b.Bottom - 20))
  Start-Sleep -Milliseconds 300
}

# --- 0. Screen and recording ---------------------------------------------------------
Write-Host "e2e: helpers compiled"
# Set the screen to 1920x1080 when it is smaller, in a job with a time limit (the
# setting tool can stall on some images).
$cur = [System.Windows.Forms.SystemInformation]::VirtualScreen
if ($cur.Width -lt 1920 -or $cur.Height -lt 1080) {
  $job = Start-Job { Set-DisplayResolution -Width 1920 -Height 1080 -Force -ErrorAction Stop }
  if (Wait-Job $job -Timeout 20) { try { Receive-Job $job -ErrorAction Stop | Out-Null; Log "display set to 1920x1080" } catch { Log "display resolution unchanged: $($_.Exception.Message)" } }
  else { Stop-Job $job; Log "display resolution unchanged: the setting did not finish in 20 s" }
  Remove-Job $job -Force -ErrorAction SilentlyContinue
}
$vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
Note "screen" "$($vs.Width)x$($vs.Height), $([System.Environment]::OSVersion.VersionString)"
$ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
$rec = $null
if ($ffmpeg) {
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $ffmpeg.Source
  $psi.Arguments = "-hide_banner -loglevel error -y -f gdigrab -framerate 8 -i desktop -vf scale=trunc(iw/2)*2:trunc(ih/2)*2 -c:v libx264 -preset ultrafast -pix_fmt yuv420p `"$Out\session.mp4`""
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $rec = [System.Diagnostics.Process]::Start($psi)
  Log "recording the desktop to session.mp4"
}

$app = $null
try {
  # --- 1. Install --------------------------------------------------------------------
  Log "installing $Installer"
  $inst = Start-Process -FilePath $Installer -ArgumentList "/S" -PassThru -Wait
  $dir = Join-Path $env:LOCALAPPDATA "GSpot"
  $reg = Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like "GSpot*" } | Select-Object -First 1
  if ($reg -and $reg.InstallLocation) { $dir = $reg.InstallLocation.Trim('"') }
  if ($reg) { Note "uninstall entry" "$($reg.DisplayName) $($reg.DisplayVersion) at $dir" }
  $exe = Get-ChildItem -Path $dir -Filter *.exe -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike "uninstall*" } | Select-Object -First 1
  if ($inst.ExitCode -eq 0 -and $exe) { Pass "silent install" "$($exe.FullName) ($([math]::Round($exe.Length / 1MB, 1)) MB)" } else { Fail "silent install" "exit $($inst.ExitCode), files: $((Get-ChildItem $dir -ErrorAction SilentlyContinue | ForEach-Object Name) -join ', ')"; throw "not installed" }
  $lnk = Get-ChildItem "$env:APPDATA\Microsoft\Windows\Start Menu\Programs" -Recurse -Filter "GSpot*.lnk" -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($lnk) { Pass "Start menu shortcut" $lnk.Name } else { SoftFail "Start menu shortcut" "none found" }

  # --- 2. Self-test (optional: provisions a real publik install) -----------------------
  if ($SelfTest) {
    $env:PUBLIK_SELF_TEST = "1"
    $st = Start-Process -FilePath $exe.FullName -PassThru -Wait -NoNewWindow -RedirectStandardOutput "$Out\self-test.txt"
    Remove-Item Env:\PUBLIK_SELF_TEST
    $line = (Get-Content "$Out\self-test.txt" -Raw)
    $cred = Join-Path $env:LOCALAPPDATA "publik\apps\gspot.json"
    if ($line -match "publik self-test: ok gspot" -and (Test-Path $cred)) { Pass "publik self-test" "$($line.Trim()); credential at $cred" } else { Fail "publik self-test" "output: $line; credential file present: $(Test-Path $cred)" }
  }

  # --- 3. Start GSpot with the debug probe ----------------------------------------------
  $env:GRAMMARLAMA_DEBUG_PORT = "$Port"
  $env:DO_NOT_TRACK = "1"
  $env:RUST_LOG = "info"
  $app = Start-Process -FilePath $exe.FullName -PassThru -RedirectStandardError "$Out\gspot.log" -RedirectStandardOutput "$Out\gspot-out.log"
  $h = Wait-Until { $r = Probe "/health"; if ($r.ok) { return $r } } 60
  if ($h) { Pass "GSpot starts and its probe answers" "pid $($app.Id), trusted=$($h.trusted)" } else { Fail "GSpot starts" "no probe answer in 60 s"; throw "app did not start" }
  $ready = Wait-Until { $s = State; if (@($s.ui_ready).Count -ge 4) { return $s } } 40
  $s = State
  if ($ready) { Pass "all four WebView2 windows mounted" (@($s.ui_ready) -join ", ") } else { Fail "WebView2 windows mounted" "ui_ready=$(@($s.ui_ready) -join ', ') ui_log=$(@($s.ui_log) -join ' | ')" }
  if ($s.panel.tucked -and $s.panel.tab_visible) { Pass "starts tucked behind the edge tab" "tab visible, panel hidden" } else { Fail "starts tucked" ($s.panel | ConvertTo-Json -Compress) }
  Shot "01-started-tucked"

  # --- 4. Notepad --------------------------------------------------------------------
  $fixture = Join-Path $Out "fixture.txt"
  $text = "I has teh cat and I think that we should just ship it.`r`nIn order to be able to make progress at this point in time, we will recieve the files tomorow.`r`nTeh end."
  [System.IO.File]::WriteAllText($fixture, $text)
  $np = Start-Process notepad.exe -ArgumentList "`"$fixture`"" -PassThru
  $npw = Find-Window "notepad" 20
  if (-not $npw) { Fail "Notepad opens" "no window"; throw "no notepad" }
  [void][W]::MoveWindow($npw.MainWindowHandle, 480, 60, 900, 520, $true)
  Start-Sleep -Milliseconds 600
  $editor = Find-Editor $npw.MainWindowHandle
  if (-not $editor) { Fail "Notepad editor found" "no Edit/Document element"; throw "no editor" }
  Note "Notepad editor" "class $($editor.Class), UI Automation type $($editor.El.Current.ControlType.ProgrammaticName)"
  Focus-Editor $npw $editor

  $doc = Wait-Until { $s = State; if ($s.doc -and $s.doc.bundle_id -eq "notepad.exe" -and @($s.alerts).Count -ge 3) { return $s } } 30
  $s = State
  if ($doc) { Pass "Notepad text connected and linted" "app '$($s.doc.app_name)' ($($s.doc.bundle_id)), role $($s.doc.role), $(@($s.alerts).Count) alerts: $((@($s.alerts) | ForEach-Object { $_.original }) -join ', ')" } else { Fail "Notepad text connected" "doc=$($s.doc | ConvertTo-Json -Compress -Depth 3) status=$($s.status | ConvertTo-Json -Compress)"; Shot "fail-notepad"; throw "no doc" }
  $norm = { param($t) ($t -replace "`r`n", "`n") -replace "`r", "`n" }
  $breaks = if ($s.doc.text -like "*`r`n*") { "CR LF" } elseif ($s.doc.text -like "*`r*") { "CR" } else { "LF" }
  if ((& $norm $s.doc.text) -eq (& $norm $text)) { Pass "text read exactly" "$($s.doc.text.Length) chars, line breaks $breaks" } else { Fail "text read exactly" "got '$($s.doc.text)'" }

  $geo = Wait-Until { $s = State; if (@($s.geometry.rects).Count -ge 3) { return $s } } 15
  $s = State
  if ($geo) { Pass "underline geometry" "$(@($s.geometry.rects).Count) rects; field at ($($s.geometry.origin_x), $($s.geometry.origin_y)) $($s.geometry.width)x$($s.geometry.height)" } else { Fail "underline geometry" "rects=$(@($s.geometry.rects).Count)" }
  $s | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $Out "state-notepad.json") -Encoding UTF8
  $ul = $s.panel.underlines
  if ($ul.visible -and [math]::Abs($ul.x - $s.geometry.origin_x) -le 2 -and [math]::Abs($ul.y - $s.geometry.origin_y) -le 2) { Pass "underline layer shown over the field" "at ($($ul.x), $($ul.y)) $($ul.w)x$($ul.h)" } else { Fail "underline layer shown over the field" ($ul | ConvertTo-Json -Compress) }
  Start-Sleep -Milliseconds 500
  $drawn = 0
  foreach ($r in @($s.geometry.rects | Where-Object { -not $_.sentence })) {
    $share = Colored-Share ([int]($s.geometry.origin_x + $r.x)) ([int]($s.geometry.origin_y + $r.y + $r.h - 1)) ([int][math]::Max(4, $r.w))
    if ($share -ge 0.5) { $drawn++ }
  }
  $words = @($s.geometry.rects | Where-Object { -not $_.sentence }).Count
  if ($drawn -ge 1 -and $drawn -ge $words - 1) { Pass "underlines drawn on screen" "$drawn of $words word underlines found in screen pixels" } else { Fail "underlines drawn on screen" "$drawn of $words word underlines found in screen pixels" }

  # GSpot's rect for "tomorow" (third line, after two CR LF) against Notepad's own measurement.
  $a = Alert-By "tomorow"
  $want = Find-WordRect $editor "tomorow"
  $mine = @($s.geometry.rects | Where-Object { $_.id -eq $a.id }) | Select-Object -First 1
  if ($a -and $want -and $mine) {
    $dx = [math]::Abs(($s.geometry.origin_x + $mine.x) - $want.X); $dy = [math]::Abs(($s.geometry.origin_y + $mine.y) - $want.Y)
    $detail = "GSpot ($($s.geometry.origin_x + $mine.x), $($s.geometry.origin_y + $mine.y)) vs Notepad ($($want.X), $($want.Y)); off by $dx, $dy px"
    if ($dx -le 3 -and $dy -le 3) { Pass "underline sits on the word after line breaks" $detail } else { Fail "underline position" $detail }
  } else { Fail "underline position" "alert=$([bool]$a) notepad rect=$([bool]$want) gspot rect=$([bool]$mine)" }
  if ([W]::FgPid() -eq $npw.Id) { Pass "Notepad keeps the focus while the overlay shows" "" } else { Fail "Notepad keeps the focus" "foreground pid $([W]::FgPid())" }
  Shot "02-notepad-underlines"

  # Accept through the probe (same code path as the panel's Accept).
  $a = Alert-By "tomorow"
  $r = Probe "/accept" @{ id = $a.id }
  Start-Sleep -Milliseconds 400
  $now = Get-EditorText $editor
  if ($r.method -and $now -like "*files tomorrow.*") { Pass "accept 'tomorow' -> 'tomorrow' changes Notepad" "method $($r.method)" } else { Fail "accept in Notepad" "result $($r | ConvertTo-Json -Compress) text '$now'" }

  # Hover the "recieve" underline with the real pointer, then click Accept on the card.
  [void](Wait-Until { $x = Alert-By "recieve"; $t = State; if ($x -and @($t.geometry.rects | Where-Object { $_.id -eq $x.id }).Count -gt 0) { return $t } } 10)
  $a = Alert-By "recieve"
  $hov = if ($a) { Hover-Alert $a } else { $null }
  if ($hov) {
    Pass "hovering the underline opens the suggestion card" "card at $(($hov.card_rects | Select-Object -First 1) | ConvertTo-Json -Compress)"
    Start-Sleep -Milliseconds 400
    Shot "03-hover-card"
    $btn = Wait-Until { Find-CardButton "Accept" ($hov.geometry.origin_x) ($hov.geometry.origin_y) } 10
    if ($btn) {
      Click-Element $btn
      $ok = Wait-Until { $t = Get-EditorText $editor; if ($t -like "*will receive the files*") { return $t } } 8
      if ($ok) { Pass "clicking Accept on the card changes Notepad" "'recieve' -> 'receive'" } else { Fail "card Accept click" "text '$(Get-EditorText $editor)'" }
      if ([W]::FgPid() -eq $npw.Id) { Pass "Notepad keeps the focus after the card click" "" } else { Fail "focus after card click" "foreground pid $([W]::FgPid())" }
    } else { Fail "card Accept button" "not found through UI Automation" }
  } else { $t = State; Fail "hover card" "alert=$([bool]$a) hovered=$($t.geometry.hovered) card_rects=$(@($t.card_rects).Count)" }
  [void][W]::SetCursorPos(1000, 900)
  Start-Sleep -Milliseconds 800

  # Dismiss "Teh" from its card: the alert goes away, the text stays, the choice is saved.
  $a = Wait-Until { $x = Alert-By "Teh"; $t = State; if ($x -and @($t.geometry.rects | Where-Object { $_.id -eq $x.id }).Count -gt 0) { return $x } } 10
  $hov = if ($a) { Hover-Alert $a } else { $null }
  if ($hov) {
    $btn = Wait-Until { Find-CardButton "Dismiss" ($hov.geometry.origin_x) ($hov.geometry.origin_y) } 10
    if ($btn) {
      Click-Element $btn
      $gone = Wait-Until { $t = State; if (-not @($t.alerts | Where-Object { $_.original -eq "Teh" })) { return $t } } 8
      $set = Probe "/settings"
      $txt = Get-EditorText $editor
      if ($gone -and $txt -like "*Teh end.*" -and (@($set.dismissed) | ForEach-Object { "$_" }) -contains "$($a.stable_hash)") { Pass "Dismiss on the card hides the alert and keeps the text" "'Teh' left as written; saved in settings" } else { Fail "card Dismiss" "gone=$([bool]$gone) text '$txt' dismissed=$(@($set.dismissed) -join ',')" }
    } else { Fail "card Dismiss button" "not found through UI Automation" }
  } else { Fail "hover for dismiss" "alert=$([bool]$a)" }
  [void][W]::SetCursorPos(1000, 900)

  # Spot check with no model: the deterministic wordy-phrase rewrite on line 2.
  $r = Probe "/feature" @{ name = "spot_check"; args = @{} }
  $spot = Wait-Until { $s = State; $x = @($s.alerts | Where-Object { $_.kind -like "Spot*" }) | Select-Object -First 1; if ($x) { return $x } } 15
  if ($spot) {
    Pass "spot check suggests a rewrite" "'$($spot.original)' -> '$(@($spot.suggestions)[0])'"
    $r = Probe "/accept" @{ id = $spot.id }
    Start-Sleep -Milliseconds 400
    $now = Get-EditorText $editor
    $want = @($spot.suggestions)[0]
    if ($r.method -and $now -and $now -notlike "*In order to be able to*" -and $now.Contains($want)) { Pass "accepting the rewrite changes Notepad" "line 2 now: '$(($now -split "`r`n")[1])'" } else { Fail "accept spot rewrite" "result $($r | ConvertTo-Json -Compress) text '$now'" }
  } else { Fail "spot check" "result $($r | ConvertTo-Json -Compress)" }

  # Panel: first run shows the AI picker; after that the Review list.
  [void](Probe "/panel" @{ tucked = $false; view = "review" })
  Start-Sleep -Milliseconds 1500
  $s = State
  if ($s.panel.visible -and -not $s.panel.tucked -and $s.panel.x -ge 0 -and $s.panel.x -le 40) { Pass "panel slides out, docked on the left" "at ($($s.panel.x), $($s.panel.y))" } else { Fail "panel slides out, docked on the left" ($s.panel | ConvertTo-Json -Compress) }
  if ([W]::FgPid() -eq $npw.Id) { Pass "Notepad keeps the focus while the panel is out" "" } else { Note "focus while the panel is out" "foreground pid $([W]::FgPid())" }
  Shot "04-panel-first-run"
  [void](Probe "/settings" @{ onboarded = $true })
  [void](Probe "/panel" @{ view = "review" })
  Start-Sleep -Milliseconds 1200
  Shot "05-panel-review"
  [void](Probe "/panel" @{ tucked = $true })
  $t = Wait-Until { $s = State; if ($s.panel.tucked -and $s.panel.tab_visible -and -not $s.panel.visible) { return $s } } 5
  if ($t) { Pass "panel tucks back behind the edge tab" "" } else { Fail "panel tucks" ((State).panel | ConvertTo-Json -Compress) }
  Stop-Process -Id $npw.Id -Force -ErrorAction SilentlyContinue

  # --- 5. WordPad (rich edit), if this Windows has it ----------------------------------
  $wordpad = "$env:ProgramFiles\Windows NT\Accessories\wordpad.exe"
  if (Test-Path $wordpad) {
    $wp = Start-Process $wordpad -ArgumentList "`"$fixture`"" -PassThru
    $wpw = Find-Window "wordpad" 20
    if ($wpw) {
      [void][W]::MoveWindow($wpw.MainWindowHandle, 480, 60, 900, 520, $true)
      Start-Sleep -Milliseconds 800
      $ed = Find-Editor $wpw.MainWindowHandle
      Focus-Editor $wpw $ed
      $doc = Wait-Until { $s = State; if ($s.doc -and $s.doc.bundle_id -eq "wordpad.exe" -and $s.doc.active -and @($s.alerts).Count -ge 2) { return $s } } 30
      if ($doc) {
        Pass "WordPad text connected and linted" "$(@($doc.alerts).Count) alerts, role $($doc.doc.role)"
        Shot "06-wordpad-underlines"
        $a = Alert-By "teh"
        $r = Probe "/accept" @{ id = $a.id }
        Start-Sleep -Milliseconds 400
        $now = Get-EditorText $ed
        if ($r.method -and $now -like "I has the cat*") { Pass "accept in WordPad" "method $($r.method)" } else { Fail "accept in WordPad" "result $($r | ConvertTo-Json -Compress) text '$now'" }
      } else { $s = State; Fail "WordPad text connected" "doc=$($s.doc.bundle_id) alerts=$(@($s.alerts).Count)" }
      Stop-Process -Id $wpw.Id -Force -ErrorAction SilentlyContinue
    } else { SoftFail "WordPad opens" "no window" }
  } else { Note "WordPad" "not on this Windows version" }

  # --- 6. Edge text area (web content through Chromium's UI Automation) ----------------
  $edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
  if ($edge) {
    $html = Join-Path $Out "fixture.html"
    [System.IO.File]::WriteAllText($html, "<!doctype html><title>GSpot test</title><body style='font:18px sans-serif;margin:24px'><textarea id=t autofocus style='width:90%;height:260px;font:18px sans-serif'>I will recieve teh files tomorow.</textarea></body>")
    $prof = Join-Path $env:TEMP "gspot-edge-profile"
    $eg = Start-Process $edge -ArgumentList "--user-data-dir=`"$prof`"", "--no-first-run", "--no-default-browser-check", "--force-renderer-accessibility", "--window-position=480,60", "--window-size=900,520", "--app=file:///$($html -replace '\\','/')" -PassThru
    $egw = Find-Window "msedge" 30
    if ($egw) {
      Start-Sleep -Seconds 3
      [void][W]::Focus($egw.MainWindowHandle)
      Start-Sleep -Milliseconds 500
      [W]::Click(700, 200)
      $doc = Wait-Until { $s = State; if ($s.doc -and $s.doc.bundle_id -eq "msedge.exe" -and $s.doc.active -and @($s.alerts).Count -ge 2) { return $s } } 30
      if ($doc) {
        Pass "Edge text area connected and linted" "$(@($doc.alerts).Count) alerts, role $($doc.doc.role)"
        $g = Wait-Until { $s = State; if (@($s.geometry.rects).Count -ge 2) { return $s } } 10
        if ($g) { Pass "Edge underline geometry" "$(@($g.geometry.rects).Count) rects" } else { Fail "Edge underline geometry" "no rects" }
        Shot "07-edge-underlines"
        $a = Alert-By "tomorow"
        $r = Probe "/accept" @{ id = $a.id }
        Start-Sleep -Milliseconds 600
        $s = State
        if ($r.method -and $s.doc.text -like "*files tomorrow.*") { Pass "accept in Edge" "method $($r.method)" } else { Fail "accept in Edge" "result $($r | ConvertTo-Json -Compress) text '$($s.doc.text)'" }
      } else { $s = State; Fail "Edge text area connected" "front=$($s.status.frontmost_bid) doc=$($s.doc.bundle_id) alerts=$(@($s.alerts).Count)"; Shot "fail-edge" }
      Get-Process msedge -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    } else { Fail "Edge opens" "no window" }
  } else { Fail "Edge" "not installed" }

  # --- 7. Terminals are skipped -----------------------------------------------------
  $cmd = Start-Process cmd.exe -ArgumentList "/k", "echo GSpot stays out of terminals" -PassThru
  Start-Sleep -Seconds 2
  $cw = Wait-Until { Get-Process -Id $cmd.Id -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } } 10
  if ($cw) { [void][W]::Focus($cw.MainWindowHandle) }
  $sk = Wait-Until { $s = State; if ($s.status.skipped) { return $s } } 10
  if ($sk) { Pass "terminal skipped" "front app $($sk.status.frontmost_bid), built in: $($sk.status.skip_built_in)" } else { $s = State; Fail "terminal skipped" "front=$($s.status.frontmost_bid) skipped=$($s.status.skipped)" }
  Shot "08-terminal-skipped"
  Stop-Process -Id $cmd.Id -Force -ErrorAction SilentlyContinue
  Get-Process WindowsTerminal -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

  # --- 8. Quit and uninstall ---------------------------------------------------------
  Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
  $app = $null
  Start-Sleep -Seconds 1
  $un = Join-Path $dir "uninstall.exe"
  if (Test-Path $un) {
    Start-Process $un -ArgumentList "/S" -Wait
    $gone = Wait-Until { -not (Test-Path $exe.FullName) } 30
    if ($gone) { Pass "silent uninstall removes the app" "" } else { Fail "uninstall" "$($exe.FullName) still there" }
  } else { Fail "uninstaller" "no uninstall.exe in $dir" }
} catch {
  Log "stopped early: $_"
  if ($script:hardFails -eq 0) { Fail "harness" "$_" }
  Shot "fail-final"
} finally {
  if ($app) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
  if ($rec -and -not $rec.HasExited) {
    try { $rec.StandardInput.Write("q"); $rec.StandardInput.Flush() } catch { }
    if (-not $rec.WaitForExit(15000)) { $rec.Kill() }
  }
}

# --- Summary --------------------------------------------------------------------------
$results | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $Out "results.json") -Encoding UTF8
$pass = @($results | Where-Object status -eq "PASS").Count
$fail = @($results | Where-Object status -eq "FAIL").Count
$md = @("## GSpot Windows e2e on $([System.Environment]::OSVersion.VersionString)", "", "$pass passed, $fail failed ($script:hardFails hard).", "", "| | Check | Detail |", "|---|---|---|")
foreach ($r in $results) { $md += "| $($r.status) | $($r.check) | $(($r.detail -replace '\|', '/') -replace "`r?`n", ' ') |" }
$md -join "`n" | Set-Content (Join-Path $Out "summary.md") -Encoding UTF8
if ($env:GITHUB_STEP_SUMMARY) { $md -join "`n" | Add-Content $env:GITHUB_STEP_SUMMARY -Encoding UTF8 }
Write-Host ""
Write-Host "$pass passed, $fail failed ($script:hardFails hard failures)"
if ($script:hardFails -gt 0) { exit 1 }
exit 0
