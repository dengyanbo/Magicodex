param(
    [ValidateSet('all', 'test', 'release', 'clippy', 'fmt')]
    [string]$Task = 'all',
    [switch]$Offline,
    [switch]$StandardRegistry
)
$ErrorActionPreference = 'Stop'
$originalPath = $env:Path
$originalEncodedFlags = $env:CARGO_ENCODED_RUSTFLAGS
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path -LiteralPath $cargoBin) { $env:Path = "$cargoBin;$env:Path" }
    $hostInfo = & rustc -vV
    if ($LASTEXITCODE -ne 0) { throw 'Rust toolchain is unavailable' }
    $hostTriple = (($hostInfo | Select-String '^host:').Line -replace '^host:\s*', '').Trim()
    # Panic messages carry the source paths of dependencies; keep the local user name out of the binary.
    $remap = "--remap-path-prefix=$env:USERPROFILE=~"
    if ($hostTriple -match 'x86_64-pc-windows-gnu') {
        if (-not (Get-Command dlltool.exe -ErrorAction SilentlyContinue)) {
            $portableRoot = Join-Path $env:USERPROFILE '.local\share\magicodex-build'
            $dlltool = Get-ChildItem -LiteralPath $portableRoot -Filter 'dlltool.exe' -Recurse -ErrorAction SilentlyContinue |
                Select-Object -First 1
            if (-not $dlltool) { throw 'GNU builds require dlltool.exe (for example from LLVM-MinGW). Add its bin directory to PATH.' }
            $env:Path = "$($dlltool.DirectoryName);$env:Path"
        }
        $sysroot = (& rustc --print sysroot).Trim()
        $linker = Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-gnu\bin\rust-lld.exe'
        $flags = if ($originalEncodedFlags) { $originalEncodedFlags.Split([char]0x1f) } elseif ($env:RUSTFLAGS) { $env:RUSTFLAGS -split '\s+' } else { @() }
        $env:CARGO_ENCODED_RUSTFLAGS = (@($flags) + @('-C', 'link-self-contained=yes', '-C', 'linker-flavor=ld.lld', '-C', "linker=$linker", $remap)) -join [char]0x1f
    }
    # Added to configured rustflags; the GNU build above passes it in CARGO_ENCODED_RUSTFLAGS instead.
    $config = @('--config', 'http.multiplexing=false', '--config', 'http.timeout=30', '--config', "target.$hostTriple.rustflags=['$remap']")
    if (-not $StandardRegistry) {
        $config += @(
            '--config', 'source.crates-io.replace-with="official-github"',
            '--config', 'source.official-github.registry="sparse+https://raw.githubusercontent.com/rust-lang/crates.io-index/master/"'
        )
    }
    $locked = @('--locked')
    if ($Offline) { $locked += '--offline' }
    if ($Task -in @('all', 'fmt')) {
        & cargo @config fmt --check
        if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed' }
    }
    if ($Task -in @('all', 'clippy')) {
        & cargo clippy @config @locked --all-targets -- -D warnings
        if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
    }
    if ($Task -in @('all', 'test')) {
        & cargo @config test @locked
        if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    }
    if ($Task -in @('all', 'release')) {
        & cargo @config build @locked --release
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
        Copy-Item -LiteralPath '.\target\release\magicodex.exe' -Destination '.\magicodex.exe'
    }
} finally {
    $env:Path = $originalPath
    $env:CARGO_ENCODED_RUSTFLAGS = $originalEncodedFlags
    Pop-Location
}
