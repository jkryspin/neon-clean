# Shared helper: re-launch the current script elevated if needed.
# Usage at the top of a script:  . "$PSScriptRoot\lib-admin.ps1"; Assert-Admin

function Test-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal $id).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Assert-Admin {
    param([string]$ScriptPath = $PSCommandPath)
    if (Test-Admin) { return }
    $argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$ScriptPath`"")
    if ($args.Count) { $argList += $args }
    Write-Host "elevating: $ScriptPath"
    $p = Start-Process -FilePath (Get-Process -Id $PID).Path -Verb RunAs -ArgumentList $argList -PassThru
    $p.WaitForExit()
    exit $p.ExitCode
}

function Assert-NotRunning([string]$Name) {
    $p = Get-Process -Name $Name -ErrorAction SilentlyContinue
    if ($p) { throw "$Name is running (pid $($p.Id -join ', ')); close it first" }
}
