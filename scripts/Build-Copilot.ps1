param(
    [ValidateSet('Build', 'Test', 'Clippy', 'Format', 'Check')]
    [string]$Action = 'Build',
    [string]$Filter = '',
    [switch]$Offline,
    [switch]$InsideVcEnv
)
$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$crate = Join-Path $project 'copilot'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin'

if (-not $InsideVcEnv) {
    $old = @{}
    foreach ($name in @('MAGICOPILOT_BUILD_ACTION', 'MAGICOPILOT_BUILD_FILTER', 'MAGICOPILOT_BUILD_OFFLINE', 'RUSTUP_TOOLCHAIN', 'PATH')) {
        $old[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    try {
        $env:MAGICOPILOT_BUILD_ACTION = $Action
        $env:MAGICOPILOT_BUILD_FILTER = $Filter
        $env:MAGICOPILOT_BUILD_OFFLINE = if ($Offline) { '1' } else { '' }
        $env:RUSTUP_TOOLCHAIN = '1.95.0-x86_64-pc-windows-msvc'
        $installer = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer'
        $env:Path = "$installer;$cargo;$env:Path"
        $vs = & (Join-Path $installer 'vswhere.exe') -latest -products '*' -property installationPath
        if (-not $vs) { throw 'Visual Studio Build Tools were not found' }
        $vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
        $shell = Join-Path $PSHOME 'pwsh.exe'
        if (-not (Test-Path -LiteralPath $shell)) { throw 'Run the build from PowerShell 7' }
        & $env:ComSpec /d /c "call `"$vcvars`" >nul && `"$shell`" -NoLogo -NoProfile -File `"$PSCommandPath`" -InsideVcEnv"
        if ($LASTEXITCODE -ne 0) { throw "magicopilot $Action failed with exit code $LASTEXITCODE" }
    } finally {
        foreach ($name in $old.Keys) { [Environment]::SetEnvironmentVariable($name, $old[$name], 'Process') }
    }
    return
}

$Action = $env:MAGICOPILOT_BUILD_ACTION
$Filter = $env:MAGICOPILOT_BUILD_FILTER
$config = @(
    '--config', 'source.crates-io.replace-with="official-github"',
    '--config', 'source.official-github.registry="sparse+https://raw.githubusercontent.com/rust-lang/crates.io-index/master/"',
    '--config', 'http.multiplexing=false',
    '--config', 'http.timeout=45'
)
if ($env:MAGICOPILOT_BUILD_OFFLINE) { $config += '--offline' }

# Windows Terminal's pseudo console, shipped next to magicopilot.exe. The console built into
# Windows answers terminal queries itself, so Copilot CLI would draw a different interface.
$conptyVersion = '1.24.260710001'
$conptySha256 = '175640566A3B59C4B132070EE96C2C77E5AB7EDD2E92732A5EB3610BBF63D90E'

function Install-ConPty([string]$Destination) {
    $cache = Join-Path $crate 'vendor'
    $package = Join-Path $cache "Microsoft.Windows.Console.ConPTY.$conptyVersion.nupkg"
    if (-not (Test-Path -LiteralPath $package -PathType Leaf)) {
        New-Item -ItemType Directory -Force -Path $cache | Out-Null
        $partial = "$package.partial"
        $uri = "https://www.nuget.org/api/v2/package/Microsoft.Windows.Console.ConPTY/$conptyVersion"
        Invoke-WebRequest -UseBasicParsing -Uri $uri -OutFile $partial
        Move-Item -Force -LiteralPath $partial -Destination $package
    }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $package).Hash
    if ($actual -ne $conptySha256) {
        Remove-Item -LiteralPath $package
        throw "The ConPTY package does not match its pinned SHA256 (got $actual)"
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($package)
    try {
        $files = @{
            'runtimes/win-x64/native/conpty.dll' = 'conpty.dll'
            'build/native/runtimes/x64/OpenConsole.exe' = 'OpenConsole.exe'
        }
        foreach ($entryName in $files.Keys) {
            $entry = $zip.GetEntry($entryName)
            if (-not $entry) { throw "The ConPTY package has no $entryName" }
            $target = Join-Path $Destination $files[$entryName]
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        }
    } finally {
        $zip.Dispose()
    }
}

Set-Location $crate
switch ($Action) {
    'Build' {
        & cargo build --release @config
        if ($LASTEXITCODE -eq 0) { Install-ConPty (Join-Path $crate 'target\release') }
    }
    'Check' { & cargo check --all-targets @config }
    'Test' {
        $arguments = @('test', '--release') + $config
        if ($Filter) { $arguments += @('--', $Filter) }
        & cargo @arguments
    }
    'Clippy' { & cargo clippy --release --all-targets @config -- -D warnings }
    'Format' { & cargo fmt }
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
