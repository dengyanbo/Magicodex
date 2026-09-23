#Requires -Version 7
<#
.SYNOPSIS
Builds the Magicodex release packages; -Publish creates the GitHub releases.

.DESCRIPTION
Each variant is its own GitHub release, so install.ps1 can offer them side by side:

  codex       codex-v<version>       the native OpenAI Codex CLI 0.153.4 patch
  copilot     copilot-v<version>     magicopilot, the GitHub Copilot CLI wrapper
  standalone  standalone-v<version>  the original standalone frontend

Every release gets dist\<tag>\ with the package zip, SHA256SUMS.txt, install.ps1 and the
release notes. Nothing is uploaded without -Publish, which also requires a clean, pushed HEAD
because the tags point at it.

.EXAMPLE
scripts\New-Release.ps1 -Variant copilot
scripts\New-Release.ps1 -Publish
#>
param(
    [ValidateSet('codex', 'copilot', 'standalone')]
    [string[]]$Variant = @('codex', 'copilot', 'standalone'),
    [string]$OutDir,
    # The official @openai/codex-win32-x64 0.153.4 directory ...\vendor\x86_64-pc-windows-msvc.
    [string]$CodexPackage,
    [switch]$NoBuild,
    [switch]$Publish,
    [string]$Repo = 'dengyanbo/Magicodex'
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$project = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $project 'dist' }
$installer = Join-Path $project 'install.ps1'

# The Codex release follows the native-patch series (native-patch\0001-0005).
$codexVersion = '0.5.0'
$codexUpstream = '0.153.4'

function Get-CargoVersion([string]$Manifest) {
    foreach ($line in Get-Content -LiteralPath $Manifest) {
        if ($line -match '^version\s*=\s*"([^"]+)"') { return $Matches[1] }
    }
    throw "No version in $Manifest"
}

function Write-Text([string]$Path, [string]$Text, [switch]$Crlf) {
    $Text = $Text.Replace("`r`n", "`n")
    if ($Crlf) { $Text = $Text.Replace("`n", "`r`n") }
    New-Item -ItemType Directory -Force -Path (Split-Path $Path) | Out-Null
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}

function Copy-File([string]$Source, [string]$Destination) {
    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) { throw "Missing $Source" }
    New-Item -ItemType Directory -Force -Path (Split-Path $Destination) | Out-Null
    Copy-Item -LiteralPath $Source -Destination $Destination
}

function Assert-Signature([string]$Path, [string]$Publisher) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike "*$Publisher*") {
        throw "$Path is not validly signed by $Publisher ($($signature.Status))"
    }
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

# Package READMEs travel without the repository's images.
function Get-PackageReadme([string]$Path) {
    $lines = (Get-Content -Raw -Encoding utf8 -LiteralPath $Path).Replace("`r`n", "`n") -split "`n"
    ($lines | Where-Object { $_ -notmatch '^\s*!\[' }) -join "`n"
}

function Write-Notices([string]$Manifest, [string]$Output, [string]$Title, [string]$Target, [string[]]$Extra) {
    $arguments = @((Join-Path $PSScriptRoot 'Write-ThirdPartyNotices.py'), $Manifest, $Output, '--title', $Title, '--target', $Target)
    foreach ($part in $Extra) { $arguments += @('--extra', $part) }
    & python @arguments
    if ($LASTEXITCODE -ne 0) { throw "Could not write $Output" }
}

function Write-PackageManifest([string]$Stage, [System.Collections.IDictionary]$Info) {
    $Info['commit'] = $commit
    $Info['source'] = "https://github.com/$Repo"
    Write-Text (Join-Path $Stage 'magicodex-package.json') (($Info | ConvertTo-Json -Depth 4) + "`n")
}

function Resolve-CodexVendor {
    $candidates = @()
    if ($CodexPackage) { $candidates += $CodexPackage }
    # The Codex install behind the local bridge, as scripts\Publish-Native.ps1 finds it.
    $lookup = @'
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {dirname,join} from 'node:path';
import {homedir} from 'node:os';
try {
  const settings=JSON.parse(readFileSync(join(homedir(),'.codex','copilot-proxy','settings.json'),'utf8'));
  const require=createRequire(settings.codexEntry);
  console.log(join(dirname(require.resolve('@openai/codex-win32-x64/package.json')),'vendor','x86_64-pc-windows-msvc'));
} catch {}
'@
    $fromBridge = $lookup | node --input-type=module
    if ($fromBridge) { $candidates += $fromBridge.Trim() }
    $npmRoot = & npm root -g 2>$null
    if ($npmRoot) {
        $candidates += Join-Path $npmRoot.Trim() '@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc'
    }
    foreach ($candidate in $candidates) {
        $manifest = Join-Path $candidate 'codex-package.json'
        if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { continue }
        $info = Get-Content -Raw -LiteralPath $manifest | ConvertFrom-Json
        if ($info.version -eq $codexUpstream -and $info.target -eq 'x86_64-pc-windows-msvc') {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw "The official @openai/codex-win32-x64 $codexUpstream package was not found; pass -CodexPackage <...\vendor\x86_64-pc-windows-msvc>."
}

function New-CodexPackage([string]$Stage) {
    $binary = Join-Path $project 'native\codex.exe'
    $reported = (& $binary --version | Out-String).Trim()
    if ($reported -ne "codex-cli $codexUpstream") {
        throw "native\codex.exe reports '$reported'; build and publish the $codexUpstream patch first (scripts\Build-Native.ps1, scripts\Publish-Native.ps1)."
    }
    $vendor = Resolve-CodexVendor
    Copy-File (Join-Path $vendor 'codex-package.json') (Join-Path $Stage 'codex-package.json')
    Copy-File $binary (Join-Path $Stage 'bin\codex.exe')
    foreach ($relative in 'bin\codex-code-mode-host.exe', 'codex-resources\codex-command-runner.exe', 'codex-resources\codex-windows-sandbox-setup.exe') {
        $source = Join-Path $vendor $relative
        Assert-Signature $source 'OpenAI'
        Copy-File $source (Join-Path $Stage $relative)
    }
    Copy-File (Join-Path $vendor 'codex-path\rg.exe') (Join-Path $Stage 'codex-path\rg.exe')
    $published = Join-Path $project 'native\codex-code-mode-host.exe'
    if ((Test-Path -LiteralPath $published) -and (Get-Sha256 $published) -ne (Get-Sha256 (Join-Path $Stage 'bin\codex-code-mode-host.exe'))) {
        throw 'native\codex-code-mode-host.exe differs from the official package copy.'
    }
    Copy-File (Join-Path $project 'scripts\Start-NativeBridge.mjs') (Join-Path $Stage 'scripts\Start-NativeBridge.mjs')
    foreach ($patch in Get-ChildItem -LiteralPath (Join-Path $project 'native-patch') -File) {
        Copy-File $patch.FullName (Join-Path $Stage "patches\$($patch.Name)")
    }
    $upstream = Join-Path $project 'upstream\codex-rust-v0.153.4'
    Copy-File (Join-Path $upstream 'LICENSE') (Join-Path $Stage 'LICENSE')
    Copy-File (Join-Path $upstream 'NOTICE') (Join-Path $Stage 'NOTICE')
    Copy-File (Join-Path $project 'packaging\codex-NOTICE.md') (Join-Path $Stage 'MAGICODEX-NOTICE.md')
    Write-Text (Join-Path $Stage 'README.md') (Get-PackageReadme (Join-Path $project 'packaging\codex-README.md'))
    Write-Text (Join-Path $Stage 'magicodex.cmd') @'
@echo off
rem Magicodex: OpenAI Codex CLI 0.153.4 with the magic circle, using your own Codex sign-in.
rem The update prompt is off: it would install a separate, unpatched Codex.
"%~dp0bin\codex.exe" -c check_for_update_on_startup=false %*
'@ -Crlf
    Write-Text (Join-Path $Stage 'magicodex-bridge.cmd') @'
@echo off
rem Magicodex through the local GitHub Copilot bridge in %USERPROFILE%\.codex\copilot-proxy.
node "%~dp0scripts\Start-NativeBridge.mjs" %*
'@ -Crlf
    Write-PackageManifest $Stage ([ordered]@{
        name = 'magicodex-codex'
        variant = 'codex'
        version = $codexVersion
        description = "OpenAI Codex CLI $codexUpstream with the Magicodex magic circle (native TUI patch)"
        requires = 'A Codex sign-in (ChatGPT or API key); magicodex-bridge also needs the local copilot-proxy bridge and Node.js'
        commands = [ordered]@{ 'magicodex' = 'magicodex.cmd'; 'magicodex-bridge' = 'magicodex-bridge.cmd' }
    })
    & (Join-Path $Stage 'bin\codex.exe') --version | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'The packaged codex.exe did not start' }
}

function New-CopilotPackage([string]$Stage, [string]$Version) {
    if (-not $NoBuild) {
        & (Join-Path $PSScriptRoot 'Build-Copilot.ps1') -Action Build
    }
    $release = Join-Path $project 'copilot\target\release'
    Copy-File (Join-Path $release 'magicopilot.exe') (Join-Path $Stage 'magicopilot.exe')
    foreach ($name in 'conpty.dll', 'OpenConsole.exe') {
        $source = Join-Path $release $name
        Assert-Signature $source 'Microsoft Corporation'
        Copy-File $source (Join-Path $Stage $name)
    }
    $reported = (& (Join-Path $Stage 'magicopilot.exe') --magic-version | Out-String).Trim()
    if ($reported -ne "magicopilot $Version") { throw "magicopilot.exe reports '$reported', expected $Version" }
    Copy-File (Join-Path $project 'LICENSE') (Join-Path $Stage 'LICENSE')
    Write-Text (Join-Path $Stage 'README.md') (Get-PackageReadme (Join-Path $project 'copilot\README.md'))
    Write-Notices (Join-Path $project 'copilot\Cargo.toml') (Join-Path $Stage 'THIRD-PARTY-NOTICES.md') `
        'magicopilot third-party notices' 'x86_64-pc-windows-msvc' @((Join-Path $project 'packaging\conpty-NOTICE.md'))
    Write-PackageManifest $Stage ([ordered]@{
        name = 'magicopilot'
        variant = 'copilot'
        version = $Version
        description = 'GitHub Copilot CLI with the Magicodex magic circle (wrapper around the installed, unmodified Copilot CLI)'
        requires = 'GitHub Copilot CLI installed and signed in (tested with 1.0.87); Windows 10 1809 or later'
        commands = [ordered]@{ 'magicopilot' = 'magicopilot.exe' }
    })
}

function New-StandalonePackage([string]$Stage, [string]$Version) {
    $binary = Join-Path $project 'magicodex.exe'
    $built = Join-Path $project 'target\release\magicodex.exe'
    if ((Test-Path -LiteralPath $built) -and (Get-Sha256 $built) -ne (Get-Sha256 $binary)) {
        throw 'magicodex.exe differs from target\release\magicodex.exe; run scripts\Build.ps1 -Task release.'
    }
    Copy-File $binary (Join-Path $Stage 'magicodex.exe')
    Copy-File (Join-Path $project 'LICENSE') (Join-Path $Stage 'LICENSE')
    Write-Text (Join-Path $Stage 'README.md') (Get-PackageReadme (Join-Path $project 'README-standalone.md'))
    # The standalone frontend is built with the GNU toolchain (scripts\Build.ps1).
    Write-Notices (Join-Path $project 'Cargo.toml') (Join-Path $Stage 'THIRD-PARTY-NOTICES.md') `
        'Magicodex standalone third-party notices' 'x86_64-pc-windows-gnu' @()
    Write-PackageManifest $Stage ([ordered]@{
        name = 'magicodex-standalone'
        variant = 'standalone'
        version = $Version
        description = 'The original standalone Magicodex frontend for Codex app-server'
        requires = 'Codex CLI on PATH (codex, or codex-original with --backend official)'
        commands = [ordered]@{ 'magicodex-standalone' = 'magicodex.exe' }
    })
}

function Get-FileTable([string]$Stage) {
    $rows = foreach ($file in Get-ChildItem -LiteralPath $Stage -Recurse -File | Where-Object { $_.Extension -in '.exe', '.dll' } | Sort-Object FullName) {
        $relative = $file.FullName.Substring($Stage.Length + 1)
        '| `{0}` | {1:N1} MiB | `{2}` |' -f $relative, ($file.Length / 1MB), (Get-Sha256 $file.FullName)
    }
    (@('| 文件 | 大小 | SHA256 |', '| --- | --- | --- |') + $rows) -join "`n"
}

function Get-InstallSnippet([string]$Tag, [string]$VariantName) {
    @"
``````powershell
gh release download $Tag --repo $Repo --pattern install.ps1
powershell -ExecutionPolicy Bypass -File .\install.ps1 -Variant $VariantName
``````

``install.ps1`` 下载本发布的 zip，按 ``SHA256SUMS.txt`` 校验后安装到 ``%LOCALAPPDATA%\Magicodex``，命令入口在 ``%LOCALAPPDATA%\Magicodex\bin``（加 ``-AddToPath`` 才会写入用户 PATH）。不带参数运行可交互选择版本；``-List`` 列出所有发布，``-Uninstall -Variant $VariantName`` 卸载。也可以直接解压 zip 使用。
"@
}

$commit = (& git -C $project rev-parse HEAD | Out-String).Trim()
$status = (& git -C $project status --porcelain | Out-String).Trim()
$dirty = $status.Length -gt 0
if ($Publish) {
    if ($dirty) { throw 'Commit every change before publishing: the release tags point at HEAD.' }
    $upstreamHead = (& git -C $project rev-parse '@{u}' | Out-String).Trim()
    if ($upstreamHead -ne $commit) { throw 'Push HEAD before publishing: the release tags point at it.' }
    & gh auth status *> $null
    if ($LASTEXITCODE -ne 0) { throw 'gh is not signed in (gh auth login).' }
}
if ($dirty) {
    Write-Warning "The working tree has uncommitted changes; packages record $commit-dirty."
    $commit = "$commit-dirty"
}

$versions = @{
    codex = $codexVersion
    copilot = Get-CargoVersion (Join-Path $project 'copilot\Cargo.toml')
    standalone = Get-CargoVersion (Join-Path $project 'Cargo.toml')
}
$definitions = @{
    codex = @{
        Tag = "codex-v$($versions.codex)"
        Asset = "magicodex-codex-$($versions.codex)-windows-x64.zip"
        Title = "Magicodex for Codex CLI $codexUpstream · v$($versions.codex)"
        Prerelease = $false
        Latest = $true
    }
    copilot = @{
        Tag = "copilot-v$($versions.copilot)"
        Asset = "magicopilot-$($versions.copilot)-windows-x64.zip"
        Title = "magicopilot for GitHub Copilot CLI · v$($versions.copilot)"
        Prerelease = $true
        Latest = $false
    }
    standalone = @{
        Tag = "standalone-v$($versions.standalone)"
        Asset = "magicodex-standalone-$($versions.standalone)-windows-x64.zip"
        Title = "Magicodex standalone frontend · v$($versions.standalone)"
        Prerelease = $false
        Latest = $false
    }
}

$notes = @{
    codex = @"
OpenAI Codex CLI **$codexUpstream** + Magicodex 原生魔法阵补丁（非官方）。

**适合**：使用 OpenAI Codex CLI、想在原版 Codex 界面里看到魔法阵的用户。Codex 的输入框、快捷键、命令、默认提示、审批与会话保持原样；在输入框里输入 ``/magic on`` 或 ``/magic list`` 使用。

- 10 种法阵：classic 经典、wind 风、fire 火、water 水、thunder 雷、earth 土、holy 神圣、dark 黑暗、eerie 诡异、tech 科技，外形、运动和文字路径各不相同；``/magic list`` 实时预览选择。
- 输入前是小法阵，提交后随等待逐层变大变复杂；prompt 环绕外圈、中间回复环绕内圈（不含 reasoning）；最终回复从法阵下方吐出。

## 安装

$(Get-InstallSnippet "codex-v$($versions.codex)" 'codex')

安装后运行 ``magicodex``（使用你自己的 Codex 登录，首次可先 ``magicodex login``）。``magicodex-bridge`` 只适用于已有本地 ``~/.codex/copilot-proxy`` 桥接的机器。

## 要求

- Windows 10/11 x64
- Codex 登录（ChatGPT 账号或 API key）；不需要另外安装官方 Codex。与官方 Codex 共用 ``~/.codex``：若它已被更新版本的 Codex 使用过，请为补丁版单独设置 ``CODEX_HOME``。

## 包内容

``bin\codex.exe`` 由 ``rust-v$codexUpstream`` 源码应用 ``patches\0001``–``0005`` 构建；``codex-code-mode-host.exe``、``codex-resources\``、``codex-path\rg.exe`` 与 ``codex-package.json`` 是官方 ``@openai/codex-win32-x64`` $codexUpstream 原文件（OpenAI 签名，未修改），目录布局与官方包相同。许可：Apache-2.0（``LICENSE``、``NOTICE``、``MAGICODEX-NOTICE.md``）。

## 验证

本地 fixture，未调用真实模型：原生 TUI 测试 4108 通过 / 10 跳过，魔法阵定向用例 41 项；ConPTY 端到端在通用与 Windows Terminal 两种滚动策略下通过（``/magic list`` 预览 / Esc 恢复 / Enter 选用、fire 出口、``/magic 雷``、默认 instructions 与开关无关）。本发布包按官方目录布局解压后再次运行了同一端到端验收。

__FILES__
"@
    copilot = @"
**magicopilot**：GitHub Copilot CLI 的魔法阵外壳（预发布）。

**适合**：使用 GitHub Copilot CLI 的用户。magicopilot 运行你已安装、**未修改**的 ``copilot``，在它上方画魔法阵；Copilot 的界面、快捷键、斜杠命令、默认 prompt、会话、认证与计费都不变。用 ``magicopilot`` 代替 ``copilot`` 启动，其余参数原样传给 copilot。

- 输入前是小法阵；提交后法阵变大，并随等待时间越来越大、越来越复杂，直到第一次回复。
- prompt 与中间回复（不含 reasoning）环绕法阵；最终回复时法阵定格、光从阵心向下释放，Copilot 的回答出现在法阵下方，随后法阵缩回待机大小。
- 在 Copilot 输入框里输入 ``/magic list``（预览选择 10 种法阵）、``/magic on``、``/magic off``、``/magic 火``；这些命令由外壳截获，不会发给模型。

## 安装

$(Get-InstallSnippet "copilot-v$($versions.copilot)" 'copilot')

## 要求

- Windows 10 1809 或更高版本，x64；推荐 Windows Terminal
- 已安装并登录 GitHub Copilot CLI（验证于 1.0.87）

## 工作方式与限制

伪终端里运行 copilot，vt100 解析其界面后与法阵区域合成；法阵内容来自 Copilot 实时写入的 ``~/.copilot/session-state/<会话>/events.jsonl``。随包的 ``conpty.dll`` / ``OpenConsole.exe`` 是微软官方 NuGet 包 Microsoft.Windows.Console.ConPTY 1.24.260710001 的原文件（MIT，微软签名），与 Windows Terminal 使用的伪终端相同，使 Copilot 的界面与直接运行时一致。法阵占用顶部 5–24 行（Copilot 至少保留 14 行，窗口低于 19 行时法阵隐藏）；超链接与终端图片不转发；设置不持久化。magicopilot 不包含、不修改、不再分发 Copilot CLI。

## 验证

真实 Copilot CLI 1.0.87 + 本地假模型服务（BYOK 离线，不消耗额度），在 Windows Terminal 与通用两种模式下：待机 5 行 → 施法 21 行且点阵随时间变大 → prompt 与中间回复环绕 → 出口 24 行 → 回到待机；``/magic off``、``/magic 火``、``/magic list`` 的预览 / Esc / 数字选择，命令后多打空格或光标移回命令中间时输入框同样清空，窗口太矮时 ``/magic list`` 只提示不接管按键；鼠标点击 Copilot 标签页；``/exit`` 恢复终端并保留 Copilot 的退出摘要（含 ``--resume=``）；``--continue`` 恢复会话与 ``/clear`` 新会话后法阵继续跟随；与直接运行 Copilot 相比，发给模型的系统提示与工具列表一致、输入框样式一致；35 项单元测试（含真实批处理文件的参数转义）。**预发布**：尚未在人工操作的真实 Windows Terminal 窗口中验收。

__FILES__
"@
    standalone = @"
Magicodex **独立前端** $($versions.standalone)（最早的版本，保留）。

**适合**：想要一个完全自绘的魔法阵客户端的用户。它通过 Codex app-server 连接 Codex，界面（输入框、审批、日志视图）是 Magicodex 自己实现的，不是 Codex 原版界面；公开 reasoning、工具执行与中间回复都会显示在法阵中，F3 查看原文，F4 阅读正文。只有一种法阵样式。若想要原版 Codex 界面，请选择 ``codex`` 版本。

## 安装

$(Get-InstallSnippet "standalone-v$($versions.standalone)" 'standalone')

安装后运行 ``magicodex-standalone --demo`` 离线演示；真实对话需要 PATH 中的 ``codex``（或 ``--backend official`` 时的 ``codex-original``），详见包内 README。

## 要求

- Windows 10/11 x64、Windows Terminal；已安装并配置 Codex CLI

__FILES__
"@
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
foreach ($name in $Variant) {
    $definition = $definitions[$name]
    $releaseDir = Join-Path $OutDir $definition.Tag
    $stage = Join-Path $releaseDir 'package'
    if (Test-Path -LiteralPath $releaseDir) { Remove-Item -Recurse -Force -LiteralPath $releaseDir }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Write-Host "== $($definition.Tag)"
    switch ($name) {
        'codex' { New-CodexPackage $stage }
        'copilot' { New-CopilotPackage $stage $versions.copilot }
        'standalone' { New-StandalonePackage $stage $versions.standalone }
    }
    $zip = Join-Path $releaseDir $definition.Asset
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip, [IO.Compression.CompressionLevel]::Optimal, $false)
    Copy-File $installer (Join-Path $releaseDir 'install.ps1')
    $sums = foreach ($file in $zip, (Join-Path $releaseDir 'install.ps1')) { '{0}  {1}' -f (Get-Sha256 $file), (Split-Path $file -Leaf) }
    Write-Text (Join-Path $releaseDir 'SHA256SUMS.txt') (($sums -join "`n") + "`n")
    $body = $notes[$name].Replace('__FILES__', "## 文件`n`n$(Get-FileTable $stage)`n`nzip SHA256：``$(Get-Sha256 $zip)``（亦见 ``SHA256SUMS.txt``）。源码提交：``$commit``。")
    $notesFile = Join-Path $releaseDir 'notes.md'
    Write-Text $notesFile $body
    '{0}  {1:N1} MiB' -f $zip, ((Get-Item -LiteralPath $zip).Length / 1MB) | Write-Host

    if ($Publish) {
        & gh release view $definition.Tag --repo $Repo --json tagName *> $null
        if ($LASTEXITCODE -eq 0) { throw "Release $($definition.Tag) already exists." }
        $arguments = @('release', 'create', $definition.Tag, $zip, (Join-Path $releaseDir 'SHA256SUMS.txt'), (Join-Path $releaseDir 'install.ps1'),
            '--repo', $Repo, '--target', $commit, '--title', $definition.Title, '--notes-file', $notesFile)
        if ($definition.Prerelease) { $arguments += '--prerelease' }
        $arguments += $(if ($definition.Latest) { '--latest' } else { '--latest=false' })
        & gh @arguments
        if ($LASTEXITCODE -ne 0) { throw "gh release create $($definition.Tag) failed" }
    }
}
