# Magicodex for Codex CLI 0.153.4

这是 OpenAI Codex CLI **0.153.4** 加上 Magicodex 魔法阵补丁后重新构建的版本（非官方）。Codex 自己的输入框、快捷键、命令菜单、默认提示、审批、会话和登录都保持原样；魔法阵只是显示层，用 `/magic` 命令控制。

## 启动

| 命令（安装后） | 包内文件 | 用途 |
| --- | --- | --- |
| `magicodex` | `magicodex.cmd` | 使用你自己的 Codex 登录（ChatGPT 账号或 API key）和 `~/.codex` 配置；参数原样交给 Codex |
| `magicodex-bridge` | `magicodex-bridge.cmd` | 只适用于本机已有 `~/.codex/copilot-proxy` 本地 GitHub Copilot 桥接的环境（需要 Node.js） |

没有登录过 Codex 时，先运行 `magicodex login`（与官方 `codex login` 相同）。补丁版启动时关闭了官方的“有新版本”更新弹窗：那个更新只会安装另一份没有补丁的 Codex。

补丁版与官方 Codex 共用 `~/.codex`（配置、登录、会话与状态数据库）。如果你的 `~/.codex` 已被**更新版本**的官方 Codex 使用过，0.153.4 可能无法识别新版本写入的数据库迁移；这时可以为补丁版单独设置 `CODEX_HOME`，例如 `set CODEX_HOME=%USERPROFILE%\.magicodex`（需要在该目录重新登录）。

## 魔法阵

在 **Codex 输入框内**输入（不要作为命令行参数传入，那会成为给模型的 prompt）：

| 命令 | 作用 |
| --- | --- |
| `/magic on` / `/magic off` | 开 / 关魔法阵（默认关闭，保持原版起始界面） |
| `/magic list`（或 `/magic`） | Codex 原生选择弹窗：↑↓ 预览，Enter 选用并开启，1–9 直接选，Esc 恢复 |
| `/magic <类型>` | 直接切换并开启，例如 `/magic fire`、`/magic 火` |

10 种类型：classic 经典、wind 风、fire 火、water 水、thunder 雷、earth 土、holy 神圣、dark 黑暗、eerie 诡异、tech 科技。输入前是小法阵；提交后法阵随等待逐层变大、变复杂，prompt 环绕外圈、中间回复环绕内圈（不含 reasoning）；最终回复开始时法阵定格为出口，正文从法阵下方吐出。设置只在本次运行中有效，不写入配置文件。

## 包内文件

| 路径 | 来源 |
| --- | --- |
| `bin\codex.exe` | 由 OpenAI Codex `rust-v0.153.4` 源码应用 `patches\` 中的补丁后构建 |
| `bin\codex-code-mode-host.exe`、`codex-resources\*.exe`、`codex-path\rg.exe`、`codex-package.json` | 官方 npm 包 `@openai/codex-win32-x64` 0.153.4 中的原文件，未修改（与官方包相同的目录布局，Codex 据此找到沙箱辅助程序和 ripgrep） |
| `patches\*.patch`、`patches\manifest.json` | 相对上游源码的全部改动 |
| `scripts\Start-NativeBridge.mjs`、`*.cmd` | Magicodex 启动脚本 |

## 许可

Codex 以 Apache License 2.0 发布，见 `LICENSE` 与 `NOTICE`；补丁同样以 Apache-2.0 提供，改动说明见 `MAGICODEX-NOTICE.md`。这不是 OpenAI 官方发布的 Codex。

源码、构建方法与完整验证记录：<https://github.com/dengyanbo/Magicodex>
