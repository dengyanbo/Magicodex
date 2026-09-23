<#
.SYNOPSIS
安装 Magicodex 发布版：Codex CLI 原生补丁、GitHub Copilot CLI 外壳或独立前端。
Installs a Magicodex release: the Codex CLI patch, the Copilot CLI wrapper or the standalone frontend.

.DESCRIPTION
每种版本是一个独立的 GitHub Release（codex-v*、copilot-v*、standalone-v*）。本脚本列出发布、
下载所选版本、按 SHA256SUMS.txt 校验后解压到 <InstallDir>\<版本类型>\<版本号>，并在
<InstallDir>\bin 创建命令入口。只有加 -AddToPath 才会修改用户 PATH。

仓库为私有时需要已登录的 GitHub CLI（gh auth login）；公开仓库也可以不用 gh。

.EXAMPLE
.\install.ps1                                   # 交互式选择
.\install.ps1 -Variant copilot                  # 安装最新的 copilot 版本
.\install.ps1 -Variant codex -Version 0.5.0 -AddToPath
.\install.ps1 -List                             # 列出可安装与已安装的版本
.\install.ps1 -Uninstall -Variant copilot
.\install.ps1 -Variant copilot -Source .\downloads   # 从已下载的 zip + SHA256SUMS.txt 安装
#>
[CmdletBinding()]
param(
    [ValidateSet('codex', 'copilot', 'standalone')]
    [string]$Variant,
    [string]$Version = 'latest',
    [string]$InstallDir,
    [string]$Repo = 'dengyanbo/Magicodex',
    [string]$Source,
    [switch]$AddToPath,
    [switch]$List,
    [switch]$Uninstall,
    [switch]$Force
)
Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA 'Magicodex' }
# A relative path is relative to the current PowerShell location, not the process directory.
$InstallDir = [IO.Path]::GetFullPath($PSCmdlet.GetUnresolvedProviderPathFromPSPath($InstallDir)).TrimEnd('\')
$binDir = Join-Path $InstallDir 'bin'
# Test hook: the registry key under HKCU that holds the user environment.
$environmentKey = if ($env:MAGICODEX_INSTALL_ENV_KEY) { $env:MAGICODEX_INSTALL_ENV_KEY } else { 'Environment' }

$catalog = [ordered]@{
    codex = @{
        Title = 'Codex CLI 0.153.4 原生魔法阵补丁'
        Detail = '原版 Codex 界面 + 魔法阵，使用你自己的 Codex 登录'
        Pattern = '^magicodex-codex-(.+)-windows-x64\.zip$'
    }
    copilot = @{
        Title = 'GitHub Copilot CLI 魔法阵外壳 (magicopilot)'
        Detail = '运行你已安装的原版 Copilot CLI，在它上方画魔法阵'
        Pattern = '^magicopilot-(.+)-windows-x64\.zip$'
    }
    standalone = @{
        Title = '独立前端（最早的版本）'
        Detail = '自绘界面，通过 Codex app-server 对话'
        Pattern = '^magicodex-standalone-(.+)-windows-x64\.zip$'
    }
}

function Get-SortableVersion([string]$Text) {
    $core = ($Text -split '[-+]', 2)[0]
    $parsed = $null
    if ([version]::TryParse($core, [ref]$parsed)) { return $parsed }
    return [version]'0.0'
}

function Invoke-Gh([string[]]$Arguments) {
    # Windows PowerShell turns redirected stderr into errors; report failures by exit code.
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $lines = & gh @Arguments 2>$null
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $saved
    }
    [pscustomobject]@{ Code = $code; Lines = @($lines) }
}

function Test-Gh {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { return $false }
    return (Invoke-Gh @('auth', 'status')).Code -eq 0
}

function New-ReleaseRecord([string]$VariantName, [string]$ReleaseVersion, [string]$Tag, [bool]$Prerelease, [string]$Published, [string]$Asset) {
    [pscustomobject]@{
        Variant = $VariantName
        Version = $ReleaseVersion
        Tag = $Tag
        Prerelease = $Prerelease
        Published = $Published
        Asset = $Asset
        Zip = $null
        Sums = $null
        ZipUrl = $null
        SumsUrl = $null
    }
}

function Get-Releases {
    $found = @()
    if ($Source) {
        foreach ($zip in Get-ChildItem -LiteralPath $Source -Recurse -File -Filter '*.zip') {
            foreach ($name in $catalog.Keys) {
                if ($zip.Name -match $catalog[$name].Pattern) {
                    $release = New-ReleaseRecord $name $Matches[1] "$name-v$($Matches[1])" $false '' $zip.Name
                    $release.Zip = $zip.FullName
                    $release.Sums = Join-Path $zip.DirectoryName 'SHA256SUMS.txt'
                    $found += $release
                }
            }
        }
        return $found
    }
    if (Test-Gh) {
        # One line per asset; the jq program has no string literals, which Windows PowerShell
        # would mangle when passing them to gh.
        $jq = '.[] | select(.draft | not) | .tag_name as $t | (.prerelease | tostring) as $p | (.published_at | tostring) as $d | .assets[] | [$t, $p, $d, .name] | @tsv'
        $result = Invoke-Gh @('api', '--paginate', "repos/$Repo/releases?per_page=100", '--jq', $jq)
        if ($result.Code -ne 0) { throw "无法读取 $Repo 的发布列表（gh api 失败）。" }
        foreach ($line in $result.Lines) {
            $fields = "$line" -split "`t"
            if ($fields.Count -lt 4) { continue }
            if (-not ($fields[0] -match '^(codex|copilot|standalone)-v(.+)$')) { continue }
            $name = $Matches[1]
            if ($fields[3] -match $catalog[$name].Pattern) {
                $published = if ($fields[2] -ne 'null') { $fields[2].Substring(0, [Math]::Min(10, $fields[2].Length)) } else { '' }
                $found += New-ReleaseRecord $name $Matches[1] $fields[0] ($fields[1] -eq 'true') $published $fields[3]
            }
        }
        return $found
    }
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $headers = @{ 'User-Agent' = 'magicodex-installer'; Accept = 'application/vnd.github+json' }
    try {
        $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases?per_page=100" -Headers $headers
    } catch {
        throw "无法读取 $Repo 的发布列表。仓库为私有时，请先安装 GitHub CLI 并登录（gh auth login）。$($_.Exception.Message)"
    }
    foreach ($item in @($releases)) {
        if ($item.draft) { continue }
        if (-not ($item.tag_name -match '^(codex|copilot|standalone)-v(.+)$')) { continue }
        $name = $Matches[1]
        $sums = @($item.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' }) | Select-Object -First 1
        foreach ($asset in @($item.assets)) {
            if ($asset.name -match $catalog[$name].Pattern) {
                $published = if ($item.published_at) { ([string]$item.published_at).Substring(0, 10) } else { '' }
                $release = New-ReleaseRecord $name $Matches[1] $item.tag_name ([bool]$item.prerelease) $published $asset.name
                $release.ZipUrl = $asset.browser_download_url
                if ($sums) { $release.SumsUrl = $sums.browser_download_url }
                $found += $release
            }
        }
    }
    return $found
}

function Select-Release([object[]]$Releases, [string]$VariantName) {
    $matching = @($Releases | Where-Object { $_.Variant -eq $VariantName })
    if ($Version -ne 'latest') {
        $wanted = $Version.TrimStart('v')
        $exact = @($matching | Where-Object { $_.Version -eq $wanted })
        if (-not $exact) { throw "没有 $VariantName $wanted 这个版本。可用：$((@($matching | ForEach-Object { $_.Version }) -join ', '))" }
        return $exact[0]
    }
    if (-not $matching) { throw "没有可安装的 $VariantName 发布。" }
    $stable = @($matching | Where-Object { -not $_.Prerelease })
    $pool = if ($stable) { $stable } else { $matching }
    return @($pool | Sort-Object -Property @{ Expression = { Get-SortableVersion $_.Version } } -Descending)[0]
}

function Save-ReleaseFiles($Release, [string]$Directory) {
    $zip = Join-Path $Directory $Release.Asset
    $sums = Join-Path $Directory 'SHA256SUMS.txt'
    if ($Release.Zip) {
        if (-not (Test-Path -LiteralPath $Release.Sums -PathType Leaf)) { throw "$($Release.Zip) 旁边没有 SHA256SUMS.txt。" }
        Copy-Item -LiteralPath $Release.Zip -Destination $zip
        Copy-Item -LiteralPath $Release.Sums -Destination $sums
    } elseif ($Release.ZipUrl) {
        if (-not $Release.SumsUrl) { throw "$($Release.Tag) 没有 SHA256SUMS.txt，无法校验。" }
        Invoke-WebRequest -UseBasicParsing -Uri $Release.ZipUrl -OutFile $zip
        Invoke-WebRequest -UseBasicParsing -Uri $Release.SumsUrl -OutFile $sums
    } else {
        Write-Host "正在下载 $($Release.Asset) ..."
        $result = Invoke-Gh @('release', 'download', $Release.Tag, '--repo', $Repo, '--pattern', $Release.Asset,
            '--pattern', 'SHA256SUMS.txt', '--dir', $Directory, '--clobber')
        if ($result.Code -ne 0) { throw "gh release download $($Release.Tag) 失败。" }
    }
    foreach ($file in $zip, $sums) {
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "下载不完整：缺少 $(Split-Path $file -Leaf)。" }
    }
    return @($zip, $sums)
}

# .NET instead of Get-FileHash, which Windows PowerShell autoloads from a module that a
# PowerShell 7 module path can shadow.
function Get-Sha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $sha = [Security.Cryptography.SHA256]::Create()
        try { return ([BitConverter]::ToString($sha.ComputeHash($stream)) -replace '-', '').ToLowerInvariant() }
        finally { $sha.Dispose() }
    } finally {
        $stream.Dispose()
    }
}

function Assert-Checksum([string]$Zip, [string]$Sums) {
    $name = Split-Path $Zip -Leaf
    $expected = $null
    foreach ($line in Get-Content -LiteralPath $Sums) {
        if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+?)\s*$' -and $Matches[2] -eq $name) { $expected = $Matches[1].ToLowerInvariant() }
    }
    if (-not $expected) { throw "SHA256SUMS.txt 中没有 $name。" }
    $actual = Get-Sha256 $Zip
    if ($actual -ne $expected) { throw "$name 的 SHA256 不匹配（期望 $expected，实际 $actual），已停止安装。" }
    Write-Host "SHA256 校验通过：$actual"
}

function Read-Package([string]$Directory) {
    $manifest = Join-Path $Directory 'magicodex-package.json'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw "$Directory 中没有 magicodex-package.json。" }
    return Get-Content -Raw -Encoding UTF8 -LiteralPath $manifest | ConvertFrom-Json
}

function Assert-Package($Package, $Release) {
    if ($Package.variant -ne $Release.Variant -or $Package.version -ne $Release.Version) {
        throw "包内容（$($Package.variant) $($Package.version)）与所选发布（$($Release.Variant) $($Release.Version)）不符。"
    }
}

# Folders this installer made under <InstallDir>\<variant>: a package of that variant, or what
# is left of one that was being replaced or removed. Nothing else there is ever deleted.
function Test-OwnDirectory([string]$Directory, [string]$VariantName) {
    if ((Split-Path $Directory -Leaf) -match '\.old-[0-9a-f]{8}$') { return $true }
    $manifest = Join-Path $Directory 'magicodex-package.json'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { return $false }
    try { $package = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifest | ConvertFrom-Json } catch { return $false }
    return ($package -is [Management.Automation.PSCustomObject]) -and
        (@($package.PSObject.Properties.Name) -contains 'variant') -and ($package.variant -eq $VariantName)
}

# Running programs and loaded DLLs cannot be opened for writing; neither can files another
# program holds open without sharing them.
function Test-FileBusy([string]$Path) {
    try {
        [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]'ReadWrite, Delete').Dispose()
        return $false
    } catch {
        $e = $_.Exception
        while ($e.InnerException) { $e = $e.InnerException }
        # ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION
        return ($e -is [IO.IOException]) -and (($e.HResult -band 0xFFFF) -in 32, 33)
    }
}

# A file that keeps $Directory in use. Windows still lets such a folder be renamed, and deleting
# it would then remove everything but that file, so a folder in use is left alone.
function Get-BusyFile([string]$Directory) {
    foreach ($file in @(Get-ChildItem -LiteralPath $Directory -Recurse -File -Force -ErrorAction SilentlyContinue)) {
        if (Test-FileBusy $file.FullName) { return $file.FullName }
    }
    return $null
}

# Renames first, so a delete that fails half way leaves a folder marked as a leftover.
function Remove-OwnDirectory([string]$Directory) {
    $busy = Get-BusyFile $Directory
    if ($busy) { throw "$busy 正在使用" }
    $doomed = $Directory
    if ($Directory -notmatch '\.old-[0-9a-f]{8}$') {
        $doomed = "$Directory.old-" + [guid]::NewGuid().ToString('N').Substring(0, 8)
        Move-Item -LiteralPath $Directory -Destination $doomed
    }
    Remove-Item -Recurse -Force -LiteralPath $doomed
}

# Puts a verified, unpacked package in place of the installed one, if any.
function Set-PackageDirectory([string]$Staging, [string]$Target) {
    if (-not (Test-Path -LiteralPath $Target)) {
        Move-Item -LiteralPath $Staging -Destination $Target
        return
    }
    $busy = Get-BusyFile $Target
    if ($busy) { throw "无法替换 $Target：$busy 正在使用。请先退出该程序再重试，已安装的版本未做改动。" }
    $backup = "$Target.old-" + [guid]::NewGuid().ToString('N').Substring(0, 8)
    try { Move-Item -LiteralPath $Target -Destination $backup }
    catch { throw "无法替换 $Target（程序是否仍在运行？）：$($_.Exception.Message)" }
    try { Move-Item -LiteralPath $Staging -Destination $Target }
    catch {
        Move-Item -LiteralPath $backup -Destination $Target
        throw
    }
    # The backup is removed with the other old versions after the install.
}

function Get-Commands($Package) {
    $commands = [ordered]@{}
    foreach ($property in $Package.commands.PSObject.Properties) {
        $commands[$property.Name] = [string]$property.Value
    }
    return $commands
}

function Write-Shims([string]$VariantName, [string]$ReleaseVersion, [string]$Target, $Package) {
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    $commands = Get-Commands $Package
    foreach ($name in $commands.Keys) {
        $relative = $commands[$name]
        if ($name -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$' -or $relative -notmatch '^[A-Za-z0-9._\\/-]+$' -or $relative -match '\.\.') {
            throw "magicodex-package.json 中的命令无效：$name -> $relative"
        }
        if (-not (Test-Path -LiteralPath (Join-Path $Target $relative) -PathType Leaf)) { throw "包内缺少 $relative。" }
        # Relative to the shim, so that profile paths with non-ASCII characters never reach cmd.exe's code page.
        $path = "%~dp0..\$VariantName\$ReleaseVersion\$($relative.Replace('/', '\'))"
        $invoke = if ($relative -match '\.(cmd|bat)$') { "call `"$path`" %*" } else { "`"$path`" %*" }
        $content = "@echo off`r`nrem Magicodex $VariantName $ReleaseVersion - created by install.ps1`r`n$invoke`r`nexit /b %ERRORLEVEL%`r`n"
        [IO.File]::WriteAllText((Join-Path $binDir "$name.cmd"), $content, [Text.Encoding]::ASCII)
    }
}

function Read-State([string]$VariantName) {
    $state = Join-Path (Join-Path $InstallDir $VariantName) 'installed.json'
    if (Test-Path -LiteralPath $state -PathType Leaf) { return Get-Content -Raw -LiteralPath $state | ConvertFrom-Json }
    return $null
}

function Remove-Shims([string]$VariantName, [string[]]$Names) {
    foreach ($name in $Names) {
        $shim = Join-Path $binDir "$name.cmd"
        if ((Test-Path -LiteralPath $shim) -and ([IO.File]::ReadAllText($shim) -match "rem Magicodex $VariantName ")) {
            Remove-Item -LiteralPath $shim
        }
    }
}

function Get-UserPath {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($environmentKey)
    if (-not $key) { return '' }
    try { return [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } finally { $key.Close() }
}

function Set-UserPath([string]$Value) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($environmentKey)
    try {
        # Keep REG_EXPAND_SZ, so entries such as %USERPROFILE%\bin still expand.
        $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
        if ($key.GetValueNames() -contains 'Path') { $kind = $key.GetValueKind('Path') }
        $key.SetValue('Path', $Value, $kind)
    } finally {
        $key.Close()
    }
    if (-not ('Magicodex.NativeMethods' -as [type])) {
        Add-Type -Namespace Magicodex -Name NativeMethods -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
'@
    }
    $ignored = [UIntPtr]::Zero
    # WM_SETTINGCHANGE to all windows, so new terminals see the new PATH.
    [void][Magicodex.NativeMethods]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$ignored)
}

function Test-InPath([string]$PathList, [string]$Directory) {
    foreach ($entry in $PathList -split ';') {
        if ($entry -and [Environment]::ExpandEnvironmentVariables($entry).TrimEnd('\') -ieq $Directory) { return $true }
    }
    return $false
}

function Add-BinToUserPath {
    $current = Get-UserPath
    if (Test-InPath $current $binDir) { return $false }
    $entries = @($current -split ';' | Where-Object { $_ })
    Set-UserPath ((@($entries) + $binDir) -join ';')
    return $true
}

function Remove-BinFromUserPath {
    $current = Get-UserPath
    if (-not (Test-InPath $current $binDir)) { return }
    $kept = @($current -split ';' | Where-Object { $_ -and [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -ine $binDir })
    Set-UserPath ($kept -join ';')
    Write-Host "已从用户 PATH 移除 $binDir"
}

function Show-Installed {
    Write-Host "已安装（$InstallDir）："
    $any = $false
    foreach ($name in $catalog.Keys) {
        $state = Read-State $name
        if ($state) {
            $any = $true
            Write-Host ('  {0,-11} {1,-10} 命令：{2}' -f $name, $state.version, (@($state.commands) -join ', '))
        }
    }
    if (-not $any) { Write-Host '  （无）' }
}

function Show-Releases([object[]]$Releases) {
    $origin = if ($Source) { $Source } else { $Repo }
    Write-Host "可安装的发布（$origin）："
    foreach ($release in @($Releases | Sort-Object Variant, @{ Expression = { Get-SortableVersion $_.Version }; Descending = $true })) {
        $flag = if ($release.Prerelease) { '预发布' } else { '' }
        Write-Host ('  {0,-11} {1,-10} {2,-6} {3,-10} {4}' -f $release.Variant, $release.Version, $flag, $release.Published, $catalog[$release.Variant].Title)
    }
    if (-not $Releases) { Write-Host '  （无）' }
}

function Install-Release($Release) {
    $variantDir = Join-Path $InstallDir $Release.Variant
    $target = Join-Path $variantDir $Release.Version
    if ((Test-Path -LiteralPath $target) -and -not (Test-OwnDirectory $target $Release.Variant)) {
        throw "$target 已存在，但不是本安装程序安装的 $($Release.Variant) 版本（没有对应的 magicodex-package.json）。请移走它后重试，未做任何改动。"
    }
    if (-not $Force -and (Test-Path -LiteralPath (Join-Path $target 'magicodex-package.json'))) {
        Write-Host "$($Release.Variant) $($Release.Version) 已在 $target，只重新创建命令入口（-Force 可重新下载）。"
    } else {
        $temp = Join-Path ([IO.Path]::GetTempPath()) ('magicodex-install-' + [guid]::NewGuid().ToString('N'))
        $staging = "$target.partial"
        New-Item -ItemType Directory -Path $temp | Out-Null
        try {
            $zip, $sums = Save-ReleaseFiles $Release $temp
            Assert-Checksum $zip $sums
            if (Test-Path -LiteralPath $staging) { Remove-Item -Recurse -Force -LiteralPath $staging }
            New-Item -ItemType Directory -Force -Path $variantDir | Out-Null
            Write-Host '正在解压 ...'
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            [IO.Compression.ZipFile]::ExtractToDirectory($zip, $staging)
            Assert-Package (Read-Package $staging) $Release
            # Only a downloaded, verified and unpacked copy replaces an installed one.
            Set-PackageDirectory $staging $target
        } finally {
            Remove-Item -Recurse -Force -LiteralPath $temp -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $staging) { Remove-Item -Recurse -Force -LiteralPath $staging -ErrorAction SilentlyContinue }
        }
    }
    $package = Read-Package $target
    Assert-Package $package $Release
    $previous = Read-State $Release.Variant
    if ($previous) { Remove-Shims $Release.Variant @($previous.commands) }
    Write-Shims $Release.Variant $Release.Version $target $package
    $commands = @((Get-Commands $package).Keys)
    $state = [ordered]@{ variant = $Release.Variant; version = $Release.Version; tag = $Release.Tag; commands = $commands; installed = (Get-Date).ToString('s') }
    [IO.File]::WriteAllText((Join-Path $variantDir 'installed.json'), ($state | ConvertTo-Json), (New-Object Text.UTF8Encoding $false))

    foreach ($old in Get-ChildItem -LiteralPath $variantDir -Directory) {
        if ($old.Name -eq $Release.Version -or -not (Test-OwnDirectory $old.FullName $Release.Variant)) { continue }
        try {
            Remove-OwnDirectory $old.FullName
            if ($old.Name -notmatch '\.old-[0-9a-f]{8}$') { Write-Host "已移除旧版本 $($old.Name)" }
        } catch {
            Write-Warning "未删除 $($old.FullName)（$($_.Exception.Message)），已原样保留，下次安装或卸载时会再试。"
        }
    }

    Write-Host ''
    Write-Host "已安装 $($Release.Variant) $($Release.Version)：$target"
    foreach ($name in $commands) { Write-Host "  命令 $name  ->  $(Join-Path $binDir "$name.cmd")" }
    if ($AddToPath) {
        if (Add-BinToUserPath) { Write-Host "已把 $binDir 加入用户 PATH；新开的终端中可直接运行上面的命令。" }
        else { Write-Host "$binDir 已在用户 PATH 中。" }
    } elseif (-not (Test-InPath $env:Path $binDir) -and -not (Test-InPath (Get-UserPath) $binDir)) {
        Write-Host "$binDir 不在 PATH 中：可用 -AddToPath 重新运行，或直接使用上面的完整路径。"
    }
    if ($Release.Variant -eq 'copilot' -and -not (Get-Command copilot -ErrorAction SilentlyContinue)) {
        Write-Warning '没有在 PATH 中找到 copilot。magicopilot 需要已安装并登录的 GitHub Copilot CLI（npm install -g @github/copilot）。'
    }
}

function Uninstall-Variant([string]$VariantName) {
    $variantDir = Join-Path $InstallDir $VariantName
    $state = Read-State $VariantName
    $own = @()
    if (Test-Path -LiteralPath $variantDir) {
        $own = @(Get-ChildItem -LiteralPath $variantDir -Directory | Where-Object { Test-OwnDirectory $_.FullName $VariantName })
    }
    if (-not $state -and -not $own) {
        if (Test-Path -LiteralPath $variantDir) {
            throw "$variantDir 中没有本安装程序安装的 $VariantName 版本（没有 installed.json 或 magicodex-package.json），未删除任何内容。"
        }
        Write-Host "$VariantName 没有安装在 $InstallDir。"
        return
    }
    # Nothing changes while any of it is in use.
    foreach ($dir in $own) {
        $busy = Get-BusyFile $dir.FullName
        if ($busy) { throw "$busy 正在使用。请先退出该程序再卸载 $VariantName，未做任何改动。" }
    }
    $commands = @()
    if ($state) { $commands += @($state.commands) }
    foreach ($dir in $own) {
        try { $commands += @((Get-Commands (Read-Package $dir.FullName)).Keys) } catch { }
    }
    $active = if ($state) { Join-Path $variantDir $state.version } else { '' }
    $failed = @()
    # The installed version goes last: if it cannot be removed, it still works.
    foreach ($dir in @($own | Sort-Object { $_.FullName -eq $active })) {
        try { Remove-OwnDirectory $dir.FullName }
        catch { $failed += "$($dir.FullName)：$($_.Exception.Message)" }
    }
    if ($active -and (Test-Path -LiteralPath $active)) {
        throw "无法删除 $active，$VariantName 仍保持安装：$($failed -join '; ')"
    }
    Remove-Shims $VariantName $commands
    $stateFile = Join-Path $variantDir 'installed.json'
    if (Test-Path -LiteralPath $stateFile) { Remove-Item -LiteralPath $stateFile }
    if ($failed) { Write-Warning "以下残留未能删除，可稍后手动删除或再次卸载：$($failed -join '; ')" }
    if (Get-ChildItem -LiteralPath $variantDir -Force) {
        Write-Host "保留了 $variantDir 中的其余内容。"
    } else {
        Remove-Item -LiteralPath $variantDir
    }
    Write-Host "已卸载 $VariantName。"
    $remaining = @($catalog.Keys | Where-Object { Read-State $_ })
    if (-not $remaining) {
        Remove-BinFromUserPath
        if ((Test-Path -LiteralPath $binDir) -and -not (Get-ChildItem -LiteralPath $binDir -Force)) { Remove-Item -LiteralPath $binDir }
        if ((Test-Path -LiteralPath $InstallDir) -and -not (Get-ChildItem -LiteralPath $InstallDir -Force)) { Remove-Item -LiteralPath $InstallDir }
    }
}

function Read-Choice([object[]]$Releases) {
    $options = @()
    foreach ($name in $catalog.Keys) {
        $matching = @($Releases | Where-Object { $_.Variant -eq $name })
        if ($matching) {
            # With -Version, offer only the variants that have it.
            try { $options += Select-Release $Releases $name } catch { continue }
        }
    }
    if (-not $options) { throw '没有可安装的发布。' }
    if ([Console]::IsInputRedirected) { throw '请用 -Variant 指定要安装的版本（codex、copilot、standalone）。' }
    Write-Host 'Magicodex 可安装的版本：'
    for ($i = 0; $i -lt $options.Count; $i++) {
        $release = $options[$i]
        $flag = if ($release.Prerelease) { '（预发布）' } else { '' }
        Write-Host ('  [{0}] {1,-11} {2}{3}  {4}' -f ($i + 1), $release.Variant, $release.Version, $flag, $catalog[$release.Variant].Title)
        Write-Host ('      {0}' -f $catalog[$release.Variant].Detail)
    }
    $answer = Read-Host "输入编号（1-$($options.Count)），直接回车取消"
    $index = 0
    if (-not [int]::TryParse($answer, [ref]$index) -or $index -lt 1 -or $index -gt $options.Count) { return $null }
    return $options[$index - 1]
}

if ($Uninstall) {
    if (-not $Variant) { throw '请用 -Variant 指定要卸载的版本（codex、copilot、standalone）。' }
    Uninstall-Variant $Variant
    return
}

$releases = @(Get-Releases)
if ($List) {
    Show-Releases $releases
    Show-Installed
    return
}
if ($Variant) {
    $choice = Select-Release $releases $Variant
} else {
    $choice = Read-Choice $releases
    if (-not $choice) { Write-Host '已取消。'; return }
}
Install-Release $choice
