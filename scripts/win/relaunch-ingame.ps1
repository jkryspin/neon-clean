# Kill, relaunch and walk to in-game. Ratios are relative to the current window rect,
# so the client moving its own window between screens does not matter.
param([string]$Url = "nxl://launch/59813", [int]$StartTimeoutSec = 90)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\ms.ps1"

$LOGIN = @(0.7553, 0.4576)
$CH_A   = @(0.4726, 0.7014)
$CH_B   = @(0.7797, 0.8612)
$GO     = @(0.6451, 0.4939)
$OKDLG  = @(0.4901, 0.5383)
$CHAR   = @(0.3476, 0.5698)

Get-Process MapleStory -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process nexon_client, nexon_runtime -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 3
Start-Process $Url

$deadline = (Get-Date).AddSeconds($StartTimeoutSec)
do { Start-Sleep -Seconds 2; $p = Get-Process MapleStory -ErrorAction SilentlyContinue | Select-Object -First 1 } until ($p -or (Get-Date) -gt $deadline)
if (-not $p) { throw "client did not start" }
Write-Host "pid $($p.Id)"

$h = [IntPtr]::Zero
for ($i = 0; $i -lt 40; $i++) {
    Start-Sleep -Seconds 1
    $h = Find-Window $p.Id
    if ($h -ne [IntPtr]::Zero) { break }
}
if ($h -eq [IntPtr]::Zero) { throw "no game window" }
Start-Sleep -Seconds 9

Bring-Top $h
Click-Rel $h $LOGIN[0] $LOGIN[1]
Start-Sleep -Seconds 9

foreach ($ch in @($CH_A, $CH_B)) {
    Bring-Top $h
    Click-Rel $h $ch[0] $ch[1]
    Start-Sleep -Milliseconds 400
    Click-Rel $h $GO[0] $GO[1]
    Start-Sleep -Seconds 5
    Click-Rel $h $OKDLG[0] $OKDLG[1]
    Start-Sleep -Seconds 3
}

Bring-Top $h
Double-Click-Rel $h $CHAR[0] $CHAR[1]
Start-Sleep -Seconds 15
Write-Host "walked, pid $($p.Id)"
Shot "auto_game"
