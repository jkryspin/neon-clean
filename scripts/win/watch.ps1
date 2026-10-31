# Live-run telemetry: process/module/window timeline + periodic screenshots.
# Usage: .\watch.ps1 -OutDir ..\..\builds\ngsx-runs\B1-20261030-1700 -Seconds 600
param(
    [Parameter(Mandatory)][string]$OutDir,
    [int]$Seconds = 600,
    [int]$IntervalSec = 5
)
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$shots = Join-Path $OutDir 'shots'
New-Item -ItemType Directory -Force -Path $shots | Out-Null
$log = Join-Path $OutDir 'watch.jsonl'
$summary = Join-Path $OutDir 'summary.json'

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class Win {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    public static List<string> Windows(uint pid) {
        var r = new List<string>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && IsWindowVisible(h)) {
                var c = new StringBuilder(128); GetClassName(h, c, 128);
                var t = new StringBuilder(256); GetWindowText(h, t, 256);
                r.Add(c + "|" + t);
            }
            return true;
        }, IntPtr.Zero);
        return r;
    }
}
"@

function Shot([string]$name) {
    try {
        $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
        $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
        $bmp.Save((Join-Path $shots $name), [System.Drawing.Imaging.ImageFormat]::Png)
        $g.Dispose(); $bmp.Dispose()
    } catch { }
}

$start = Get-Date
$seenModules = @{}
$seenWindows = @{}
$bestUptime = 0
$startedAt = $null
$lastShot = Get-Date
$i = 0

while (((Get-Date) - $start).TotalSeconds -lt $Seconds) {
    $i++
    $p = Get-Process MapleStory -ErrorAction SilentlyContinue | Select-Object -First 1
    $rec = [ordered]@{ t = (Get-Date).ToString('o'); alive = [bool]$p }
    if ($p) {
        if (-not $startedAt) { $startedAt = $p.StartTime }
        $rec.pid = $p.Id
        $rec.uptime_s = [int]((Get-Date) - $p.StartTime).TotalSeconds
        if ($rec.uptime_s -gt $bestUptime) { $bestUptime = $rec.uptime_s }
        $rec.threads = $p.Threads.Count
        $rec.ws_mb = [int]($p.WorkingSet64 / 1MB)
        $rec.cpu = [int]$p.TotalProcessorTime.TotalSeconds
        $rec.responding = $p.Responding
        try {
            $mods = @($p.Modules | ForEach-Object { $_.ModuleName })
            $rec.modules = $mods.Count
            $gra = @($mods | Where-Object { $_ -match '^(grap64|nmcogame64|nmconew64|BlackCat)' })
            $rec.guard = $gra
            foreach ($m in $gra) { $seenModules[$m] = $true }
        } catch { $rec.modules = 'denied'; $rec.guard = @() }
        try {
            $wins = [Win]::Windows([uint32]$p.Id)
            $rec.windows = $wins
            foreach ($w in $wins) { $seenWindows[$w] = ($seenWindows[$w] + 1) }
        } catch {}
    }
    ($rec | ConvertTo-Json -Compress) | Add-Content -LiteralPath $log
    if (((Get-Date) - $lastShot).TotalSeconds -ge 60) {
        Shot ("t{0:d4}.png" -f [int](((Get-Date) - $start).TotalSeconds))
        $lastShot = Get-Date
    }
    if (-not $p -and $startedAt) { break }
    Start-Sleep -Seconds $IntervalSec
}

$res = [ordered]@{
    started = $startedAt
    ended = Get-Date
    best_uptime_s = $bestUptime
    alive_at_end = [bool](Get-Process MapleStory -ErrorAction SilentlyContinue)
    guard_modules_seen = @($seenModules.Keys)
    windows_seen = @($seenWindows.Keys)
    samples = $i
}
$res | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $summary
$res | ConvertTo-Json -Depth 4
