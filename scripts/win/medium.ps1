# Run a command at the logged-on user's normal (medium) integrity level.
# An elevated shell's children stay high IL; the bot runs medium, so tests that care
# about integrity (handle stripping, UAC) must be launched through the task scheduler.
#
#   .\medium.ps1 -Command "cmd /c whoami" -OutFile out.txt [-TimeoutSec 60]
param(
    [Parameter(Mandatory)][string]$Command,
    [Parameter(Mandatory)][string]$OutFile,
    [int]$TimeoutSec = 60
)
$name = 'neon-medium-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$tr = "cmd /c $Command > `"$OutFile`" 2>&1"
schtasks /create /tn $name /tr $tr /sc once /st 00:00 /it /f | Out-Null
try {
    schtasks /run /tn $name | Out-Null
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 300
        $q = schtasks /query /tn $name /fo list 2>$null | Out-String
        if ($q -match 'Status:\s+Ready') { break }
        if ($q -match 'Status:\s+(Could not start|Disabled)') { break }
    }
} finally {
    schtasks /delete /tn $name /f 2>$null | Out-Null
}
if (Test-Path -LiteralPath $OutFile) { Get-Content -LiteralPath $OutFile }
