# Launch the neon-ce workbench (Python UI + Rust engine + MCP discovery file).
# Run from an elevated shell so the engine can open the game process.
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ui = Join-Path $root "neon-win\neon-ce.py"
$exe = Join-Path $root "neon-ce\target\debug\neon-ce.exe"
if (-not (Test-Path $exe)) {
    Push-Location (Join-Path $root "neon-ce")
    cargo build
    Pop-Location
}
Start-Process python -ArgumentList $ui -WorkingDirectory $root
