$ErrorActionPreference = 'Stop'
$launcher = Join-Path $PSScriptRoot 'scripts\Start-NativeBridge.mjs'
& node $launcher @args
exit $LASTEXITCODE
