use crate::{
    backend::{Backend, BackendEvent, BackendKind},
    protocol::{self, Message, io_error},
    session::{Approval, Session, Status},
    terminal::TerminalGuard,
    terminal_input::InputReader,
    ui::{self, input::Editor},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Terminal,
    backend::TestBackend,
    widgets::{Paragraph, Wrap},
};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, VecDeque},
    env,
    fs::OpenOptions,
    io::{self, Write},
    path::PathBuf,
    sync::mpsc::TryRecvError,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct Options {
    pub backend: BackendKind,
    pub cwd: PathBuf,
    pub entry: Option<PathBuf>,
    pub model: Option<String>,
    pub demo: bool,
    pub snapshot: bool,
    pub probe: bool,
    pub smoke: bool,
    pub plain: bool,
    pub reduced_motion: bool,
    pub ascii: bool,
}

impl Options {
    pub fn parse() -> Result<Option<Self>, String> {
        let mut result = Self {
            backend: BackendKind::Copilot,
            cwd: env::current_dir().map_err(|e| e.to_string())?,
            entry: None,
            model: None,
            demo: false,
            snapshot: false,
            probe: false,
            smoke: false,
            plain: false,
            reduced_motion: false,
            ascii: env::var_os("NO_COLOR").is_some(),
        };
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => {
                    print_help();
                    return Ok(None);
                }
                "--version" | "-V" => {
                    println!("Magicodex {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                "--backend" => {
                    result.backend = match args.next().as_deref() {
                        Some("copilot") => BackendKind::Copilot,
                        Some("official") => BackendKind::Official,
                        _ => return Err("--backend 必须为 copilot 或 official".into()),
                    }
                }
                "--cwd" => result.cwd = PathBuf::from(args.next().ok_or("--cwd 需要路径")?),
                "--codex" => {
                    result.entry = Some(PathBuf::from(args.next().ok_or("--codex 需要启动器路径")?))
                }
                "--model" => result.model = Some(args.next().ok_or("--model 需要模型 ID")?),
                "--demo" => result.demo = true,
                "--snapshot" => result.snapshot = true,
                "--probe" => result.probe = true,
                "--smoke" => result.smoke = true,
                "--plain" => result.plain = true,
                "--reduced-motion" => result.reduced_motion = true,
                "--ascii" => result.ascii = true,
                _ => return Err(format!("未知选项：{arg}，使用 --help 查看帮助")),
            }
        }
        result.cwd = result
            .cwd
            .canonicalize()
            .map_err(|e| format!("工作目录无效：{e}"))?;
        // Windows verbatim paths are unsuitable as display/protocol cwd values.
        #[cfg(windows)]
        if let Some(path) = result.cwd.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
            result.cwd = PathBuf::from(if let Some(unc) = path.strip_prefix("UNC\\") {
                format!("\\\\{unc}")
            } else {
                path.to_string()
            });
        }
        if !result.cwd.is_dir() {
            return Err("--cwd 必须是目录".into());
        }
        if result.snapshot && !result.demo {
            return Err("--snapshot 必须与 --demo 一起使用，不会静默请求模型".into());
        }
        if result.probe && result.smoke {
            return Err("--probe 和 --smoke 不能同时使用".into());
        }
        if result.demo && (result.probe || result.smoke) {
            return Err("DEMO 不能代替真实后端 probe/smoke".into());
        }
        Ok(Some(result))
    }
}

fn print_help() {
    println!(
        "Magicodex {}\n\
真正的魔法阵终端 · 默认沿用现有 Copilot 桥接\n\n\
magicodex [--backend copilot|official] [--cwd PATH] [--model ID]\n\
          [--codex ENTRY] [--plain] [--reduced-motion] [--ascii]\n\
magicodex --demo [--snapshot]\n\
magicodex --backend official --probe\n\
magicodex --backend official --smoke\n\n\
--probe 仅检查 app-server 和模型列表，不发起模型推理。\n\
--smoke 明确发起三轮无工具测试请求，消耗所选后端额度；不自动批准任何工具。\n\
--codex 指定当前所选后端的 .exe/.ps1/.cmd 入口，prompt 不经过 shell。\n\
环境变量：MAGICODEX_COPILOT_ENTRY / MAGICODEX_OFFICIAL_ENTRY。\n\
F2 魔法阵 / F3 原文 / F4 正文 / F6 动效；PgUp/PgDn 滚动。\n\
Enter 提交，Alt+Enter 换行，Ctrl+S 导出，Ctrl+C 中断，Ctrl+N 新会话，Ctrl+Q 退出。",
        env!("CARGO_PKG_VERSION")
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Magic,
    Transcript,
    Answer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Initialize,
    Thread,
    Turn,
    Interrupt,
    Models,
    Mcp,
    Export,
}

struct Pending {
    purpose: Purpose,
    sent: Instant,
}

pub struct App {
    pub options: Options,
    pub session: Session,
    pub input: Editor,
    pub approval_input: Editor,
    pub view: View,
    pub scroll: u16,
    pub approval_scroll: u16,
    backend: Option<Backend>,
    pending: HashMap<u64, Pending>,
    started: Instant,
    completion_at: Option<Instant>,
    completion_seen: u64,
    question_index: usize,
    question_answers: Map<String, Value>,
    current_request: Option<Value>,
    demo_events: VecDeque<(f64, String, Value)>,
    demo_started: Option<Instant>,
    demo_serial: u64,
    pub probe_complete: bool,
    pub model_count: usize,
    pub model_ids: Vec<String>,
    mcp_poll_at: Option<Instant>,
    mcp_deadline: Option<Instant>,
    mcp_unsettled: bool,
    tools_ready: bool,
    cancel_requested: bool,
    quit: bool,
}

impl App {
    pub fn new(options: Options) -> Self {
        let view = if options.plain {
            View::Transcript
        } else {
            View::Magic
        };
        let mut session = Session::default();
        if options.demo {
            session.status = Status::Ready;
            session.thread_id = Some("demo".into());
            session.model = "模拟事件，不连接模型".into();
        }
        let tools_ready = options.demo;
        Self {
            options,
            session,
            input: Editor::default(),
            approval_input: Editor::default(),
            view,
            scroll: 0,
            approval_scroll: 0,
            backend: None,
            pending: HashMap::new(),
            started: Instant::now(),
            completion_at: None,
            completion_seen: 0,
            question_index: 0,
            question_answers: Map::new(),
            current_request: None,
            demo_events: VecDeque::new(),
            demo_started: None,
            demo_serial: 0,
            probe_complete: false,
            model_count: 0,
            model_ids: Vec::new(),
            mcp_poll_at: None,
            mcp_deadline: None,
            mcp_unsettled: false,
            tools_ready,
            cancel_requested: false,
            quit: false,
        }
    }

    pub fn connect(&mut self) -> io::Result<()> {
        self.backend = Some(Backend::start(
            self.options.backend,
            self.options.entry.as_deref(),
            &self.options.cwd,
        )?);
        self.request(Purpose::Initialize, "initialize", protocol::initialize())?;
        Ok(())
    }

    fn request(&mut self, purpose: Purpose, method: &str, params: Value) -> io::Result<()> {
        let id = self
            .backend
            .as_mut()
            .ok_or_else(|| io_error("后端未连接"))?
            .request(method, params)?;
        self.pending.insert(
            id,
            Pending {
                purpose,
                sent: Instant::now(),
            },
        );
        Ok(())
    }

    fn send(&mut self, value: Value) -> io::Result<()> {
        self.backend
            .as_mut()
            .ok_or_else(|| io_error("后端未连接"))?
            .send(&value)
    }

    pub fn tick(&mut self) -> io::Result<bool> {
        let before = self.session.revision;
        if self.mcp_poll_at.is_some_and(|at| Instant::now() >= at) {
            self.mcp_poll_at = None;
            self.mcp_unsettled = false;
            self.request(
                Purpose::Mcp,
                "mcpServerStatus/list",
                json!({"threadId":self.session.thread_id,"limit":100}),
            )?;
        }
        if self.mcp_deadline.is_some_and(|at| Instant::now() >= at) {
            self.mcp_deadline = None;
            self.mcp_poll_at = None;
            self.session
                .fail("MCP 工具初始化超时。为避免桥接会话中的工具目录变化，尚未提交任何 prompt。");
        }
        for _ in 0..64 {
            let event = match &self.backend {
                Some(backend) => match backend.try_recv() {
                    Ok(event) => event,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                },
                None => break,
            };
            match event {
                BackendEvent::Message(message) => self.message(message)?,
                BackendEvent::Diagnostic(text) => {
                    self.session
                        .system(format!("后端诊断：{}", ui::safe_text(&text)));
                }
                BackendEvent::Disconnected(error) => {
                    self.session.notice = error.clone();
                    self.session.system(error);
                    self.session.status = Status::Disconnected;
                    self.session.turn_active = false;
                    self.tools_ready = false;
                    self.mcp_poll_at = None;
                    self.mcp_deadline = None;
                    self.session.approvals.clear();
                    self.pending.clear();
                    if let Some(mut backend) = self.backend.take() {
                        backend.shutdown()?;
                    }
                    break;
                }
            }
        }
        if self
            .pending
            .values()
            .any(|p| p.sent.elapsed() > Duration::from_secs(45))
        {
            self.session.fail(
                "后端 RPC 超时；已关闭本实例连接。为避免重复副作用，不会自动重试或切换后端。",
            );
            self.pending.clear();
            self.session.status = Status::Disconnected;
            self.session.turn_active = false;
            self.tools_ready = false;
            self.mcp_poll_at = None;
            self.mcp_deadline = None;
            self.session.approvals.clear();
            if let Some(mut backend) = self.backend.take() {
                backend.shutdown()?;
            }
        }
        if let Some(started) = self.demo_started {
            while self
                .demo_events
                .front()
                .is_some_and(|(due, _, _)| *due <= started.elapsed().as_secs_f64())
            {
                let (_, method, params) = self.demo_events.pop_front().expect("checked");
                if let Err(error) = self.session.notification(&method, &params) {
                    self.session.fail(error);
                }
            }
        }
        let request = self.session.approvals.front().map(|a| a.id.clone());
        if request != self.current_request {
            self.current_request = request;
            self.question_index = 0;
            self.question_answers.clear();
            self.approval_input.take();
            self.approval_scroll = 0;
        }
        if self.session.completion_serial != self.completion_seen {
            self.completion_seen = self.session.completion_serial;
            self.completion_at = Some(Instant::now());
        }
        if self.cancel_requested && self.session.turn_active && self.session.turn_id.is_some() {
            self.cancel_requested = false;
            self.interrupt()?;
        }
        Ok(before != self.session.revision)
    }

    fn message(&mut self, message: Message) -> io::Result<()> {
        match message {
            Message::Response { id, result } => {
                let Some(pending) = self.pending.remove(&id) else {
                    self.session.system(format!("忽略无对应请求的响应 ID={id}"));
                    return Ok(());
                };
                match result {
                    Ok(value) => {
                        if let Err(error) = self.response(pending.purpose, value) {
                            if pending.purpose != Purpose::Export {
                                return Err(error);
                            }
                            self.session.notice = format!("导出失败：{error}");
                            self.session.system(self.session.notice.clone());
                        }
                    }
                    Err(error) => {
                        if pending.purpose == Purpose::Export {
                            self.session.notice = format!("导出失败：{error}");
                            self.session.system(self.session.notice.clone());
                        } else {
                            if pending.purpose == Purpose::Turn {
                                self.session.turn_active = false;
                            }
                            self.session.fail(error);
                        }
                    }
                }
            }
            Message::Notification { method, params } => {
                if let Err(error) = self.session.notification(&method, &params) {
                    self.session.fail(format!("{method}: {error}"));
                }
            }
            Message::Request { id, method, params } => {
                if self
                    .session
                    .approvals
                    .iter()
                    .any(|request| request.id == id)
                {
                    self.session
                        .system("重复的待处理请求已忽略；只对原始请求回复一次。");
                    return Ok(());
                }
                if serde_json::to_vec(&params)?.len() > 512 * 1024 {
                    self.send(json!({"id":id,"error":{"code":-32602,"message":"Approval/input request exceeds the 512 KiB review limit; nothing was approved."}}))?;
                    self.session.system("请求过大，无法完整审阅，已明确拒绝。");
                    return Ok(());
                }
                let supported = matches!(
                    method.as_str(),
                    "item/commandExecution/requestApproval"
                        | "item/fileChange/requestApproval"
                        | "item/permissions/requestApproval"
                        | "item/tool/requestUserInput"
                        | "execCommandApproval"
                        | "applyPatchApproval"
                );
                if !supported || self.options.smoke || self.options.probe {
                    if method == "mcpServer/elicitation/request" {
                        self.send(json!({"id":id,"result":{"action":"cancel","content":null,"_meta":null}}))?;
                    } else {
                        self.send(json!({"id":id,"error":{"code":-32601,"message":"This Magicodex mode does not support this server request; nothing was approved."}}))?;
                    }
                    self.session
                        .system(format!("明确拒绝未支持或无交互模式下的请求：{method}"));
                } else if let Err(error) = self.session.add_request(Approval {
                    id: id.clone(),
                    method,
                    params,
                }) {
                    self.send(json!({"id":id,"error":{"code":-32600,"message":error}}))?;
                    self.session.fail(error);
                }
            }
        }
        Ok(())
    }

    fn response(&mut self, purpose: Purpose, value: Value) -> io::Result<()> {
        match purpose {
            Purpose::Initialize => {
                self.send(json!({"method":"initialized","params":{}}))?;
                if let Some(agent) = value["userAgent"].as_str() {
                    self.session.system(format!("连接：{agent}"));
                }
                if self.options.probe {
                    self.request(Purpose::Models, "model/list", json!({}))?;
                } else {
                    let mut params = json!({"cwd": self.options.cwd});
                    if let Some(model) = &self.options.model {
                        params["model"] = json!(model);
                    }
                    self.request(Purpose::Thread, "thread/start", params)?;
                }
            }
            Purpose::Thread => {
                let id = value["thread"]["id"]
                    .as_str()
                    .ok_or_else(|| io_error("thread/start 缺少 thread.id"))?;
                self.session.thread_id = Some(id.into());
                self.session.status = Status::Connecting;
                self.tools_ready = false;
                self.session.model = value["model"].as_str().unwrap_or("后端默认").into();
                self.session.policy = format!(
                    "审批:{} · 沙箱:{}",
                    value["approvalPolicy"],
                    value["sandbox"]["type"].as_str().unwrap_or("继承后端配置")
                );
                self.session
                    .system("已建立真实会话。权限与认证继承所选后端，不自动提高权限。");
                self.session.notice = "正在等待 MCP 工具目录初始化完成。".into();
                self.mcp_deadline = Some(Instant::now() + Duration::from_secs(45));
                self.request(
                    Purpose::Mcp,
                    "mcpServerStatus/list",
                    json!({"threadId":self.session.thread_id,"limit":100}),
                )?;
            }
            Purpose::Turn => {
                let id = value["turn"]["id"]
                    .as_str()
                    .ok_or_else(|| io_error("turn/start 缺少 turn.id"))?;
                self.session.turn_id = Some(id.into());
                self.session.revision += 1;
            }
            Purpose::Interrupt => self.session.system("中断请求已送达，等待回合停止事件。"),
            Purpose::Models => {
                let models = value["data"]
                    .as_array()
                    .ok_or_else(|| io_error("model/list 缺少 data"))?;
                self.model_count = models.len();
                self.model_ids = models
                    .iter()
                    .filter_map(|m| m["model"].as_str().or_else(|| m["id"].as_str()))
                    .map(str::to_owned)
                    .collect();
                self.probe_complete = true;
                self.session.status = Status::Ready;
                self.tools_ready = true;
            }
            Purpose::Mcp => {
                let data = value["data"]
                    .as_array()
                    .ok_or_else(|| io_error("mcpServerStatus/list 缺少 data"))?;
                self.mcp_unsettled |= data.iter().any(|server| {
                    matches!(
                        server["runtimeStatus"].as_str(),
                        Some("starting" | "notStarted")
                    )
                });
                if let Some(cursor) = value["nextCursor"].as_str() {
                    self.request(
                        Purpose::Mcp,
                        "mcpServerStatus/list",
                        json!({"threadId":self.session.thread_id,"limit":100,"cursor":cursor}),
                    )?;
                } else if self.mcp_deadline.is_some() {
                    if self.mcp_unsettled {
                        self.mcp_poll_at = Some(Instant::now() + Duration::from_millis(250));
                    } else {
                        self.mcp_deadline = None;
                        self.session.status = Status::Ready;
                        self.tools_ready = true;
                        self.session.notice.clear();
                        self.session.system("工具目录初始化完成。");
                    }
                }
            }
            Purpose::Export => {
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| io_error(e.to_string()))?
                    .as_millis();
                let path = self.options.cwd.join(format!("magicodex-{stamp}.json"));
                let file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)?;
                serde_json::to_writer_pretty(file, &value)?;
                self.session.notice = format!(
                    "已按你的请求导出后端记录：{}；完整程度以记录中的 itemsView 为准。",
                    path.display()
                );
                self.session.system(self.session.notice.clone());
            }
        }
        Ok(())
    }

    pub fn submit(&mut self, prompt: String) -> io::Result<()> {
        if prompt.trim().is_empty() {
            return Ok(());
        }
        if self.session.status.busy()
            || self.session.turn_active
            || !self.session.approvals.is_empty()
        {
            return Err(io_error("当前回合尚未结束，输入已保留；不会并发提交"));
        }
        if !self.tools_ready {
            return Err(io_error("工具初始化尚未完成；Ctrl+N 可重新建立会话"));
        }
        let thread_id = self
            .session
            .thread_id
            .clone()
            .ok_or_else(|| io_error("尚未建立会话"))?;
        if self.session.status == Status::Disconnected {
            return Err(io_error("连接已断开，请退出后重新启动"));
        }
        self.cancel_requested = false;
        if self.options.demo {
            self.begin_demo(prompt);
        } else {
            self.request(
                Purpose::Turn,
                "turn/start",
                json!({"threadId":thread_id,"input":[{"type":"text","text":prompt}]}),
            )?;
            self.session.submitted(prompt);
        }
        self.scroll = 0;
        self.completion_at = None;
        Ok(())
    }

    fn interrupt(&mut self) -> io::Result<()> {
        if self.options.demo {
            self.demo_events.clear();
            if self.session.status == Status::Running {
                self.session.status = Status::Interrupted;
                self.session.turn_active = false;
                self.session.system("演示已中断。");
            }
        } else if let (Some(thread), Some(turn)) = (&self.session.thread_id, &self.session.turn_id)
        {
            if self
                .pending
                .values()
                .any(|pending| pending.purpose == Purpose::Interrupt)
            {
                return Ok(());
            }
            if self.session.turn_active || !self.session.approvals.is_empty() {
                self.request(
                    Purpose::Interrupt,
                    "turn/interrupt",
                    json!({"threadId":thread,"turnId":turn}),
                )?;
                self.session.status = Status::Interrupting;
                self.session.revision += 1;
            }
        } else if self.session.turn_active {
            self.cancel_requested = true;
            self.session.status = Status::Interrupting;
            self.session.notice = "中断已排队，收到 turn ID 后立即发送。".into();
        } else if self.session.status.busy() {
            self.session.notice = "后端尚未给出 turn ID；Ctrl+Q 可关闭本实例连接。".into();
        }
        Ok(())
    }

    pub fn current_question(&self) -> Option<&Value> {
        self.session.approvals.front()?.params["questions"]
            .as_array()?
            .get(self.question_index)
    }

    pub fn approval_detail(&self) -> String {
        let Some(request) = self.session.approvals.front() else {
            return String::new();
        };
        if request.is_input() {
            let Some(question) = self.current_question() else {
                return "请求缺少有效 questions。Esc 取消并中断；不会自动作答。".into();
            };
            let mut text = format!(
                "问题 {}：{}\n\n{}\n",
                self.question_index + 1,
                question["header"].as_str().unwrap_or("补充输入"),
                question["question"].as_str().unwrap_or("请求格式不受支持")
            );
            if let Some(options) = question["options"].as_array() {
                for (index, option) in options.iter().enumerate() {
                    text.push_str(&format!(
                        "\n{}. {} — {}",
                        index + 1,
                        option["label"].as_str().unwrap_or("?"),
                        option["description"].as_str().unwrap_or("")
                    ));
                }
            }
            text
        } else {
            let mut text = format!(
                "{}\n\n{}",
                request.method,
                serde_json::to_string_pretty(&request.params)
                    .unwrap_or_else(|e| format!("无法格式化请求：{e}"))
            );
            if let Some(id) = request.params["itemId"].as_str()
                && let Some(item) = self.session.items.iter().find(|i| i.id == id)
            {
                text.push_str(&format!("\n\n对应项目：{}\n{}", item.title, item.text));
            }
            text
        }
    }

    pub fn approval_scrolled_to_end(&self, width: u16, height: u16) -> bool {
        let lines = Paragraph::new(ui::safe_text(&self.approval_detail()))
            .wrap(Wrap { trim: false })
            .line_count(width.saturating_sub(2).max(1));
        usize::from(self.approval_scroll) + usize::from(height.saturating_sub(2)) >= lines
    }

    fn reply_approval(&mut self, allow: bool) -> io::Result<()> {
        let request = self
            .session
            .approvals
            .front()
            .ok_or_else(|| io_error("没有待审批请求"))?;
        if allow && request.method == "item/fileChange/requestApproval" {
            let id = request.params["itemId"]
                .as_str()
                .ok_or_else(|| io_error("文件审批缺少 itemId"))?;
            let item = self
                .session
                .items
                .iter()
                .find(|item| item.id == id && item.kind == "fileChange")
                .ok_or_else(|| io_error("尚未收到完整文件差异，不能批准"))?;
            if item.truncated {
                return Err(io_error(
                    "文件差异超过内存审阅上限；不能批准截断内容，请使用原版 Codex 处理",
                ));
            }
        }
        let result = request.response(allow).map_err(io_error)?;
        self.send(json!({"id":request.id,"result":result}))?;
        self.session.approvals.pop_front();
        self.session.system(if allow {
            "已由你明确允许此次请求。"
        } else {
            "已由你拒绝此次请求。"
        });
        Ok(())
    }

    fn answer_question(&mut self) -> io::Result<()> {
        let question = self
            .current_question()
            .ok_or_else(|| io_error("请求缺少有效问题，请 Esc 取消"))?
            .clone();
        let id = question["id"]
            .as_str()
            .ok_or_else(|| io_error("问题缺少 ID"))?;
        let mut answer = if question["isSecret"].as_bool() == Some(true) {
            self.approval_input.text.clone()
        } else {
            self.approval_input.text.trim().to_string()
        };
        if answer.is_empty() {
            return Err(io_error("请输入答案，或按 Esc 取消"));
        }
        if let Some(options) = question["options"].as_array().filter(|o| !o.is_empty()) {
            if let Ok(number) = answer.parse::<usize>() {
                answer = options
                    .get(number.wrapping_sub(1))
                    .and_then(|o| o["label"].as_str())
                    .ok_or_else(|| io_error("选项编号无效"))?
                    .to_string();
            } else if question["isOther"].as_bool() != Some(true)
                && !options.iter().any(|o| o["label"].as_str() == Some(&answer))
            {
                return Err(io_error("该问题要求选择列出的选项，请输入选项编号"));
            }
        }
        self.question_answers
            .insert(id.into(), json!({"answers":[answer]}));
        self.approval_input.take();
        self.question_index += 1;
        self.approval_scroll = 0;
        let request = self
            .session
            .approvals
            .front()
            .ok_or_else(|| io_error("请求已撤销"))?;
        let count = request.params["questions"]
            .as_array()
            .ok_or_else(|| io_error("问题列表无效"))?
            .len();
        if self.question_index >= count {
            self.send(json!({"id":request.id,"result":{"answers":self.question_answers}}))?;
            self.session.approvals.pop_front();
            self.question_answers.clear();
            self.session
                .system("补充输入已提交（不在本地日志记录答案）。");
        }
        Ok(())
    }

    pub fn key(&mut self, key: KeyEvent, size: (u16, u16)) -> io::Result<()> {
        if key.kind == KeyEventKind::Release {
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('q') => {
                    self.quit = true;
                    return Ok(());
                }
                KeyCode::Char('c') => {
                    self.interrupt()?;
                    return Ok(());
                }
                KeyCode::Char('n')
                    if !self.session.turn_active && self.session.approvals.is_empty() =>
                {
                    if let Some(mut backend) = self.backend.take() {
                        backend.shutdown()?;
                    }
                    let draft = self.input.take();
                    let options = self.options.clone();
                    *self = Self::new(options);
                    self.input.insert(&draft);
                    if !self.options.demo {
                        self.connect()?;
                    }
                    self.session.notice =
                        "已开始新会话，旧上下文未迁移；原记录可在后端历史中查找。".into();
                    return Ok(());
                }
                _ => {}
            }
        }
        if let Some(request) = self.session.approvals.front() {
            let is_input = request.is_input();
            match key.code {
                KeyCode::PageDown => {
                    self.approval_scroll = self.approval_scroll.saturating_add((size.1 / 2).max(1))
                }
                KeyCode::PageUp => {
                    self.approval_scroll = self.approval_scroll.saturating_sub((size.1 / 2).max(1))
                }
                KeyCode::Esc => {
                    if is_input {
                        self.send(json!({"id":request.id,"result":{"answers":{}}}))?;
                        self.session.approvals.pop_front();
                    } else {
                        self.reply_approval(false)?;
                    }
                    self.interrupt()?;
                }
                KeyCode::Enter if is_input => self.answer_question()?,
                KeyCode::Char('n' | 'N')
                    if !is_input
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.reply_approval(false)?
                }
                KeyCode::Char('y' | 'Y')
                    if !is_input
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if self.approval_scrolled_to_end(
                        size.0.saturating_sub(6),
                        size.1.saturating_sub(10),
                    ) {
                        self.reply_approval(true)?;
                    } else {
                        self.session.notice = "请先用 PgDn 阅读完整请求再批准。".into();
                    }
                }
                _ if is_input => {
                    self.approval_input.key(key);
                }
                _ => {}
            }
            return Ok(());
        }
        match key.code {
            KeyCode::F(2) => {
                self.view = View::Magic;
                self.scroll = 0;
            }
            KeyCode::F(3) => {
                self.view = View::Transcript;
                self.scroll = 0;
            }
            KeyCode::F(4) => {
                self.view = View::Answer;
                self.scroll = 0;
            }
            KeyCode::F(6) => self.options.reduced_motion = !self.options.reduced_motion,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub((size.1 / 2).max(1)),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add((size.1 / 2).max(1)),
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.options.demo {
                    let stamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| io_error(e.to_string()))?
                        .as_millis();
                    let path = self.options.cwd.join(format!("magicodex-demo-{stamp}.txt"));
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?;
                    file.write_all(self.session.transcript().as_bytes())?;
                    self.session.notice = format!("已导出演示记录：{}", path.display());
                } else {
                    let thread = self
                        .session
                        .thread_id
                        .as_ref()
                        .ok_or_else(|| io_error("没有可导出的 thread"))?;
                    self.request(
                        Purpose::Export,
                        "thread/read",
                        json!({"threadId":thread,"includeTurns":true}),
                    )?;
                    self.session.notice =
                        "正在请求后端历史；不会将截断的内存视图标记为完整导出。".into();
                }
            }
            KeyCode::Enter
                if !key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
            {
                self.submit(self.input.text.clone())?;
                self.input.take();
            }
            KeyCode::Esc => self.completion_at = None,
            _ => {
                self.input.key(key);
            }
        }
        Ok(())
    }

    pub fn animation_clock(&self) -> f64 {
        if self.options.reduced_motion {
            0.0
        } else {
            self.started.elapsed().as_secs_f64()
        }
    }

    pub fn reveal_progress(&self) -> Option<f64> {
        if self.options.reduced_motion || self.session.status != Status::Completed {
            return None;
        }
        self.completion_at
            .map(|t| t.elapsed().as_secs_f64())
            .filter(|t| *t < 1.0)
    }

    pub fn animating(&self) -> bool {
        !self.options.reduced_motion
            && self.view == View::Magic
            && self.session.approvals.is_empty()
            && (self.session.status == Status::Running || self.reveal_progress().is_some())
    }

    pub fn begin_demo(&mut self, prompt: String) {
        self.demo_serial += 1;
        let turn = format!("demo-{}", self.demo_serial);
        self.session.submitted(prompt.clone());
        self.demo_started = Some(Instant::now());
        self.demo_events = VecDeque::from(vec![
            (0.0, "turn/started".into(), json!({"turn":{"id":turn}})),
            (
                0.1,
                "item/completed".into(),
                json!({"turnId":turn,"item":{"id":"u","type":"userMessage","content":[{"type":"text","text":prompt}]}}),
            ),
            (
                0.4,
                "item/reasoning/summaryTextDelta".into(),
                json!({"turnId":turn,"itemId":"r","summaryIndex":0,"delta":"[演示摘要] 将文字化为符文，连接工具节点。"}),
            ),
            (
                1.2,
                "item/started".into(),
                json!({"turnId":turn,"item":{"id":"c","type":"commandExecution","command":"demo: inspect constellation","cwd":"DEMO","status":"inProgress"}}),
            ),
            (
                2.0,
                "item/commandExecution/outputDelta".into(),
                json!({"turnId":turn,"itemId":"c","delta":"[模拟工具输出] 六个节点已点亮，几何轨道对齐。"}),
            ),
            (
                2.6,
                "item/completed".into(),
                json!({"turnId":turn,"item":{"id":"c","type":"commandExecution","command":"demo: inspect constellation","cwd":"DEMO","status":"completed","exitCode":0,"aggregatedOutput":"[模拟工具输出] 六个节点已点亮，几何轨道对齐。"}}),
            ),
            (
                3.0,
                "item/started".into(),
                json!({"turnId":turn,"item":{"id":"a","type":"agentMessage","phase":"final_answer","text":""}}),
            ),
            (
                3.2,
                "item/agentMessage/delta".into(),
                json!({"turnId":turn,"itemId":"a","delta":"咒语已凝聚成形。\n\n"}),
            ),
            (
                3.8,
                "item/agentMessage/delta".into(),
                json!({"turnId":turn,"itemId":"a","delta":"这是离线演示，不是模型推理。\n真实模式下，这里显示 Codex 的原始回复。"}),
            ),
            (
                4.5,
                "turn/completed".into(),
                json!({"turn":{"id":turn,"status":"completed","items":[]}}),
            ),
        ]);
    }
}

pub fn run(options: Options) -> io::Result<()> {
    if options.snapshot {
        return snapshot(options);
    }
    if options.probe || options.smoke {
        return headless(options);
    }
    let mut terminal = TerminalGuard::enter()?;
    let mut events = InputReader::new()?;
    let mut app = App::new(options);
    if app.options.demo {
        app.begin_demo("将 prompt、工具和回复编织为终端魔法阵。".into());
    } else if let Err(error) = app.connect() {
        app.session.fail(error.to_string());
    }
    let mut dirty = true;
    let mut last_frame = Instant::now() - Duration::from_secs(1);
    let mut viewport = crossterm::terminal::size()?;
    let mut animated_before = false;
    while !app.quit {
        match app.tick() {
            Ok(changed) => dirty |= changed,
            Err(error) => {
                app.session.fail(error.to_string());
                dirty = true;
            }
        }
        let animate = app.animating()
            && crate::magic::supported_size(viewport.0, viewport.1.saturating_sub(9));
        dirty |= animated_before && !animate;
        animated_before = animate;
        if (dirty || animate) && last_frame.elapsed() >= Duration::from_millis(50) {
            terminal.terminal.draw(|f| ui::render(f, &app))?;
            last_frame = Instant::now();
            dirty = false;
        }
        if let Some(event) = events.next_event(Duration::from_millis(
            if app.session.turn_active || app.session.status == Status::Connecting {
                10
            } else {
                80
            },
        ))? {
            match event {
                Event::Key(key) => {
                    let size = crossterm::terminal::size()?;
                    if let Err(error) = app.key(key, size) {
                        app.session.notice = error.to_string();
                    }
                    dirty = true;
                }
                Event::Paste(text) => {
                    if app
                        .session
                        .approvals
                        .front()
                        .is_some_and(Approval::is_input)
                    {
                        app.approval_input.insert(&text);
                    } else if app.session.approvals.is_empty() {
                        app.input.insert(&text);
                    }
                    dirty = true;
                }
                Event::Resize(width, height) => {
                    viewport = (width, height);
                    dirty = true;
                }
                _ => {}
            }
        }
    }
    drop(terminal);
    if let Some(mut backend) = app.backend.take() {
        backend.shutdown()?;
    }
    Ok(())
}

fn headless(options: Options) -> io::Result<()> {
    let mut app = App::new(options);
    app.connect()?;
    let deadline = Instant::now() + Duration::from_secs(if app.options.probe { 55 } else { 240 });
    let prompts = [
        "This is a harmless client smoke test. Do not use any tools. Remember the marker ARCANA_731 and the exact Unicode phrase 青蓝星环. Reply only READY.",
        "Do not use tools. What marker did I ask you to remember? Reply only the marker.",
        "Do not use tools. Reply with the exact Unicode phrase from my first message, followed by a space and the marker.",
    ];
    let mut sent = 0;
    let mut checked = 0;
    while Instant::now() < deadline {
        app.tick()?;
        if matches!(app.session.status, Status::Failed | Status::Disconnected) {
            return Err(io_error(format!(
                "{}：{}\n{}",
                app.options.backend.name(),
                app.session.notice,
                app.session.transcript()
            )));
        }
        if app.options.probe && app.probe_complete {
            println!(
                "{}: initialize=ok model/list=ok models={} owned_pid={}",
                app.options.backend.name(),
                app.model_count,
                app.backend.as_ref().map_or(0, Backend::pid)
            );
            println!("models: {}", app.model_ids.join(", "));
            return Ok(());
        }
        if app.options.smoke {
            if app.session.status == Status::Completed && checked < sent {
                let answer = app.session.answer();
                if !app.session.items.iter().any(|item| {
                    item.kind == "userMessage"
                        && item.text == prompts[sent - 1]
                        && Some(item.turn_id.as_str()) == app.session.turn_id.as_deref()
                }) {
                    return Err(io_error("后端回显的输入与原文不一致，Unicode 回环失败"));
                }
                if sent > 1 && !answer.contains("ARCANA_731") {
                    return Err(io_error(format!(
                        "第 {sent} 轮未保留上下文；答案：{}",
                        ui::safe_text(&answer)
                    )));
                }
                if sent == 3 && !answer.contains("青蓝星环") {
                    return Err(io_error("中文回复未保持原始字符，Unicode 回环失败"));
                }
                println!("turn {sent}: completed; answer={}", ui::safe_text(&answer));
                checked = sent;
            }
            if checked == 3 {
                return Ok(());
            }
            if matches!(app.session.status, Status::Ready | Status::Completed) && checked == sent {
                app.submit(prompts[sent].into())?;
                sent += 1;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(io_error("验证超时；不会重发请求或自动切换后端"))
}

fn snapshot(options: Options) -> io::Result<()> {
    let mut app = App::new(options);
    app.begin_demo("编织文字与工具，让结果从魔法阵诞生。".into());
    while let Some((due, _, _)) = app.demo_events.front() {
        if *due > 2.5 {
            break;
        }
        let (_, method, params) = app.demo_events.pop_front().expect("checked");
        app.session
            .notification(&method, &params)
            .map_err(io_error)?;
    }
    app.options.reduced_motion = true;
    let mut terminal = Terminal::new(TestBackend::new(120, 40))?;
    terminal.draw(|f| ui::render(f, &app))?;
    let buffer = terminal.backend().buffer();
    for y in 0..40 {
        let mut line = String::new();
        for x in 0..120 {
            let cell = &buffer[(x, y)];
            if x > 0 && unicode_width::UnicodeWidthStr::width(buffer[(x - 1, y)].symbol()) > 1 {
                continue;
            }
            line.push_str(cell.symbol());
        }
        println!("{}", line.trim_end());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_options() -> Options {
        Options {
            backend: BackendKind::Copilot,
            cwd: PathBuf::from(r"C:\test"),
            entry: None,
            model: None,
            demo: true,
            snapshot: false,
            probe: false,
            smoke: false,
            plain: false,
            reduced_motion: true,
            ascii: false,
        }
    }

    #[test]
    fn static_states_do_not_animate() {
        let mut app = App::new(demo_options());
        app.options.reduced_motion = false;
        assert!(!app.animating());
        app.session.status = Status::Running;
        assert!(app.animating());
        app.view = View::Transcript;
        assert!(!app.animating());
    }

    #[test]
    fn renders_small_and_large_terminals_without_panicking() {
        for (width, height) in [(120, 40), (80, 24), (40, 12), (35, 10), (20, 5), (80, 3000)] {
            let mut app = App::new(demo_options());
            app.input.insert("EDGE_DRAFT");
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| ui::render(frame, &app)).unwrap();
            let cursor = terminal.get_cursor_position().unwrap();
            assert!(cursor.x < width && cursor.y < height);
            if width == 120 {
                let dots = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .filter(|c| {
                        c.symbol()
                            .chars()
                            .any(|c| ('\u{2800}'..='\u{28ff}').contains(&c))
                    })
                    .count();
                assert!(
                    dots > 100,
                    "Expected a real point-grid circle, got {dots} cells"
                );
            }
        }
    }

    #[test]
    fn demo_replays_full_success_and_keeps_original_text() {
        let mut app = App::new(demo_options());
        app.begin_demo("原始 prompt".into());
        while let Some((_, method, params)) = app.demo_events.pop_front() {
            app.session.notification(&method, &params).unwrap();
        }
        assert_eq!(app.session.status, Status::Completed);
        assert!(app.session.transcript().contains("原始 prompt"));
        assert!(app.session.answer().contains("这是离线演示"));
    }

    #[test]
    fn mcp_completion_opens_submission_gate() {
        let mut app = App::new(demo_options());
        app.tools_ready = false;
        app.session.status = Status::Connecting;
        app.mcp_deadline = Some(Instant::now() + Duration::from_secs(45));
        app.response(Purpose::Mcp, json!({"data":[],"nextCursor":null}))
            .unwrap();
        assert!(app.tools_ready);
        assert_eq!(app.session.status, Status::Ready);
    }

    #[test]
    fn duplicate_pending_request_is_not_answered_twice() {
        let mut app = App::new(demo_options());
        app.session
            .add_request(Approval {
                id: json!("a"),
                method: "item/commandExecution/requestApproval".into(),
                params: json!({}),
            })
            .unwrap();
        app.message(Message::Request {
            id: json!("a"),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({}),
        })
        .unwrap();
        assert_eq!(app.session.approvals.len(), 1);
        assert!(app.session.transcript().contains("只对原始请求回复一次"));
    }

    #[test]
    fn immediate_interrupt_waits_for_turn_id_without_losing_intent() {
        let mut app = App::new(demo_options());
        app.options.demo = false;
        app.session.submitted("test".into());
        app.interrupt().unwrap();
        assert!(app.cancel_requested);
        assert_eq!(app.session.status, Status::Interrupting);
        assert!(app.session.turn_active);
    }

    #[test]
    fn unsent_draft_is_inscribed_without_becoming_a_submitted_prompt() {
        let mut app = App::new(demo_options());
        app.input.insert("BINDING_DRAFT");
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
        let canvas: String = terminal.backend().buffer().content[..120 * 33]
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(canvas.contains("BINDING_DRAFT"));
        assert!(app.session.prompt.is_empty());
        assert!(!app.session.turn_active);
    }

    #[test]
    fn truncation_is_visible_in_every_primary_view() {
        for view in [View::Magic, View::Transcript, View::Answer] {
            let mut app = App::new(demo_options());
            app.view = view;
            app.session.trimmed = true;
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal.draw(|frame| ui::render(frame, &app)).unwrap();
            let footer: String = terminal.backend().buffer().content[120 * 39..]
                .iter()
                .filter(|cell| !cell.symbol().chars().all(char::is_whitespace))
                .map(|cell| cell.symbol())
                .collect();
            assert!(footer.contains("已截断"));
        }
    }
}
