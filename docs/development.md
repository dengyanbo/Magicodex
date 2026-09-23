# Magicodex 开发文档

使用说明见 [README](../README.md)。本文面向想从源码构建、修改或审查 Magicodex 的开发者，记录各版本的实现边界、构建与发布方法，以及验证结果。

## 目录结构

| 路径 | 内容 |
| --- | --- |
| `copilot\` | magicopilot：GitHub Copilot CLI 外壳（Rust） |
| `upstream\codex-rust-v0.153.4\` | 与 Codex 发布标签对应的源码，已应用 Magicodex 补丁 |
| `native-patch\` | 相对上游源码的补丁 0001–0005 与 `manifest.json` |
| `native\` | 本机构建并发布的补丁版 `codex.exe`（不在 Git 中） |
| `src\`、`Cargo.toml` | 最早的独立前端（standalone），说明见 [README-standalone.md](../README-standalone.md) |
| `scripts\` | 构建、导出补丁、打包与发布脚本 |
| `tests\` | 端到端验收与画面渲染脚本 |
| `packaging\` | 发布包内的说明与许可声明 |
| `install.ps1` | 发布版安装脚本 |
| `magicodex-native.ps1`、`magicodex-native-official.ps1` | 从源码目录启动补丁版 Codex |
| `docs\images\` | README 图片 |

## Git 仓库范围

仓库包含项目源码（根目录独立前端、`copilot\` 外壳）、修改后的上游源码、补丁、打包与构建脚本，保留上游许可证与声明；不提交 EXE、依赖缓存、源码 ZIP、用户配置或数据库。克隆后需按下文构建，Git 仓库本身不包含本地已生成的二进制。许可证范围见 `LICENSE` 和 `NOTICE`。

## 发布

每个版本是一个独立的 GitHub Release：`codex-v*`、`copilot-v*`、`standalone-v*`。每个 Release 附 zip、`SHA256SUMS.txt` 和同一份 `install.ps1`。

```powershell
pwsh -File scripts\New-Release.ps1                    # 在 dist\<标签>\ 生成三个版本的包与发布说明，不上传
pwsh -File scripts\New-Release.ps1 -Variant copilot   # 只生成一个版本
pwsh -File scripts\New-Release.ps1 -Publish           # 生成并创建 GitHub Release
```

- `-Publish` 要求工作区干净、HEAD 已推送、`gh` 已登录，因为标签指向 HEAD；同名 Release 已存在时停止。
- codex 版本为 Latest；copilot 版本是预发布，不标为 Latest。
- 版本号：codex 版本写在 `New-Release.ps1` 的 `$codexVersion`（对应补丁系列），copilot 与 standalone 取各自 `Cargo.toml`。
- 每个包带有 `magicodex-package.json`（版本类型、版本号、源码提交、命令列表），安装器据此创建命令入口，并以此识别自己安装的目录。

包内容：

- **codex**：沿用官方 npm 包 `@openai/codex-win32-x64` 的目录布局（`bin\`、`codex-resources\`、`codex-path\`、`codex-package.json`），只替换 `bin\codex.exe`，因此沙箱辅助程序和内置 ripgrep 照常可用。其余文件从本机官方 0.153.4 安装复制，并校验 OpenAI 签名。
- **copilot**：`magicopilot.exe`，加上微软官方 NuGet 包 Microsoft.Windows.Console.ConPTY 1.24.260710001 中的 `conpty.dll`、`OpenConsole.exe`（MIT，微软签名，未修改；构建脚本固定其 SHA256）。
- **standalone**：`magicodex.exe`。
- 每个包都附第三方许可 `THIRD-PARTY-NOTICES.md`，由 `scripts\Write-ThirdPartyNotices.py` 根据 `cargo metadata` 生成。

### 安装器

`install.ps1` 支持 Windows PowerShell 5.1 与 PowerShell 7，行为如下：

- 仓库为私有时，通过 `gh api` 列出发布、用 `gh release download` 下载；公开仓库时可走 REST；`-Source <目录>` 从本地 zip 安装，不联网。
- 先按 `SHA256SUMS.txt` 校验 zip，再解压到 `<安装目录>\<类型>\<版本>.partial`，核对包内的 `magicodex-package.json` 后才移到位；`-Force` 重装时，新包就绪后才替换旧的。命令入口是 `<安装目录>\bin\*.cmd`，按相对路径指向当前版本，路径中的非 ASCII 字符不会经过 cmd.exe 的代码页。
- 只删除自己创建的目录，即带有该类型 `magicodex-package.json` 的目录，或 `.old-xxxxxxxx` 残留。删除或替换前，逐个文件以读写方式试开，检查目录是否在用（运行中的 EXE/DLL、被占用的文件）；在用时，升级保留旧版本，`-Force` 与卸载则拒绝且不做改动。这是因为 Windows 允许重命名运行中程序所在的目录，先改名再删会把目录删到只剩 EXE。
- 相对 `-InstallDir` 以当前 PowerShell 位置为准。PATH 只在 `-AddToPath` 时修改，并保留 `REG_EXPAND_SZ`。测试时可用环境变量 `MAGICODEX_INSTALL_ENV_KEY` 把 PATH 的注册表键换成 HKCU 下的测试键。

## Codex 原生补丁

基于官方 **Codex 0.153.4** 的非官方 UI 补丁。它沿用 Codex 自己的输入框、快捷键、命令菜单、历史记录、审批和默认提示；魔法阵不是另一个仿 Codex 的终端界面。

### 从源码运行

先完成下文的构建和发布（`native\codex.exe`），再在 Windows Terminal 中运行：

```powershell
# 沿用原有 Copilot 桥接与 profile（对应发布版的 magicodex-bridge）
.\magicodex-native.ps1

# 原版官方入口与配置；参数原样交给 Codex（对应发布版的 magicodex）
.\magicodex-native-official.ps1 --model gpt-5.5
```

### 后端与安装边界

`scripts\Start-NativeBridge.mjs` 复用现有桥接的 SDK、模型目录、profile 和生命周期代码，只把实际运行的 Codex 换成补丁版。模型目录模板读取自原安装，避免悄悄替换其默认指令。发布包里的补丁版位于 `bin\codex.exe`，源码目录里的位于 `native\codex.exe`，脚本两处都会查找。

桥接原有的后端限制仍然存在；原生 UI 不会把一个原本不支持的桥接操作伪装成成功。官方入口不通过桥接，也不会自动切换账号或模型。

原生补丁锁定 0.153.4。升级原安装后，应在对应源码版本重新移植，而不是混用不同版本的模型模板和运行组件。

补丁入口在当前进程关闭原版的启动更新弹窗：该弹窗会调用上游安装器，无法更新这份 UI 补丁，还可能安装另一份未打补丁的 Codex。补丁升级由本项目构建和发布管理。这是发行管理上的限制，不修改原始配置文件、系统提示词或快捷键。系统中原来的 Codex 入口仍保留其原有更新行为。

原生补丁的发布包含 Codex 本体及同版本的原始运行组件，体积远大于约 1.1 MiB 的独立前端：当前发布的补丁版主程序约 283.76 MiB，未修改的 code-mode host 约 69.12 MiB，发布 zip 约 137 MiB。这是磁盘体积，不代表运行内存。选择原生补丁，是用更大的构建和分发体积换取真实的原版交互兼容。

### 源码与补丁

| 路径 | 用途 |
| --- | --- |
| `upstream\codex-rust-v0.153.4` | 匹配发布标签的源码 |
| `native\codex.exe` | 构建并发布后的补丁版主程序 |
| `native\codex-code-mode-host.exe` | 同版本、未修改的官方运行组件 |
| `native-patch\0001-release-lock-alignment.patch` | 发布标签的内部包版本与 lockfile 对齐 |
| `native-patch\0002-native-magic-tui.patch` | 原生 TUI 魔法阵补丁 |
| `native-patch\0003-downward-reply.patch` | 法阵上方定型、文字向下输出及流式预览 |
| `native-patch\0004-visual-refresh.patch` | 法阵视觉重绘：分层六芒星、双文字带、流光与描线动画、光锥出口、流式高亮 |
| `native-patch\0005-magic-styles.patch` | 10 种法阵类型、`/magic list` 预览选择弹窗与 `/magic <类型>` |
| `native-patch\manifest.json` | 上游版本、改动文件与受保护源码校验 |

重新导出补丁需要原始源码 ZIP（不在 Git 中）：

```powershell
Invoke-WebRequest 'https://codeload.github.com/openai/codex/zip/refs/tags/rust-v0.153.4' -OutFile .\upstream\codex-rust-v0.153.4.zip
```

`scripts\Export-NativePatch.py` 的参数：

- `--base-ref <提交>`：沿用该提交中已有的补丁阶段；
- `--keep-stage <补丁>`：原样保留该提交之后尚未提交的阶段，按顺序可重复；
- `--stage-name`：接收其余改动的新阶段文件名。

它同时核对原提示、输入与键位相关文件未被改动。0001–0005 目前都已提交（0005 于 5c43a18）。新的改动可以用 `--base-ref HEAD --stage-name 0006-<名称>.patch` 导出为下一个阶段。0005 当时是这样导出的：

```powershell
python .\scripts\Export-NativePatch.py --archive .\upstream\codex-rust-v0.153.4.zip --base-ref 11cbcbc250297f8b50ea94ac6851b11170c4de9d --keep-stage .\native-patch\0003-downward-reply.patch --keep-stage .\native-patch\0004-visual-refresh.patch --stage-name 0005-magic-styles.patch
```

上游标签的 manifest 是 0.153.4，但锁文件中的 149 个工作区包仍标记为 0.0.0。准备构建时使用 `cargo update --workspace --offline` 对齐，1232 条外部依赖记录保持不变，并运行了上游要求的 Bazel 锁更新。

第一阶段补丁还包含发布版快照对齐：上游快照原先按开发版 `0.0.0` 绘制版本横幅、填充空格和更新提示，本次构建实际为 `0.153.4`。这些差异与魔法阵补丁分开保存，没有通过改动运行时版本或原版提示来迁就快照。

**Windows 数据库兼容：** 官方 Windows 构建的 SQLx 迁移校验基于 CRLF，而 GitHub 源归档为 LF。构建脚本会把 `state` 迁移文件的换行规范为 CRLF（SQL 内容不变），避免已有数据库被误报“迁移被修改”。没有重写、删除或重建用户数据库；可用 `python .\scripts\Prepare-NativeWindows.py --check-database` 只读核对已安装的迁移校验。

补丁只在 TUI 增加渲染模块、命令及少量生命周期挂钩，不修改 core、protocol、模型提示模板、原输入组件或键位实现。

### 构建与测试

本机开发工具为 Rust 1.95.0 MSVC、Visual Studio Build Tools、just、cargo-nextest、cargo-insta。构建脚本只调整当前进程环境，不修改系统 PATH。

```powershell
.\scripts\Build-Native.ps1 -Action Build
.\scripts\Build-Native.ps1 -Action Test -Filter 'test(magic)'
.\scripts\Build-Native.ps1 -Action Test
.\scripts\Build-Native.ps1 -Action Fix
.\scripts\Build-Native.ps1 -Action Format
.\scripts\Publish-Native.ps1
```

原生测试按上游规则通过 `just test` / nextest 运行。UI 变化使用 insta snapshots；默认关闭效果时，原组件的既有快照不应因魔法阵而改变。

初次开发时使用无 Git 索引的发布归档，上游 `just fmt` 因无法枚举 Bazel 文件而未能运行，当时只对改动的 Rust crate 执行了格式化。现在源码已有 Git 索引，运行上游格式化并补齐缺失的 `dotslash` 后，完整 `just fmt-check` 已通过；克隆后也需要准备 `dotslash`、`uv` 等上游格式化依赖。本机归档的 Rust 格式化替代命令：`.\scripts\Build-Native.ps1 -Action FormatRust`。

`tests\native_terminal.py` 用本地 Responses fixture 驱动真实原生 CLI，不消耗模型额度。它检查：

- 本地命令、图形增长、开关、默认指令一致性及原退出键；
- `/magic list` 弹窗的预览、Esc 恢复与 Enter 选用，以及非 classic 类型的出口位置；
- 向下吐字：暂停后续响应，先确认没有换行的首段已经出现在出口下方，再继续发送剩余正文；
- 流中开关、缩放，以及最终正文没有重复。

默认不设置 `WT_SESSION`，走 Codex 的通用滚动策略；加 `--windows-terminal` 则按 Windows Terminal 的策略运行。读屏前会等待输出静默，避免把半帧重绘误判为结果。

`tests\render_frames.py` 用同样的本地 fixture，把待机、描线、蓄力、中间回复、吐字、完成各阶段渲染为 PNG，便于审阅视觉改动（需要 Pillow，运行方式见文件开头）；`--style <类型>` 选择要渲染的法阵类型。README 中的 Codex 法阵图片由 `tests\render_frames.py --style <类型> --windows-terminal` 驱动真实补丁程序、按终端缓冲区渲染（本地 fixture，Campbell 配色近似），不是屏幕截图；Copilot 版的图片由 `tests\copilot_terminal.py --frames` 按终端内容渲染的真实运行画面拼成。

字体效果、输入法候选窗与主观审美仍需在实际的 Windows Terminal 中确认。

### 设计细节

#### 效果与内容边界

- 输入框与正常回复保持原版样式，在法阵外面；阵心不放文本框。
- 提交新回合后，法阵立即变大，随后按等待时间扩大、增加几何层次，有终端空间上限。
- 收到本回合第一段非空助手回复后停止增长。最终正文开始时，法阵固定为上方的文字出口，正文从出口下方展开；结束后不再在正文下面另画一座小阵。
- prompt 沿外圈文字带环绕；最近的部分助手回复沿内圈文字带环绕。字符位置沿圆弧移动，不伪装成像素图像旋转。
- 法阵不采集 reasoning。Codex 原本的状态行与完整 transcript 不因补丁而删改。
- `/magic` 是本地显示命令，不调用模型，也不添加系统指令；开关提示使用 Codex 原生的 `• …` 信息样式，并注明当前类型。
- `/magic list` 使用 Codex 原生的选择弹窗列出 10 种法阵类型，可逐个预览后选用。
- 不增加 F2/F3/F4 等专属快捷键，沿用 Codex 的键位与用户既有 keymap。
- 开关与类型只影响当前应用的展示，并在当前应用的新会话、会话切换间共享，不改用户配置文件。

#### classic 法阵的构成

| 等待时间 | 新增层次（逐层“描线”绘入） |
| --- | --- |
| 待机 | 5 行高的双环星芯，静止不重绘 |
| 提交后 | 外环与 prompt 文字带；prompt 逐字刻写进文字带 |
| 2.5 秒 | 回复文字带与内接六芒星（六条边依次画出） |
| 5 秒 | 六芒星顶点光珠、内环 |
| 8 秒 | 阵心小环与六条辐条 |
| 11 秒 | 外缘副环 |

- **明暗与颜色**：
  - 主线为 Codex 的 magenta，细节为暗淡 magenta，光珠与阵心为加亮 magenta。
  - 等待期间，一道默认前景色的流光沿外环顺时针移动，阵心随节拍明灭；六芒星与文字带反向旋转。
  - 只使用 ANSI magenta、cyan（prompt，即用户输入色）与默认前景的明暗变化，遵循 Codex 样式指南，不引入 RGB、黄色或蓝色。
- **文字带**：
  - 文字在两道圆环之间，不压在线上；顶部和底部逐字紧排保持单词可读，两侧每行一个字。
  - 短 prompt 重复填满整圈，未写完的一遍保留整词，其余位置以 `✦` 补齐；过长 prompt 以 `…` 截断。
- 所有圆按盲文点阵对称取整，不出现单侧凸点。
- 终端空间不足时自动省略六芒星等内层，只保留可读的环与文字。
- Codex 的 `animations = false`：不旋转、无流光、不逐层绘入，已解锁的层直接完整显示；大小仍随等待时间增长。

#### 从法阵下方显字

最终回复的第一段文字到来时，法阵定格进入当前终端记录：

- 外环底部出现光门，向下投出逐渐展开的光锥；正文紧接在光锥下方展开，后续文本始终接在它的下方，完成时不会跳回法阵上方。
- 尚未换行的普通文本也能逐段预览。正在书写的最后几个字以 magenta 高亮，随后续文字到来而冷却，换行提交后恢复原生样式。
- 表格仍保留 Codex 的原生延迟排版；完整回复最终使用原生 Markdown 归并，避免重复文字。

法阵是纯显示层，不进入模型上下文、复制友好原文或 transcript 导出：

- `/magic off` 会重绘并隐藏历史中的法阵装饰，但不删除回复；再次开启可恢复装饰。
- 长回复随终端正常滚动，法阵可能滚出当前可见区域。

输出过程中切换开关：

- 开关立即生效，但确认文字会等当前消息结束后再写入历史，避免把正文拆断。
- 若在一条已经开始的普通回复中途才开启，出口从下一条回复开始出现，不会插进已有文字中间。
- 中断时，已显现的部分正文会保留。

#### 法阵类型

`/magic list`（或只输入 `/magic`）打开 Codex 原生的选择弹窗：

- ↑/↓ 移动高亮即实时预览：终端约 85 列及以上时，弹窗右侧显示该类型充能完成的法阵；法阵已开启时，输入框上方的待机阵或蓄力阵也同步切换。
- Enter 选用并开启法阵；数字键 1–9 按 Codex 列表的原有行为直接选用对应类型；Esc 恢复打开弹窗前的类型，开关状态不变。
- 也可以直接输入 `/magic fire`、`/magic 火` 或 `/magic FIRE`，切换并开启。无法识别的参数只显示用法，不改变当前设置。
- 类型与开关一样只在本次运行的各会话间共享，不写入配置；重新启动后恢复为关闭的 `classic`。
- 已定格在历史中的出口保留当时的类型；之后切换只影响新的法阵。

| 类型 | 外形 | 运动 | 文字 | 待机小阵 | 出口 | ANSI 配色 |
| --- | --- | --- | --- | --- | --- | --- |
| `classic` 经典 | 双文字带、内接六芒星、光珠与辐条 | 外环流光，六芒星反向旋转，逐层描线 | prompt 在外圈，回复在内圈 | 双环星芯 | 光门与光锥 | magenta、cyan |
| `wind` 风 | 3–6 条螺旋气旋臂、虚线外环、阵眼 | 顺时针疾转，气流粒子掠过 | prompt 沿螺旋卷入；最新回复在阵眼处，旧字向外展开 | 三道卷风 | 收窄的龙卷 | cyan |
| `fire` 火 | 外缘火舌、五芒星、阵心火芒 | 火舌跳动，火星上升 | 圆环排字，随热浪闪烁加粗 | 火苗 | 火柱与火星 | 红、黄 |
| `water` 水 | 波浪外缘与内缘、池心涟漪、水滴 | 波纹流动，涟漪扩散，水滴起伏 | 沿波浪起伏 | 水滴与水波 | 水滴串落入水波 | 蓝、青 |
| `thunder` 雷 | 锯齿八边形、内八边形、旋转方阵 | 棱边噼啪跳变，闪电劈向阵心 | 沿直边排列，落雷时加亮 | 分叉闪电 | 折线落雷 | 亮蓝、黄 |
| `earth` 土 | 方形石印、角石、刻度、坤卦 ☷ | 菱形逐格顿挫转动 | 刻在四条边上 | 方印坤卦 | 裂纹石柱 | 黄、绿 |
| `holy` 神圣 | 放射圣光、光环、八芒星、光十字 | 光芒呼吸明灭 | prompt 字间留空 | 八芒星 | 渐宽光柱 | 默认前景、黄 |
| `dark` 黑暗 | 深渊漩涡臂、事件视界、血色新月 | 逆时针吞噬，尘埃坠入 | prompt 每遍尾部变暗；回复沿螺旋坠入视界 | 血色新月 | 暗影触须与坠滴 | 暗 magenta、红 |
| `eerie` 诡异 | 蠕动的环、缝线、会眨的邪眼与小眼 | 眼球转动、眨眼，整行故障错位 | 回复反向书写，偶有字符变色 | 独眼 | 不齐的滴落 | 绿、magenta |
| `tech` 科技 | 分段 HUD 环、刻度、雷达扫描、角括号 | 分段环转动，扫描线 | 圆环排字，并显示真实计时 `T+` 与 `WAIT`/`RECV` | 瞄准框 | 数据线与箭头 | cyan、绿 |

- 所有类型共用同一套增长规则：输入前为 5 行高的待机小阵，提交后变大，在 2.5/5/8/11 秒各解锁一层细节，收到第一段非空回复后停止增长；终端空间不足时省略内层。
- 各类型只改变文字的摆放路径，不改写字符；reasoning 不进入任何类型。`tech` 的计时是自提交起的真实耗时，`WAIT`/`RECV` 只表示是否已收到公开回复，不是进度百分比。
- Codex 的 `animations = false`：所有类型都不旋转、不闪烁、不错位、不眨眼，已解锁的层直接完整显示；只有 `tech` 的计时文字仍按真实时间更新。
- 配色：`classic` 只用 Codex 的 magenta/cyan。其他类型是用户主动选择的主题，按元素使用红、黄、蓝、绿等 ANSI 16 色，不使用 RGB/256 色，实际色值由终端主题决定。这是对 Codex 样式指南的有意例外，只作用于 `/magic` 装饰，不影响原生界面。
- 区别不只是颜色：测试把 10 种类型的待机、蓄力和出口画面去掉颜色后逐对比较，任意两种的已绘制单元格至少有 28%（待机）、37%（蓄力）、36%（出口）互不重合；最接近的分别是神圣/诡异、经典/神圣。待机小阵只有 9 列宽，中心单元格难免重合，所以这一项的数字最低。

### 验证记录

**多类型法阵（0005）**

- 完整原生 TUI 套件：4108 通过、10 跳过（nextest 另有 145 项 leaky，均为与魔法阵无关的 app 测试）。
- 魔法阵定向用例由 26 项增至 41 项，新增：
  - 10 种类型两两不同（去色比较）；
  - 蓄力时都在动，降低动效后都静止；
  - prompt 与回复的字符都写进阵中，出口都落到正文上方中央；
  - `tech` 计时与状态如实；
  - 弹窗预览、Esc 恢复、Enter 选用；
  - 未知参数不改设置，历史出口保留原类型；
  - 类型解析与共享设置。
- 12 份新快照（10 种类型的待机/6 秒蓄力/16 秒回复/出口图集、去色距离矩阵、选择弹窗）逐一审查后接受；classic 原有 6 份快照完全未变，说明移植到新框架后 classic 逐字节一致。
- Clippy（`-p codex-tui`）无新增告警，`just fmt` 已运行，`cargo fmt --check` 通过；release 构建仍只有未改动的 app-server 一处 `unused_mut`、cloud-tasks 两处未使用 import 警告。
- ConPTY 验收在通用策略与 Windows Terminal 策略下均通过：
  - `/magic list` 弹窗中按 ↓ 预览 wind，Esc 后恢复，且法阵仍关闭；
  - 再次打开后选中 fire 并 Enter，法阵以 fire 开启，一轮真实流程的出口位于正文上方；
  - `/magic 雷` 直接切换；
  - 三轮模型请求的默认 `instructions` 完全相同，控制命令不进入模型输入。
- 用真实程序按 Windows Terminal 策略渲染了 10 种类型的完整流程并逐张审查，修正了原型里的问题：
  - 风的 prompt 与回复螺旋交叉、诡异的 prompt 与回复共用一条文字带，都会让两段文字交错成乱码；
  - 诡异的小眼睛糊成实心块；
  - 水滴被文字盖住，水的外缘呈花瓣状；
  - 雷的落点角度计算有误，闪电折线过于平缓。
- 五阶段补丁：0001–0004 重新导出后逐字节不变，新增 0005（50 个文件）；950 个原提示、输入与键位相关文件保持不变。从原始 ZIP 依次重放五个阶段后，manifest 中 94 个文件及全部 1747 个 TUI 文件与源码逐字节一致。
- 发布的 `native\codex.exe` SHA256 为 `AAE2C8439944E01988FBAC9085925C6EAB03E682405C5BD981C087210626648A`；旧独立前端 SHA256 不变。本轮只用本地 fixture，没有调用真实模型。
- 已知取舍：
  - 选择弹窗的预览需要约 85 列以上；
  - 圆形与螺旋路径的下半部分文字从右向左排（与 classic 相同），`eerie` 的回复有意反向；
  - 类型不跨重启保存。

**视觉重绘（0004）**

- 完整原生 TUI 套件：4093 通过、10 跳过；魔法阵定向用例 26 项全部通过（含对称取整、明暗分层与配色、降低动效、长 prompt 接缝等新用例，6 份法阵快照逐一审查后接受）。
- nextest 另有 151 项标记为 leaky；与上一轮相比新增 19 项、消失 12 项，均为与魔法阵无关的 app 测试，属于运行间漂移，不把它描述成完全无告警。
- Clippy（`-p codex-tui`）无新增告警；上游完整 `just fmt` 已运行且未改动无关文件。release 构建仍有未改动的 app-server 一处 `unused_mut`、cloud-tasks 两处未使用 import 警告。
- ConPTY 验收在通用策略与 Windows Terminal 策略下均通过：
  - 法阵从待机约 9×5 字符扩大到 24×13、35×17（上一版为 5×3、16×9、20×11）；
  - 未换行首段与完整正文都在光锥下方；
  - 流中开关与 120→90 列缩放后顺序不变，正文不重复。
- 发现：在**非** Windows Terminal 的通用滚动策略下，流式输出期间已提交的正文行会从屏幕上暂时消失，直到回合结束重排才恢复。关闭魔法阵的原版同样复现，属于上游在 ConPTY 局部滚动区路径上的既有问题，并非本补丁引入；Windows Terminal 策略下逐帧检查（每 150ms）无丢失。
- 四阶段补丁从原始 ZIP 逐一重放，67 个改动文件与源码逐字节一致；0001/0002 与已提交版本相同，0003 原样保留；950 个原提示、输入与键位相关文件保持不变。
- 本轮只用本地 fixture，没有调用真实模型；旧独立前端 SHA256 不变。

**向下吐字（0003）**

- 本地 Responses fixture 驱动的原生 CLI 已验证 `/magic on/off/list`；原生回复和输入区不被覆盖。
- 新原生程序通过 ConPTY 验证：
  - 未换行首段与完整正文都在出口下方；
  - 流中关闭/重开和 120→90 列缩放后顺序不变，最终文字只出现一次；
  - 正文下面不再生成小阵。
  - 相同用例在旧程序上因首段不可见而失败。
- 开关前后发送给模型的默认 `instructions` 相同；控制命令不进入模型输入。reasoning 不进入法阵，也不会触发“首次回复”冻结。
- 当时的完整原生 TUI 套件：4087 通过、10 跳过，其中魔法阵定向用例 20 项通过；nextest 另有 144 项 leaky。
- 覆盖首段无换行、仅有完整结果而没有 delta、流中开关、表格延迟排版，以及 40 段中文混排在缩放和中断后完整保留。

**通用**

- 归档的版本快照已按实际 0.153.4 对齐；现有数据库迁移校验仅只读核对，没有通过改数据库、关闭迁移验证或删除历史来解决兼容问题。
- 原生补丁首版已用真实 Copilot 桥接验证助手回复及 `/magic on/off`；向下吐字使用本地 fixture，没有额外调用真实模型。旧独立前端的 SHA256 保持不变，原安装仍为 Codex 0.153.4。
- 验收自动化曾误触原版更新弹窗，在全局 npm 目录产生未完成的新安装；已按 npm 日志确认其为本次新建项并卸载清理，原来 `D:\Program Files\ChatGPT` 下的安装未变。现在补丁入口明确禁止启动自动更新，避免重现。
- 发布包按官方目录布局解压后，再次运行了原生端到端验收（两种滚动策略均通过），`codex doctor` 识别到包布局与内置 rg。

## Copilot CLI 外壳（magicopilot）

工作方式、参数与限制见 [copilot/README.md](../copilot/README.md)。代码结构：

| 文件 | 职责 |
| --- | --- |
| `launch.rs` | 命令行参数、查找并启动 Copilot CLI（路径规范化；批处理入口的参数转义与 Rust 标准库一致） |
| `pty.rs` | 伪终端（优先加载随包的 `conpty.dll`）、真实控制台的原始 VT 输入输出 |
| `screen.rs` | vt100 模拟 Copilot 的屏幕，把终端查询转交真实终端，跟踪同步输出、焦点、进度等状态 |
| `input.rs` | 解析键盘、鼠标、粘贴，鼠标坐标按法阵高度平移 |
| `session.rs` | 读取 `~/.copilot/session-state/<会话>/events.jsonl`，跟随会话切换 |
| `magic.rs` | 法阵状态机、区域高度、`/magic` 命令与输入框识别、类型选择器 |
| `render.rs`、`circle\` | 合成画面；10 种法阵的绘制（与 Codex 补丁同源） |
| `app.rs` | 主循环：子进程输出、输入、事件、限帧绘制、退出恢复 |

### 构建与测试

```powershell
scripts\Build-Copilot.ps1 -Action Build    # release 版本，并下载、校验、放置 ConPTY
scripts\Build-Copilot.ps1 -Action Test     # 单元测试
scripts\Build-Copilot.ps1 -Action Clippy
scripts\Build-Copilot.ps1 -Action Format
# 端到端验收：真实 Copilot CLI + 本地假模型服务（BYOK 离线，不消耗额度）
uv run --no-project --with pyte --with pywinpty --with psutil --with wcwidth --with pillow `
  python -X utf8 tests\copilot_terminal.py copilot\target\release\magicopilot.exe --windows-terminal --baseline
```

- 构建使用 Rust 1.95.0 MSVC 工具链与 Visual Studio Build Tools。
- 端到端测试在临时 `COPILOT_HOME` 中运行，不读写你自己的 Copilot 配置、会话或 Windows Terminal 设置。
- 各参数的作用：
  - `--windows-terminal` 模拟 Windows Terminal 环境；
  - `--baseline` 另外直接运行一次 Copilot，对比发给模型的请求；
  - `--frames <目录>` 保存各阶段画面。

### 验证记录（0.1.0）

- 端到端 `tests\copilot_terminal.py` 通过：真实 Copilot CLI 1.0.87 + 本地假模型服务，Windows Terminal 与通用两种模式。
  - 待机 5 行；提交后 21 行，点阵行数随时间 13→17；
  - prompt 与中间回复的字符出现在法阵中；
  - 最终回复时 24 行出口，回答在法阵下方；约 2.5 秒后回到 5 行，回答仍可读；
  - `/magic off`（光标先移回命令中间）后 Copilot 恢复全高，输入框被清空；
  - `/magic 火 `（末尾带空格）同样清空输入框；
  - `/magic list` 的 ↓ 预览、Esc 取消、数字选择都正常；
  - 控制命令没有进入任何模型请求；
  - 窗口缩到 18 行时，`/magic list` 只显示“窗口太矮”提示，之后的按键照常进入 Copilot 输入框；
  - 鼠标点击 Copilot 的 Sessions/Current 标签页正常；
  - `/exit` 退出并恢复终端，Copilot 的退出摘要（含 `--resume=` 命令）留在终端里；
  - Windows Terminal 模式下进度条序列被转发。
- 与直接运行 Copilot 对比（`--baseline`）：发给模型的系统提示（去掉时间、会话路径与 UUID 后）和工具列表完全一致，输入框同为半块样式。
- `--continue` 恢复会话、`/clear` 新建会话后，法阵继续跟随新的事件文件，两者都没有多建会话。
- 发布包内的 `magicopilot.exe`（与随包 ConPTY 一起）单独跑过一遍端到端测试，并从 GitHub Release 实际安装运行过。
- 35 项单元测试通过（包括用真实批处理文件验证参数中的 `&`、`|`、引号和 `%PATH%` 不会被 cmd 执行或展开，以及路径末尾带点或空格时仍按批处理文件转义）；Clippy（`-D warnings`）无告警。
- 随包附带 ConPTY 的原因：使用 Windows 自带的伪终端时，它会自己应答终端查询，Copilot 因此改用另一种输入框样式（`────`/`❯`），与直接运行不一致；换成 Windows Terminal 同款 `conpty.dll`/`OpenConsole.exe` 后两者一致。
- 代码审查共两轮，发现并修复 8 项问题：
  - 安装器：`-Force` 先删后装；删除不认识的目录；运行中的程序目录被删到只剩 EXE；相对路径解析错误。
  - 外壳：非 npm 的 `.cmd` 入口可被参数注入命令，路径末尾带点或空格可绕过转义（同 CVE-2024-43402）；命令末尾空格残留；窗口过矮时看不见的选择器吞掉按键。
- 尚未在人工操作的真实 Windows Terminal 窗口中验收，所以发布为预发布版；字体、输入法与主观观感需要实际使用确认。

## 独立前端（standalone）

最早的版本：通过 Codex app-server 连接 Codex，自绘整套界面。用 `scripts\Build.ps1` 构建（GNU 工具链），说明与验证记录见 [README-standalone.md](../README-standalone.md)。
