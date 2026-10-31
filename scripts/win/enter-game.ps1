# Relaunch the client and walk it from login to in-game with synthetic input.
# The client moves its own window between screens, so every screen element is
# addressed as a ratio of the current window rect.
param(
    [string]$Url = "nxl://launch/59813",
    [int]$StartTimeoutSec = 90,
    [switch]$NoWalk
)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class U {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool c);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
    [DllImport("user32.dll")] public static extern void keybd_event(byte v, byte s, uint f, IntPtr e);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    public struct RECT { public int L, T, R, B; }
}
'@

function Focus-Game([IntPtr]$h) {
    $fg = [U]::GetForegroundWindow()
    $tp = [uint32]0
    $t1 = [U]::GetWindowThreadProcessId($fg, [ref]$tp)
    $t2 = [U]::GetCurrentThreadId()
    [U]::AttachThreadInput($t2, $t1, $true) | Out-Null
    [U]::SetForegroundWindow($h) | Out-Null
    [U]::AttachThreadInput($t2, $t1, $false) | Out-Null
}

function Get-Rect([IntPtr]$h) {
    $r = New-Object U+RECT
    if (-not [U]::GetWindowRect($h, [ref]$r)) { throw "no rect" }
    $r
}

function Click-Rel([IntPtr]$h, [double]$fx, [double]$fy) {
    $r = Get-Rect $h
    $x = [int]($r.L + ($r.R - $r.L) * $fx)
    $y = [int]($r.T + ($r.B - $r.T) * $fy)
    Focus-Game $h
    Start-Sleep -Milliseconds 250
    [U]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 400
    [U]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 120
    [U]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero)
}

function Double-Click-Rel([IntPtr]$h, [double]$fx, [double]$fy) {
    $r = Get-Rect $h
    $x = [int]($r.L + ($r.R - $r.L) * $fx)
    $y = [int]($r.T + ($r.B - $r.T) * $fy)
    Focus-Game $h
    Start-Sleep -Milliseconds 250
    [U]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 400
    1..2 | ForEach-Object {
        [U]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [U]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 120
    }
}

# element positions as ratios of the client window
$LOGIN = @(0.753, 0.463)
$CH40 = @(0.673, 0.716)
$GO = @(0.638, 0.494)
$CHAR = @(0.339, 0.570)

Get-Process MapleStory -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process nexon_client, nexon_runtime -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 3
Start-Process $Url

$deadline = (Get-Date).AddSeconds($StartTimeoutSec)
do {
    Start-Sleep -Seconds 2
    $p = Get-Process MapleStory -ErrorAction SilentlyContinue | Select-Object -First 1
} until ($p -or (Get-Date) -gt $deadline)
if (-not $p) { throw "client did not start" }
Write-Host "pid $($p.Id)"

$h = [IntPtr]::Zero
for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep -Seconds 1
    $h = (Get-Process -Id $p.Id).MainWindowHandle
    if ($h -ne [IntPtr]::Zero) { break }
}
if ($h -eq [IntPtr]::Zero) { throw "no game window" }

if ($NoWalk) { Write-Host "relaunched pid $($p.Id)"; return }
[U]::ShowWindow($h, 5) | Out-Null
Start-Sleep -Seconds 8
Click-Rel $h $LOGIN[0] $LOGIN[1]
Start-Sleep -Seconds 8

Click-Rel $h $CH40[0] $CH40[1]
Click-Rel $h $CH40[0] $CH40[1]
Click-Rel $h $GO[0] $GO[1]
Start-Sleep -Seconds 5

Double-Click-Rel $h $CHAR[0] $CHAR[1]
Start-Sleep -Seconds 14

Write-Host "walked to character select / in-game, pid $($p.Id)"
