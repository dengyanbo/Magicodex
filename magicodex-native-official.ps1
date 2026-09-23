$ErrorActionPreference = 'Stop'
$binary = Join-Path $PSScriptRoot 'native\codex.exe'
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
    throw 'Native Codex has not been built. Run scripts\Build-Native.ps1 -Action Build first.'
}
& $binary -c 'check_for_update_on_startup=false' @args
exit $LASTEXITCODE
