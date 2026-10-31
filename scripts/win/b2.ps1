# B2 live run: patched client + mock anti-cheat, no NGS.
#
# The launcher verifies its manifest and repairs MapleStory.exe back to the clean
# build before PLAY. This script cleans up, starts the launcher, injects
# hook-session32.dll into every nexon_* process (present and new), and triggers
# PLAY. The hook puts the mocks back and applies the five client patches inside
# CreateProcessW, after the repair and before the image is mapped.
#
#   .\b2.ps1 -Minutes 10                 # login soak, run dir under builds\ngsx-runs
#   .\b2.ps1 -Minutes 10 -KeepOpen        # leave the client up
#   .\b2.ps1 -Minutes 10 -Restore         # restore mocks + exe afterwards
#
# Accept: no #32770 dialog for the whole run, patched exe hash on disk, mocks
# loaded, grap ordinal 9 called, no kick.
param(
    [int]$Minutes = 10,
    [switch]$KeepOpen,
    [switch]$Restore
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\lib-admin.ps1"
Assert-Admin

$app   = 'C:\Nexon\Library\maplestorycw\appdata'
$exe   = Join-Path $app 'MapleStory.exe'
$probe = Join-Path $PSScriptRoot '..\..\neon-probe' | Resolve-Path
$hook  = Join-Path $probe 'hook-session32.dll'
$inject = Join-Path $probe 'inject32.exe'
$py    = 'C:\Users\John\AppData\Local\Programs\Python\Python312\python.exe'
$patch = Join-Path $PSScriptRoot '..\..\tools\ngs-patch\patch.py' | Resolve-Path
$launcher = 'C:\Program Files (x86)\Nexon\Nexon Launcher\nexon_launcher.exe'

$cleanHash   = '1942a7b31be7c3e582d00561c6dc41a5ea163df8ee65554859da7a3339f18f65'
$patchedHash = '0e9169d9fc0e322451ad13df16647b8d62f7c6683d23deadc17b47c5c2eefaf4'
$mockHash = @{
    'grap64.dll'     = 'D33642B922B0FF4F5A7B1E3F860CCB05E967FFA30B60CEC4D6BB8BCF39E6A6A8'
    'nmcogame64.dll' = 'DEE8FAC9ABA545EEF7569D8FD129346B0C841780CF19E8D15F315C462DFDF4E5'
}

$runId = 'B2-{0}' -f (Get-Date -Format 'yyyyMMdd-HHmmss')
$out = Join-Path (Resolve-Path "$PSScriptRoot\..\..") "builds\ngsx-runs\$runId"
New-Item -ItemType Directory -Force -Path $out | Out-Null
$runLog = Join-Path $out 'run.txt'
function Say($s) { $s | Tee-Object -FilePath $runLog -Append | Out-Host }

function Hash([string]$p) { (Get-FileHash -Algorithm SHA256 -LiteralPath $p).Hash.ToUpper() }
function Assert-Hash([string]$p, [string]$want, [string]$what) {
    $got = Hash $p
    if ($got -ne $want) { throw "$what hash $got != $want" }
}

Say "run $runId -> $out"
Assert-NotRunning MapleStory

# 1. clean exe, NGS off
Get-Process nexon_client, nexon_runtime, nexon_launcher -ErrorAction SilentlyContinue |
    Stop-Process -Force
Start-Sleep -Seconds 2
& $py $patch --restore $exe | Out-Host
Assert-Hash $exe $cleanHash 'clean exe'
& (Join-Path $PSScriptRoot 'ngs.ps1') stop | Out-Host
$bc = (Get-Service BlackCat1).Status
Say "BlackCat1: $bc  NGS: $((Get-Service NGS).Status)"

# 2. mocks
foreach ($n in $mockHash.Keys) {
    $dst = Join-Path $app $n
    if (-not (Test-Path -LiteralPath "$dst.orig")) {
        Copy-Item -LiteralPath $dst "$dst.orig" -Force
    }
    Copy-Item -LiteralPath (Join-Path $probe "mock-$($n -replace '\.dll$','')-win.dll") $dst -Force
    Assert-Hash $dst $mockHash[$n] "mock $n"
    Say "mock $n in place"
}
$fixtures = Join-Path $out 'fixtures.txt'
foreach ($n in $mockHash.Keys) { "{0}  {1}" -f (Hash (Join-Path $app $n)), $n |
    Add-Content -LiteralPath $fixtures }

# 3. launcher
Say 'starting launcher'
Start-Process -FilePath $launcher
$deadline = (Get-Date).AddSeconds(90)
while ((Get-Date) -lt $deadline) {
    if (Get-Process nexon_client -ErrorAction SilentlyContinue) { break }
    Start-Sleep -Milliseconds 500
}
if (-not (Get-Process nexon_client -ErrorAction SilentlyContinue)) { throw 'launcher did not start' }
Say 'launcher up'

# 4. wait for manifest to settle (the update check rewrites it)
$manifest = 'C:\Nexon\Library\maplestorycw\patchdata\59813.manifest.hash'
$deadline = (Get-Date).AddSeconds(120)
while ((Get-Date) -lt $deadline) {
    $age = (Get-Date) - (Get-Item $manifest).LastWriteTime
    if ($age.TotalSeconds -gt 8) { break }
    Start-Sleep -Seconds 1
}
Say "manifest settled ($([int]((Get-Date) - (Get-Item $manifest).LastWriteTime).TotalSeconds)s old)"

# 5. inject the hook into every launcher process, now and new ones
$launcherPids = @(Get-Process nexon_client, nexon_runtime -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty Id)
foreach ($n in 'nexon_launcher.exe', 'nexon_client.exe', 'nexon_runtime.exe') {
    $pn = $n -replace '\.exe$', ''
    if (-not (Get-Process $pn -ErrorAction SilentlyContinue)) { continue }
    $o = & $inject --all $n $hook 2>&1
    $o | Select-String 'injected|failed' | ForEach-Object { Say $_ }
}
foreach ($lp in $launcherPids) {
    if (-not (Test-Path "C:\Neon\logs\neon-session-hook.$lp.log")) { throw "hook not installed in launcher pid $lp" }
}
Say "hook installed in launcher: $($launcherPids -join ', ')"

$job = Start-Job -ScriptBlock {
    param($inject, $hook)
    while (-not (Get-Process MapleStory -ErrorAction SilentlyContinue)) {
        Get-Process nexon_launcher, nexon_client, nexon_runtime -ErrorAction SilentlyContinue | ForEach-Object {
            & $inject --all ($_.ProcessName + '.exe') $hook *> $null
        }
        Start-Sleep -Seconds 1
    }
} -ArgumentList $inject, $hook

# 6. PLAY
Say 'triggering PLAY'
Start-Process 'nxl://launch/59813'
$deadline = (Get-Date).AddSeconds(180)
$p = $null
while ((Get-Date) -lt $deadline) {
    $p = Get-Process MapleStory -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($p) { break }
    Start-Sleep -Seconds 1
}
Stop-Job $job -ErrorAction SilentlyContinue; Remove-Job $job -Force -ErrorAction SilentlyContinue
if (-not $p) { throw "client did not start (see $out)" }
$pidGame = $p.Id
$gameParent = (Get-CimInstance Win32_Process -Filter "ProcessId=$pidGame").ParentProcessId
Say "game pid $pidGame parent $gameParent"
if (-not (Test-Path "C:\Neon\logs\neon-session-hook.$gameParent.log")) {
    Say 'WARNING: the spawning process was not hooked'
}

$patchLog = "C:\Neon\logs\patch-launch.$pidGame.log"
$hookLogs = Get-ChildItem 'C:\Neon\logs\neon-session-hook.*.log' -ErrorAction SilentlyContinue
$exeHash = $null
for ($i = 0; $i -lt 40; $i++) {
    $exeHash = Hash $exe
    if ($exeHash -eq $patchedHash) { break }
    Start-Sleep -Milliseconds 250
}
Say "exe on disk: $exeHash"
if ($exeHash -ne $patchedHash) {
    Say "hook logs: $($hookLogs.FullName -join ', ')"
    throw 'client was NOT patched at launch (launcher repair won or hook not installed)'
}
Copy-Item -LiteralPath $patchLog $out -ErrorAction SilentlyContinue
Say 'client patched at launch'

# 7. mocks actually loaded
$mockGrap = "C:\Neon\logs\mock-grap64.$pidGame.log"
$mockNmco = "C:\Neon\logs\mock-nmcogame64.$pidGame.log"
for ($i = 0; $i -lt 40; $i++) {
    if ((Test-Path $mockGrap) -and (Test-Path $mockNmco)) { break }
    Start-Sleep -Milliseconds 250
}
if (-not (Test-Path $mockGrap)) { throw 'grap mock was not loaded' }
Copy-Item -LiteralPath $mockGrap, $mockNmco $out -ErrorAction SilentlyContinue
Say 'mocks loaded'

# 8. watch, fail early on the NGSX dialog
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class B2Win {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    public static List<string> Windows(uint pid) {
        var r = new List<string>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid) {
                var c = new StringBuilder(64); GetClassName(h, c, 64);
                if (c.ToString() == "#32770") r.Add(c.ToString());
            }
            return true;
        }, IntPtr.Zero);
        return r;
    }
}
'@

$watch = Start-Job -ScriptBlock {
    param($dir, $outDir, $secs)
    & (Join-Path $dir 'watch.ps1') -OutDir $outDir -Seconds $secs
} -ArgumentList $PSScriptRoot, $out, ($Minutes * 60)

$proc = [Diagnostics.Process]::GetProcessById($pidGame)
$dialog = $false
for ($i = 0; $i -lt ($Minutes * 60); $i++) {
    Start-Sleep -Seconds 1
    if ($proc.HasExited) { Say "client exited, code $($proc.ExitCode)"; break }
    if ([B2Win]::Windows([uint32]$pidGame).Count -gt 0) {
        $dialog = $true
        Say 'NGSX #32770 dialog'
        break
    }
}
Stop-Job $watch -ErrorAction SilentlyContinue
Remove-Job $watch -Force -ErrorAction SilentlyContinue
Say 'watch finished'

(Get-Content (Join-Path $out 'watch.jsonl') -ErrorAction SilentlyContinue | Select-Object -Last 1) |
    Add-Content -LiteralPath (Join-Path $out 'summary.txt')
"dialog=$dialog patchedHash=$((Hash $exe) -eq $patchedHash)" | Add-Content -LiteralPath (Join-Path $out 'summary.txt')

if (-not $KeepOpen) {
    Get-Process MapleStory -ErrorAction SilentlyContinue | Stop-Process -Force
}
if ($Restore) {
    & (Join-Path $PSScriptRoot 'ngs.ps1') start | Out-Null
    foreach ($n in $mockHash.Keys) {
        $dst = Join-Path $app $n
        if (Test-Path -LiteralPath "$dst.orig") { Copy-Item -LiteralPath "$dst.orig" $dst -Force }
    }
    & $py $patch --restore $exe | Out-Host
}
Say ("done: dialog={0} -> {1}" -f $dialog, $out)
if ($dialog) { exit 1 }
