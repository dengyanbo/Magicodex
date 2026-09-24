param(
    [ValidateSet('Fetch', 'Build', 'Test', 'Fix', 'Format', 'FormatRust', 'BazelLock')]
    [string]$Action = 'Build',
    [string]$Filter = '',
    [switch]$InsideVcEnv
)
$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$source = Join-Path $project 'upstream\codex-rust-v0.153.4'
$tools = Join-Path $env:USERPROFILE '.local\share\magicodex-native-tools'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin'

if (-not $InsideVcEnv) {
    $old = @{}
    foreach ($name in @('MAGICODEX_NATIVE_ACTION','MAGICODEX_NATIVE_FILTER','RUSTUP_TOOLCHAIN','PATH')) {
        $old[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    try {
        $env:MAGICODEX_NATIVE_ACTION = $Action
        $env:MAGICODEX_NATIVE_FILTER = $Filter
        $env:RUSTUP_TOOLCHAIN = '1.95.0-x86_64-pc-windows-msvc'
        $installer = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer'
        $env:Path = "$installer;$cargo;$tools;$env:Path"
        # Other products built on the Visual Studio shell, such as SSMS, have no C++ tools.
        $vs = & (Join-Path $installer 'vswhere.exe') -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $vs) { throw 'Visual Studio Build Tools with the C++ x64 tools were not found' }
        $vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
        $shell = Join-Path $PSHOME 'pwsh.exe'
        if (-not (Test-Path -LiteralPath $shell)) { throw 'Run native builds from PowerShell 7' }
        & $env:ComSpec /d /c "call `"$vcvars`" >nul && `"$shell`" -NoLogo -NoProfile -File `"$PSCommandPath`" -InsideVcEnv"
        if ($LASTEXITCODE -ne 0) { throw "Native $Action failed with exit code $LASTEXITCODE" }
    } finally {
        foreach ($name in $old.Keys) { [Environment]::SetEnvironmentVariable($name, $old[$name], 'Process') }
    }
    return
}

$Action = $env:MAGICODEX_NATIVE_ACTION
$Filter = $env:MAGICODEX_NATIVE_FILTER
$env:CARGO_PROFILE_RELEASE_DEBUG = '0'
$env:CARGO_PROFILE_RELEASE_STRIP = 'symbols'
$env:CARGO_BUILD_JOBS = '4'
# --remap-path-prefix only reaches rustc. The C in aws-lc, liblzma and tree-sitter keeps __FILE__ in
# its assertions, so cl.exe trims the user directory from those paths too. The option goes through
# CL, which cl.exe reads itself: with CFLAGS set, the cc crate drops its default warning level, and
# aws-lc's compiler checks then take __builtin_bswap for supported and the link fails.
$env:CL = "$env:CL `"/d1trimfile:$env:USERPROFILE\\`"".Trim()
$cmake = Join-Path $env:VSINSTALLDIR 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'
$ninja = Join-Path $env:VSINSTALLDIR 'Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja'
if (Test-Path -LiteralPath $cmake) { $env:Path = "$cmake;$ninja;$env:Path" }
$config = @(
    '--config', 'source.crates-io.replace-with="official-github"',
    '--config', 'source.official-github.registry="sparse+https://raw.githubusercontent.com/rust-lang/crates.io-index/master/"',
    '--config', 'http.multiplexing=false',
    '--config', 'http.timeout=45',
    # Keeps the local user name out of the binary's panic messages. RUSTFLAGS would instead replace
    # the stack size and static CRT flags in codex-rs\.cargo\config.toml; this adds to them.
    '--config', "target.x86_64-pc-windows-msvc.rustflags=['--remap-path-prefix=$env:USERPROFILE=~']"
)
Set-Location (Join-Path $source 'codex-rs')
if ($Action -in @('Build', 'Test', 'Fix')) {
    & python (Join-Path $PSScriptRoot 'Prepare-NativeWindows.py')
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
switch ($Action) {
    'Fetch' { & cargo @config fetch --locked --target x86_64-pc-windows-msvc }
    'Build' { & cargo @config build --locked --release -p codex-cli --bin codex }
    'Test' {
        $arguments = @('-p','codex-tui','--locked','--release') + $config
        $arguments += @('--config', 'profile.release.package.codex-tui.opt-level=0')
        if ($Filter) { $arguments += @('-E', $Filter) }
        & just test @arguments
    }
    'Fix' { & just fix -p codex-tui --allow-no-vcs --release @config }
    'Format' { & just fmt }
    'FormatRust' { & cargo fmt -p codex-tui -- --config imports_granularity=Item }
    'BazelLock' { Set-Location $source; & just bazel-lock-update }
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
