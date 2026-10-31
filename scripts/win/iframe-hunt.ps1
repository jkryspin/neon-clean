# Narrow an already-seeded scan with an alternating changed/decreased rhythm while
# nudging the character through a mob. Prints the survivor count per pass.
param(
    [string]$Session = "s1",
    [int]$Passes = 80,
    [int]$NudgeEvery = 4,
    [int]$DelayMs = 150
)
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. "$PSScriptRoot\ms.ps1"
$exe = Join-Path $root "neon-ce\target\debug\neon-ce.exe"
$p = Get-Process MapleStory -ErrorAction SilentlyContinue
if (-not $p) { throw "no game" }
$h = Find-Window $p.Id -All
Bring-Top $h
Hold-Key 0x27 900 | Out-Null

for ($i = 0; $i -lt $Passes; $i++) {
    $cmp = if ($i % 2 -eq 0) { "changed" } else { "decreased" }
    $json = '{"session":"' + $Session + '","cmp":"' + $cmp + '"}'
    $r = $json | & $exe call scan_next - | ConvertFrom-Json
    if ($i % 5 -eq 0) { "i=$i $cmp left=$($r.left)" }
    if ($i % $NudgeEvery -eq $NudgeEvery - 1) {
        if (($i / $NudgeEvery) % 2 -eq 0) { Hold-Key 0x25 250 | Out-Null } else { Hold-Key 0x27 250 | Out-Null }
    }
    if ($r.left -eq 0) { "empty at $i"; break }
    Start-Sleep -Milliseconds $DelayMs
}
