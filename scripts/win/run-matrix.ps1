# Prepare the live client for one anti-cheat run.
#   A1  stock exe, real DLLs, NGS service + driver running   (baseline)
#   B1  patched exe, real DLLs, NGS stopped
#   B2  patched exe, mock DLLs, NGS stopped
# Usage: .\run-matrix.ps1 -Config B1 -Minutes 10
param(
    [Parameter(Mandatory)][ValidateSet('A1', 'B1', 'B2')][string]$Config,
    [int]$Minutes = 10,
    [switch]$KeepOpen
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\lib-admin.ps1"

$app = 'C:\Nexon\Library\maplestorycw\appdata'
$exe = Join-Path $app 'MapleStory.exe'
$py = 'C:\Users\John\AppData\Local\Programs\Python\Python312\python.exe'
$patch = Join-Path (Resolve-Path "$PSScriptRoot\..\..") 'tools\ngs-patch\patch.py'
$runId = '{0}-{1}' -f $Config, (Get-Date -Format 'yyyyMMdd-HHmmss')
$out = Join-Path (Resolve-Path "$PSScriptRoot\..\..") "builds\ngsx-runs\$runId"
New-Item -ItemType Directory -Force -Path $out | Out-Null

Assert-NotRunning MapleStory
Write-Host "run $runId -> $out"

function Set-Dlls([string]$mode) {
    foreach ($n in 'grap64.dll', 'nmcogame64.dll') {
        $target = Join-Path $app $n
        if ($mode -eq 'mock') {
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot "..\..\neon-probe\mock-$($n -replace '\.dll$','')-win.dll") $target -Force
        } else {
            Copy-Item -LiteralPath "$target.orig" $target -Force
        }
        "{0}  {1}  {2}" -f (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash, $n, $mode |
            Add-Content -LiteralPath (Join-Path $out 'fixtures.txt')
    }
}

$ngs = Join-Path $PSScriptRoot 'ngs.ps1'
switch ($Config) {
    'A1' {
        Set-Dlls real
        & $py $patch --restore $exe 2>$null | Out-Null
        & $ngs install; & $ngs start
    }
    'B1' {
        Set-Dlls real
        & $py $patch --apply $exe
        & $ngs stop
    }
    'B2' {
        Set-Dlls mock
        & $py $patch --apply $exe
        & $ngs stop
    }
}
& $py $patch --status $exe | Tee-Object -FilePath (Join-Path $out 'client.txt')

$before = Get-Date
Start-Process 'nxl://launch/59813'
$p = $null
for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Seconds 1
    $p = Get-Process MapleStory -ErrorAction SilentlyContinue
    if ($p) { break }
}
if (-not $p) { throw "client did not start within 60s (see $out)" }
"started pid $($p.Id) after $([int]((Get-Date) - $before).TotalSeconds)s" |
    Tee-Object -FilePath (Join-Path $out 'run.txt')

& (Join-Path $PSScriptRoot 'watch.ps1') -OutDir $out -Seconds ($Minutes * 60) |
    Tee-Object -FilePath (Join-Path $out 'watch.out.txt')

if (-not $KeepOpen) {
    Get-Process MapleStory -ErrorAction SilentlyContinue | Stop-Process -Force
    "client closed at $(Get-Date -Format o)" | Add-Content -LiteralPath (Join-Path $out 'run.txt')
}
"done: $out"
