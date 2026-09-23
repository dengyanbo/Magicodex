# Magicodex 原生 TUI 补丁

基于官方 **Codex 0.153.4** 的非官方 UI 补丁。沿用 Codex 自己的输入框、快捷键、命令菜单、历史记录、审批和默认提示；魔法阵不是另一个仿 Codex 的终端界面。

原来的 `magicodex.exe` 独立前端保留不动，旧版说明见 [README-standalone.md](README-standalone.md)。原生版使用下面的新入口，不替换系统中已安装的 Codex。

## Git 仓库范围

仓库包含项目源码、修改后的上游源码、补丁及构建脚本，保留上游许可证与声明；不提交 EXE、依赖缓存、源码 ZIP、用户配置或数据库。克隆后需按下文构建，Git 仓库本身不包含本地已生成的二进制。许可证范围见 `LICENSE` 和 `NOTICE`。

## 使用原生版

先完成原生构建和发布，再在 Windows Terminal 中运行：

```powershell
# 沿用原有 Copilot 桥接与 profile
.\magicodex-native.ps1

# 原版官方入口与配置；参数原样交给 Codex
.\magicodex-native-official.ps1 --model gpt-5.5
```

在 **Codex 输入框内**输入：

```text
/magic on
/magic off
/magic list
```

不要把 `/magic on` 当作 CLI 的位置参数传入；原版 Codex 会把位置参数视为给模型的初始 prompt。

默认关闭效果，保持原版起始界面；`/magic on` 后显示小型待机阵。开关只影响当前应用的展示，并在当前应用的新会话/会话切换间共享，不改用户配置文件。

## 效果与内容边界

- 输入框与正常回复保持原版样式，在法阵外面；阵心不放文本框。
- 提交新回合后，法阵立即变大，随后按等待时间扩大、增加圆环与几何层次，有终端空间上限。
- 收到本回合第一段非空助手回复后停止增长；运行期间可以继续旋转，回合结束缩回待机阵。
- prompt 的片段逐字沿外环排布；最近的部分助手回复沿内部圆环排布。字符位置沿圆弧移动，不伪装成像素图像旋转。
- 法阵不采集 reasoning。Codex 原本的状态行与完整 transcript 不因补丁而删改。
- `/magic` 是本地显示命令，不调用模型，也不添加系统指令。
- `/magic list` 当前只有实际实现的 `classic` 类型。类型列表入口已保留，没有把未实现的类型列为可用。
- 不增加 F2/F3/F4 等专属快捷键，沿用 Codex 的键位与用户既有 keymap。

## 后端与安装边界

`scripts\Start-NativeBridge.mjs` 复用现有桥接的 SDK、模型目录、profile 和生命周期代码，只把实际运行的 Codex 换成补丁版。模型目录模板读取自原安装，避免悄悄替换其默认指令。

桥接原有的后端限制仍然存在；原生 UI 不会把一个原本不支持的桥接操作伪装成成功。官方入口不通过桥接，也不会自动切换账号或模型。

原生补丁锁定 0.153.4。升级原安装后，应在对应源码版本重新移植，而不是混用不同版本的模型模板和运行组件。

补丁入口在当前进程关闭原版启动更新弹窗：该弹窗会调用上游安装器，无法更新这份 UI 补丁，还可能安装另一份未打补丁的 Codex。补丁升级由本项目构建/发布管理；这是发行管理限制，不修改原始配置文件、系统提示词或快捷键。系统中原来的 Codex 入口仍保留其原有更新行为。

这不再是约 1.1 MiB 的独立前端方案：发布目录包含 Codex 本体及同版本的原始 `codex-code-mode-host.exe`。选择原生补丁的取舍是更大的构建/分发体积，换取真实的原版交互兼容。

本次 Windows 发布：补丁版主程序约 283.65 MiB，未修改的 code-mode host 约 69.12 MiB。这是磁盘体积，不代表运行内存。

## 源码与补丁

| 路径 | 用途 |
| --- | --- |
| `upstream\codex-rust-v0.153.4` | 匹配发布标签的源码 |
| `native\codex.exe` | 构建并发布后的补丁版主程序 |
| `native\codex-code-mode-host.exe` | 同版本、未修改的官方运行组件 |
| `native-patch\0001-release-lock-alignment.patch` | 发布标签的内部包版本与 lockfile 对齐 |
| `native-patch\0002-native-magic-tui.patch` | 原生 TUI 魔法阵补丁 |
| `native-patch\manifest.json` | 上游版本、改动文件与受保护源码校验 |

重新导出补丁：

```powershell
# 源码 ZIP 不在 Git 中；仅重新导出补丁时需要下载原始基线
Invoke-WebRequest 'https://codeload.github.com/openai/codex/zip/refs/tags/rust-v0.153.4' -OutFile .\upstream\codex-rust-v0.153.4.zip
python .\scripts\Export-NativePatch.py --archive .\upstream\codex-rust-v0.153.4.zip
```

上游标签的 manifest 是 0.153.4，但锁文件中的 149 个工作区包仍标记为 0.0.0。准备构建时使用 `cargo update --workspace --offline` 对齐，1232 条外部依赖记录保持不变，并运行了上游要求的 Bazel 锁更新。

第一阶段补丁还包含发布版快照对齐：上游快照原先按开发版 `0.0.0` 绘制版本横幅、填充空格和更新提示；本次构建实际为 `0.153.4`。这些差异已与魔法阵补丁分开保存，不通过改动运行时版本或原版提示来迁就快照。

**Windows 数据库兼容：** 官方 Windows 构建的 SQLx 迁移校验基于 CRLF，而 GitHub 源归档为 LF。构建脚本会将 `state` 迁移文件的换行规范为 CRLF（SQL 内容不变），避免已有数据库被误报“迁移被修改”。没有重写、删除或重建用户数据库；可用 `python .\scripts\Prepare-NativeWindows.py --check-database` 只读核对已安装的迁移校验。

补丁只在 TUI 增加渲染模块、命令及少量生命周期挂钩，不修改 core、protocol、模型提示模板、原输入组件或键位实现。

## 构建与验证

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

源码以发布归档形式保存，没有 Git 索引。上游 `just fmt` 的全仓库入口会因此无法枚举 Bazel 文件；这种情况下只对改动的 Rust crate 使用同一 Rust 格式化流程，不声称已检查未改动的 Python/Bazel 文件。

本机归档的 Rust 格式化替代命令：`.\scripts\Build-Native.ps1 -Action FormatRust`。

`tests\native_terminal.py` 使用本地 Responses fixture 驱动真实原生 CLI，不消耗模型额度，用于检查本地命令、图形增长、开关、默认指令一致性及原退出键。

字体效果、输入法候选窗与主观审美仍需在用户实际 Windows Terminal 中确认。

### 验证记录与边界

- 本地 Responses fixture 驱动的原生 CLI 已验证 `/magic on/off/list`，观察到法阵从约 5×3 字符的待机范围扩大到 16×9、20×11；原生回复和输入区不被覆盖。
- 开关前后发送给模型的默认 `instructions` 相同；控制命令不进入模型输入。reasoning 不进入法阵，也不会触发“首次回复”冻结。
- 完整原生 TUI 套件：4077 通过、10 跳过。nextest 另有 144 项标记为 leaky（测试返回后管道/后台任务未及时结束），不把它描述成完全无告警。
- 归档的版本快照已按实际 0.153.4 对齐；现有数据库迁移校验仅只读核对，没有通过改数据库、关闭迁移验证或删除历史来解决兼容问题。
- 真实 Copilot 桥接已验证助手回复及 `/magic on/off`。旧独立前端的 SHA256 保持不变，原安装仍为 Codex 0.153.4。
- 验收自动化曾误触原版更新弹窗，在全局 npm 目录产生未完成的新安装；已按 npm 日志确认其为本次新建项并卸载清理，原来 `D:\Program Files\ChatGPT` 下的安装未变。现在补丁入口明确禁止启动自动更新，避免重现。
