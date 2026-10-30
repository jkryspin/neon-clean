# Run a command as the desktop user in the *interactive logon session* (not the
# scheduler's separate logon session). explorer.exe brokers it, so the child carries the
# session SID S-1-5-5-0-<id> that the default process DACL grants VM_READ to.
#
#   .\medium-shell.ps1 -Command "cmd /c whoami" -OutFile out.txt [-TimeoutSec 60]
param(
    [Parameter(Mandatory)][string]$Command,
    [Parameter(Mandatory)][string]$OutFile,
    [int]$TimeoutSec = 60
)
Remove-Item -LiteralPath $OutFile -ErrorAction SilentlyContinue
$shell = New-Object -ComObject Shell.Application
$shell.ShellExecute('cmd.exe', "/c $Command > `"$OutFile`" 2>&1", '', 'open', 0)
$deadline = (Get-Date).AddSeconds($TimeoutSec)
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 300
    if (Test-Path -LiteralPath $OutFile) {
        Start-Sleep -Milliseconds 400
        $len = (Get-Item -LiteralPath $OutFile).Length
        if ($len -gt 0) { break }
    }
}
if (Test-Path -LiteralPath $OutFile) { Get-Content -LiteralPath $OutFile }
