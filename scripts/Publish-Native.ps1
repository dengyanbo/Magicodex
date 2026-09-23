$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$source = Join-Path $project 'upstream\codex-rust-v0.153.4'
$binary = Join-Path $source 'codex-rs\target\release\codex.exe'
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
    throw 'A successful native release build is required before publication.'
}
$destination = Join-Path $project 'native'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$original = @'
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {dirname,join} from 'node:path';
import {homedir} from 'node:os';
const settings=JSON.parse(readFileSync(join(homedir(),'.codex','copilot-proxy','settings.json'),'utf8'));
const require=createRequire(settings.codexEntry);
const manifest=require.resolve('@openai/codex-win32-x64/package.json');
const version=JSON.parse(readFileSync(manifest,'utf8')).version;
if(version!=='0.153.4-win32-x64') throw new Error('Companion binary version mismatch');
console.log(join(dirname(manifest),'vendor','x86_64-pc-windows-msvc','bin','codex-code-mode-host.exe'));
'@ | node --input-type=module
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $original -PathType Leaf)) {
    throw 'The matching unchanged code-mode host is required for publication.'
}
Copy-Item -LiteralPath $binary -Destination (Join-Path $destination 'codex.exe')
Copy-Item -LiteralPath $original -Destination (Join-Path $destination 'codex-code-mode-host.exe')
Copy-Item -LiteralPath (Join-Path $source 'LICENSE') -Destination (Join-Path $destination 'LICENSE')
if (Test-Path -LiteralPath (Join-Path $source 'NOTICE')) {
    Copy-Item -LiteralPath (Join-Path $source 'NOTICE') -Destination (Join-Path $destination 'NOTICE')
}
& (Join-Path $destination 'codex.exe') --version
if ($LASTEXITCODE -ne 0) { throw 'Published executable did not start' }
Get-FileHash -LiteralPath (Join-Path $destination 'codex.exe') -Algorithm SHA256
