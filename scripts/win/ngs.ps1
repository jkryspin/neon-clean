# NGS/BlackCat1 service + driver control for live tests.
#   .\ngs.ps1 install   copy BlackCat64.sys to the registered service path
#   .\ngs.ps1 start     start BlackCat1 then NGS
#   .\ngs.ps1 stop      stop NGS then BlackCat1
#   .\ngs.ps1 status
#   .\ngs.ps1 remove    stop + delete the copied driver file
param([Parameter(Mandatory)][ValidateSet('install', 'start', 'stop', 'status', 'remove')][string]$Action)
$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\lib-admin.ps1"

$svcPath = 'C:\ProgramData\Nexon\NGS\BlackCat1.sys'
$src = 'C:\Nexon\Library\maplestorycw\appdata\grap\BlackCat64.sys'

switch ($Action) {
    'install' {
        Assert-Admin
        if (-not (Test-Path -LiteralPath $src)) { throw "missing $src" }
        Copy-Item -LiteralPath $src $svcPath -Force
        Write-Host "installed $(Get-FileHash -Algorithm SHA256 -LiteralPath $svcPath | Select-Object -ExpandProperty Hash)"
    }
    'start' {
        Assert-Admin
        Write-Host "--- start BlackCat1"
        sc.exe start BlackCat1
        Start-Sleep -Seconds 2
        Write-Host "--- start NGS"
        sc.exe start NGS
        Start-Sleep -Seconds 2
        Get-Service BlackCat1, NGS | Format-Table Name, Status -Auto
    }
    'stop' {
        Assert-Admin
        sc.exe stop NGS | Out-Null
        sc.exe stop BlackCat1 | Out-Null
        Start-Sleep -Seconds 1
        Get-Service BlackCat1, NGS | Format-Table Name, Status -Auto
    }
    'status' {
        Get-Service BlackCat1, NGS | Format-Table Name, Status, StartType -Auto
        "driver file: " + (Test-Path -LiteralPath $svcPath)
        Get-Process NGService -ErrorAction SilentlyContinue | Select-Object Id, StartTime
    }
    'remove' {
        Assert-Admin
        sc.exe stop NGS | Out-Null; sc.exe stop BlackCat1 | Out-Null
        Start-Sleep -Seconds 1
        if (Test-Path -LiteralPath $svcPath) { Remove-Item -LiteralPath $svcPath -Force }
        Write-Host 'driver file removed'
    }
}
