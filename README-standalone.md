# Magicodex v0.1 独立前端（保留版本）

真正运行在 Windows Terminal 中的魔法阵 Codex 客户端。使用 Rust、Ratatui 和 Crossterm：没有 Electron、WebView、浏览器或额外的动画模型调用。

输入刻入外环，公开 reasoning 与中间回复成为流动文字，工具执行点亮节点；只有后端确认回合成功完成，才播放正文显现效果。F3 随时查看真实原文，F4 阅读稳定正文。动画不是模型内部思维链，也不是执行进度百分比。

## 运行

在 **Windows Terminal / PowerShell** 中执行：

```powershell
# 交付的根目录 EXE 可直接运行；下面的构建目录版本与它相同
.\magicodex.exe --demo

# 离线演示：不连接账号，不产生模型请求
.\target\release\magicodex.exe --demo

# 真实对话：默认使用 PATH 中的 codex（本机为 Copilot 桥接）
.\target\release\magicodex.exe --cwd 'C:\YourProject'

# 明确选择官方 Codex：使用 codex-original 入口及其现有认证
.\target\release\magicodex.exe --backend official --model gpt-5.5 --cwd 'C:\YourProject'

# 关闭动态效果，或直接以文本日志为主
.\target\release\magicodex.exe --reduced-motion
.\target\release\magicodex.exe --plain
```

运行前端不需要 Rust。真实模式需要用户自己安装和配置 Codex；桥接模式还需要桥接自身的 Copilot/Node 运行环境，不能将“前端单 EXE”理解为“整条进程链只有一个进程”。

推荐终端至少 120 列、40 行，使用支持中文与 Braille 的等宽字体。小于布局要求或超出点阵画布容量时自动显示文本，不会因极端窗口尺寸崩溃；`--ascii` 使用基础点线标记。中文字符能沿圆环移动，但终端字符本身不能像图片一样旋转。

首版以支持括号粘贴协议的 Windows Terminal / ConPTY 终端为目标；不支持该协议的旧式控制台不在兼容范围内。

### 后端入口

| 选项 | 入口 | 说明 |
| --- | --- | --- |
| `--backend copilot` | PATH 中的 `codex` | 默认；不自动切换官方账号 |
| `--backend official` | PATH 中的 `codex-original` | 用户明确选择后使用 |
| `--codex '完整路径'` | 指定 `.exe`、`.ps1` 或 `.cmd` | 仅覆盖本次选择的启动入口 |

也可设置当前进程环境变量 `MAGICODEX_COPILOT_ENTRY` 或 `MAGICODEX_OFFICIAL_ENTRY`。模型用 `--model ID` 指定；未指定时继承后端默认模型。前端不修改全局 PATH、Codex 配置、登录、provider 或沙箱策略。

默认 Copilot 模式会验证入口是可识别的现有桥接；如果 PATH 中的 `codex` 换成了其他程序，会拒绝启动，避免误用另一账号来源。显式设置入口表示由你负责指定正确的 provider。

**本机桥接适配：** 原桥接依赖的 `1.0.84-1` runtime 曾被清理，实施中已从官方 npm 包恢复原版本，未改动桥接配置。原启动器的 `--profile` 参数不能用于 app-server，因此 EXE 内嵌了一个小型 Node 启动适配器：只读解析原 profile，复用原桥接模块，通过标准 `-c` 参数传给 Codex。不会修改现有 `codex` 命令，也不改变账号或计费来源。

适配器在 profile 没有明确指定权限时，使用更保守的 `read-only` 沙箱与 `on-request` 审批，避免继承另一个默认配置的宽松权限。profile 已指定的权限则保留。`--codex` 显式指定入口时直接使用该入口，不自动适配。

连接后会等待 MCP 工具目录完成初始化再允许首轮输入，避免桥接的工具目录在第二轮发生变化而拒绝继续对话。运行中如果用户改变 MCP/工具配置，旧桥接可能要求新会话；Ctrl+N 明确新建，不静默丢弃或迁移上下文。

**官方模型选择：** 本机官方账号目录返回 `gpt-5.6-terra`、`gpt-5.6-luna`、`gpt-5.5`。原配置中的 `gpt-5.4` 不受该账号支持，因此示例显式选择已验证的 `gpt-5.5`，没有修改全局默认模型。其他账号请先运行 `--backend official --probe` 查看自身目录。

## 键盘

| 操作 | 按键 |
| --- | --- |
| 提交当前输入 | Enter |
| 输入换行 | Alt+Enter（终端支持时也可 Shift+Enter）；多行粘贴 |
| 编辑 | 左右/上下/Home/End/Backspace/Delete，按 Unicode 字素处理 |
| 魔法阵 / 原文 / 最终正文 | F2 / F3 / F4 |
| 动效开关 | F6 |
| 正文或审批详情滚动 | PgUp / PgDn |
| 中断正在执行的回合 | Ctrl+C |
| 明确新建会话 | Ctrl+N；旧上下文不迁移，未提交的输入草稿保留 |
| 退出并关闭本实例拥有的后端 | Ctrl+Q |
| 显式导出到当前工作目录 | Ctrl+S |
| 审批 | 阅读详情后 Y 允许；N 拒绝；Esc 拒绝并中断 |
| 补充问题 | 输入文字或选项编号，Enter 下一题/提交；Esc 取消并中断 |

待审批时动画暂停，输入焦点属于审批表单。长请求需要滚动看完才能允许；拒绝和中断不需要读到底。权限请求最多授予本回合，不提供永久批准按钮。秘密问题的输入会被遮挡，答案不记录在前端日志中。

没有启用鼠标捕获，可使用终端自己的文本选择功能。复制较长文本时建议 F4 或 Ctrl+S。Windows 使用原生 VT 输入模式与括号粘贴解码，多行粘贴只编辑，不会自动提交；单次待解码输入/粘贴上限为 1 MiB。秘密问题的输入不会回显原文。

## 会话与边界

- 单 thread、多轮对话；一个回合未结束时可继续编辑下一条输入，但不能并发提交。
- 模型消息、reasoning、工具原文与视觉片段分开，最终完整 item 会校准已有 delta，而不是重复追加。
- reasoning 只展示后端公开的内容；后端不提供时显示等待状态。
- 保留命令、文件变更、退出码和错误。工具失败不会因为模型正常结束而从日志中消失。
- 未支持的服务端工具/审批扩展明确拒绝；不冒充官方客户端能力，不自动批准动态工具或 MCP 表单。
- 不支持原版所有 slash commands、插件 GUI、多会话面板、跨后端迁移或桥接冷恢复。
- 断连不自动重发 prompt，防止命令和文件修改重复执行。
- 提交后立即按 Ctrl+C 也有效：如果后端尚未返回 turn ID，中断意图会排队，在取得 ID 后发送。桥接被中断后可能要求新会话，此时用 Ctrl+N。
- 单条协议消息上限为 16 MiB；原文内存窗口约 4 MiB、256 项，单项最多保留末尾 512 KiB。发生截断会明确标记，不再声称内存视图完整。
- Ctrl+S 向后端请求 thread 历史并导出原始 JSON；是否包含全部历史以响应的 `itemsView` 等元数据为准。后端不支持或记录超过协议上限时会明确失败，不使用截断内存冒充成功导出。
- 默认不额外落盘 prompt、reasoning 或工具输出；后端自己的历史行为仍遵循其配置。用户主动导出的文件可能包含敏感项目内容，请自行保管。
- 模型与工具文本不会作为 ANSI/OSC 终端控制指令执行。
- 桥接当前不转发所有模型的 reasoning；无事件时保持等待状态，不伪造推理。没有 message phase 的回复会等回合完成再进入正文视图。

## 构建

需要 Rust 工具链。开发环境与最终发布包是两回事，发布后的 EXE 不需要 Rust、LLVM 或 Python。

本机采用 Rust GNU 工具链、Rust 自带的链接器，以及 LLVM-MinGW 的 `dlltool.exe`，不需要 Visual Studio。`scripts\Build.ps1` 会为当前进程配置工具，不修改全局 PATH。它默认使用官方 GitHub 上的 crates 索引，规避本机 `index.crates.io` 的网络问题；下载内容仍校验 `Cargo.lock`。网络正常时可用 `-StandardRegistry`。

```powershell
# 已安装开发工具及缓存依赖的本机
.\scripts\Build.ps1 -Task all -Offline

# 新机器：先准备 Rust 与 GNU dlltool，或使用正常配置的 MSVC 工具链
.\scripts\Build.ps1 -Task all
```

正常配置的 Rust 环境也可直接运行：

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

如果 Rust 使用 `--no-modify-path` 安装，本机可在当前 PowerShell 进程临时设置：

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

`Cargo.lock` 固定依赖；`target\release\magicodex.exe` 为构建产物，发布脚本同时复制一份到根目录 `magicodex.exe`。Windows x64 发布构建仅依赖 Windows 系统 DLL，不需要额外的 GNU/LLVM DLL。不要把 `.venv`、构建缓存或整个开发工具链打包给运行用户。

## 轻量性观察

Windows x64、120×40 ConPTY、离线演示模式下的一次本机样本：

| 前端模式 | 私有内存峰值 | 工作集峰值 | 活动阶段 CPU 时间 / 4 秒 |
| --- | --- | --- | --- |
| 动态魔法阵 | 1.49 MiB | 5.18 MiB | 62.50 ms |
| 减少动态效果 | 1.21 MiB | 4.95 MiB | 156.25 ms |
| 文本日志 | 1.16 MiB | 4.91 MiB | 0.00 ms |

这些是短样本，不是性能保证。CPU 读数受计时粒度、启动及共享机器负载影响，不能从这组数据断言某种模式始终更省 CPU；0.00 ms 也不等于绝对零消耗。三种模式的静止观察窗口均没有继续向终端写入重绘字符。

EXE 约 1.1 MiB。上述数字**只包含前端进程**，不含 Windows Terminal、测试驱动或 Codex/桥接。另一次真实工具调用观察中，后端刚就绪时，Copilot 路径进程链私有内存合计约 139.40 MiB，官方路径约 118.63 MiB；这不是推理期间的峰值。实际上下文和日志增长会提高占用，内存窗口上限见前文。

## 验证入口

```powershell
# 无模型调用，仅握手与模型目录
.\target\release\magicodex.exe --backend official --probe
.\target\release\magicodex.exe --backend copilot --probe

# 三轮真实对话，会使用所选后端额度；工具一律不自动批准
.\target\release\magicodex.exe --backend official --model gpt-5.5 --smoke

# 固定大小的文字快照，不需要真实终端，也不调用模型
.\target\release\magicodex.exe --demo --snapshot
```

协议适配以本地 Codex `0.153.4` 的生成 schema 为起点；并非承诺兼容任意版本。升级 Codex 后先运行 `--probe`，再运行你愿意授权的真实请求。

无头快照和自动化测试只能覆盖布局、事件和终端协议，不能替代真实 Windows Terminal 中的字体、中文输入法与视觉审美检查。

额外的 Windows ConPTY 检查仅需要开发用 Python 依赖，不属于运行时：

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install --index-url https://pypi.org/simple -r .\tests\requirements.txt
.\.venv\Scripts\python.exe -X utf8 .\tests\terminal_smoke.py .\target\release\magicodex.exe
.\.venv\Scripts\python.exe -X utf8 .\tests\terminal_smoke.py .\target\release\magicodex.exe --fixture .\tests\fixtures\app-server.ps1

# 以下会使用所选后端额度；只要求执行一个无害的 Write-Output 命令
.\.venv\Scripts\python.exe -X utf8 .\tests\terminal_smoke.py .\target\release\magicodex.exe --live copilot
```

可设置当前进程 `MAGICODEX_BRIDGE_DIAGNOSTICS=1` 查看桥接工具目录变化的字段名；不记录 prompt、参数值或工具描述正文。

## 代码结构

| 模块 | 责任 |
| --- | --- |
| `src\backend.rs` | 启动入口、标准输入输出、受控子进程、Windows 私有 Job Object |
| `src\copilot_adapter.mjs` | 嵌入 EXE 的只读桥接适配器，复用用户已安装模块 |
| `src\protocol.rs` | JSON-RPC 包络、分片/UTF-8 解码、消息大小边界 |
| `src\session.rs` | thread/turn/item 状态、原文归并、审批队列、内存窗口 |
| `src\app.rs` | 用户操作、请求关联、主循环、演示和诊断入口 |
| `src\terminal.rs` | raw mode、alternate screen、正常/异常退出恢复 |
| `src\terminal_input.rs` | Windows VT 输入、UTF-8 分片、括号粘贴和按键解码 |
| `src\ui` | 输入编辑、日志、正文和审批覆盖层 |
| `src\magic` | 几何图案、点阵、文字轨道和事件动画 |

前端不直接调用模型 API，不包含认证密钥，不解析原版 TUI 的 ANSI 输出。stdin/stdout 只连接 app-server 协议，stderr 单独作为后端诊断显示。
