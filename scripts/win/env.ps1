# Build environment: MSVC + WDK for the neon-guard kernel driver.
# Dot-source this, then run cl / msbuild.
# Usage:  . "$PSScriptRoot\env.ps1"

$vs = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools"
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
if (-not (Test-Path -LiteralPath $vcvars)) { throw "vcvars64.bat not found: $vcvars" }

# Pull the MSVC environment into this PowerShell session.
$envDump = & cmd /c "`"$vcvars`" >nul 2>&1 && set"
foreach ($line in $envDump) {
    if ($line -match '^([^=]+)=(.*)$') { Set-Item -Path ("env:" + $matches[1]) -Value $matches[2] }
}

$root = 'C:\Program Files (x86)\Windows Kits\10'
$ver = Get-ChildItem (Join-Path $root 'Include') -Directory -ErrorAction SilentlyContinue |
    Where-Object { Test-Path (Join-Path $_.FullName 'km') } |
    Sort-Object Name -Descending | Select-Object -First 1
if (-not $ver) { throw "no WDK km headers under $root\Include (install the Windows Driver Kit)" }

$env:WDK_VERSION = $ver.Name
$env:WDK_INC = Join-Path $root "Include\$($ver.Name)\km"
$env:WDK_LIB = Join-Path $root "Lib\$($ver.Name)\km\x64"
foreach ($p in @($env:WDK_INC, (Join-Path $root "Include\$($ver.Name)\um"), (Join-Path $root "Include\$($ver.Name)\shared"))) {
    if (Test-Path -LiteralPath $p) { $env:INCLUDE = $p + ';' + $env:INCLUDE }
}
if (Test-Path -LiteralPath $env:WDK_LIB) { $env:LIB = $env:WDK_LIB + ';' + $env:LIB }

Write-Host "WDK $($ver.Name) ready; cl = $((Get-Command cl).Source)"
