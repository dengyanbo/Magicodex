use crate::protocol::{self, Message, io_error};
use serde_json::Value;
use std::{
    env, fs,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Copilot,
    Official,
}

impl BackendKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Copilot => "copilot",
            Self::Official => "official",
        }
    }
}

#[derive(Debug)]
pub enum BackendEvent {
    Message(Message),
    Diagnostic(String),
    Disconnected(String),
}

pub struct Backend {
    child: Child,
    outbound: Option<SyncSender<Value>>,
    rx: Receiver<BackendEvent>,
    next_id: u64,
    #[cfg(windows)]
    _job: job::Job,
}

fn find_entry(kind: BackendKind) -> io::Result<PathBuf> {
    let key = match kind {
        BackendKind::Copilot => "MAGICODEX_COPILOT_ENTRY",
        BackendKind::Official => "MAGICODEX_OFFICIAL_ENTRY",
    };
    if let Some(path) = env::var_os(key) {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(io_error(format!("{key} 不是有效文件")))
        };
    }
    let name = match kind {
        BackendKind::Copilot => "codex",
        BackendKind::Official => "codex-original",
    };
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            for ext in ["exe", "ps1", "cmd", "bat"] {
                let candidate = dir.join(format!("{name}.{ext}"));
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err(io_error(format!(
        "找不到 {name}；请设置 {key} 为入口绝对路径。不会自动切换后端。"
    )))
}

fn entry_command(path: &Path) -> io::Result<Command> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "ps1" => {
            let mut command = Command::new("powershell.exe");
            command
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
                .arg(path);
            Ok(command)
        }
        "cmd" | "bat" => {
            // Only an explicit launcher path goes into the shell. Prompts use the JSON pipe.
            if path.to_string_lossy().contains(['"', '\r', '\n', '%', '!']) {
                return Err(io_error(
                    "启动器路径包含不支持的 shell 字符，请使用 .ps1 或 .exe 入口",
                ));
            }
            // Rust supplies the additional cmd.exe quoting required by spaced batch paths.
            Ok(Command::new(path))
        }
        _ => Ok(Command::new(path)),
    }
}

fn bridge_adapter(entry: &Path) -> io::Result<Option<Command>> {
    let Some(root) = entry.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    if !root.join("src").join("launch.mjs").is_file() || !root.join("settings.json").is_file() {
        return Ok(None);
    }
    let package: Value = serde_json::from_str(&fs::read_to_string(root.join("package.json"))?)?;
    if package["name"] != "local-codex-copilot-proxy" {
        return Ok(None);
    }
    let settings: Value = serde_json::from_str(&fs::read_to_string(root.join("settings.json"))?)?;
    let runtime = settings["copilotRuntime"]
        .as_str()
        .ok_or_else(|| io_error("桥接设置缺少 copilotRuntime"))?;
    if !Path::new(runtime).is_file() {
        return Err(io_error(
            "桥接配置的 Copilot runtime 不存在。请恢复桥接所需版本；不会自动改动桥接配置或切换后端。",
        ));
    }
    let mut overrides = Vec::new();
    if let Some(profile) = settings["profileName"].as_str().filter(|p| !p.is_empty()) {
        if profile.contains(['/', '\\', ':']) || matches!(profile, "." | "..") {
            return Err(io_error("桥接 profileName 必须是文件名，不是路径"));
        }
        let home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| root.parent().map(Path::to_path_buf))
            .ok_or_else(|| io_error("无法确定 Codex home"))?;
        let config: toml::Value = fs::read_to_string(home.join(format!("{profile}.config.toml")))?
            .parse()
            .map_err(|e| io_error(format!("桥接 profile TOML 无效：{e}")))?;
        flatten_config("", &config, &mut overrides)?;
        // app-server rejects the CLI's named-profile flag. Explicit, conservative
        // defaults prevent unrelated config.toml permissions from leaking into it.
        if config.get("sandbox_mode").is_none() {
            overrides.push("sandbox_mode=\"read-only\"".into());
        }
        if config.get("approval_policy").is_none() {
            overrides.push("approval_policy=\"on-request\"".into());
        }
    }
    let mut command = Command::new("node");
    command
        .args([
            "--input-type=module",
            "--eval",
            include_str!("copilot_adapter.mjs"),
        ])
        .env("MAGICODEX_BRIDGE_ROOT", root)
        .env(
            "MAGICODEX_PROFILE_OVERRIDES",
            serde_json::to_string(&overrides)?,
        );
    Ok(Some(command))
}

fn flatten_config(prefix: &str, value: &toml::Value, out: &mut Vec<String>) -> io::Result<()> {
    if let Some(table) = value.as_table().filter(|table| !table.is_empty()) {
        for (key, child) in table {
            if key.is_empty()
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            {
                return Err(io_error(
                    "桥接 profile 包含无法无损表达为 Codex -c 路径的键",
                ));
            }
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            flatten_config(&path, child, out)?;
        }
    } else if !prefix.is_empty() {
        out.push(format!("{prefix}={value}"));
    }
    Ok(())
}

impl Backend {
    pub fn start(kind: BackendKind, entry: Option<&Path>, cwd: &Path) -> io::Result<Self> {
        let path = match entry {
            Some(path) => path.to_path_buf(),
            None => find_entry(kind)?,
        };
        let adapter = if kind == BackendKind::Copilot && entry.is_none() {
            let adapter = bridge_adapter(&path)?;
            if adapter.is_none() && env::var_os("MAGICODEX_COPILOT_ENTRY").is_none() {
                return Err(io_error(
                    "默认 codex 入口不是可识别的 Copilot 桥接。请明确指定 MAGICODEX_COPILOT_ENTRY/--codex，或选择 --backend official；不会自动切换账号来源。",
                ));
            }
            adapter
        } else {
            None
        };
        let mut command = match adapter {
            Some(command) => command,
            None => {
                let mut command = entry_command(&path)?;
                command.args(["app-server", "--stdio"]);
                command
            }
        };
        command
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // A suspended child cannot spawn descendants before joining our private job.
            command.creation_flags(0x00000004 | 0x00000200 | 0x08000000);
        }
        let mut child = command.spawn()?;
        #[cfg(windows)]
        let job = match job::Job::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                child.kill().map_err(|cleanup| {
                    io_error(format!(
                        "无法绑定后端进程：{error}；无法终止 PID {}：{cleanup}",
                        child.id()
                    ))
                })?;
                child.wait().map_err(|cleanup| {
                    io_error(format!(
                        "无法绑定后端进程：{error}；无法等待退出：{cleanup}"
                    ))
                })?;
                return Err(error);
            }
        };
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io_error("后端 stdin 不可用"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io_error("后端 stdout 不可用"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io_error("后端 stderr 不可用"))?;
        let (tx, rx) = mpsc::sync_channel(4);
        let (write_tx, write_rx) = mpsc::sync_channel::<Value>(8);
        let writer_events = tx.clone();
        thread::spawn(move || {
            for value in write_rx {
                let result = (|| -> io::Result<()> {
                    serde_json::to_writer(&mut stdin, &value)?;
                    stdin.write_all(b"\n")?;
                    stdin.flush()
                })();
                if let Err(error) = result {
                    let _ = writer_events.send(BackendEvent::Disconnected(format!(
                        "后端写入失败：{error}。请求可能已部分发送，不会自动重试。"
                    )));
                    break;
                }
            }
        });
        let output_tx = tx.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let event = match protocol::read_message(&mut reader) {
                    Ok(Some(message)) => BackendEvent::Message(message),
                    Ok(None) => {
                        let _ = output_tx.send(BackendEvent::Disconnected("后端已关闭连接".into()));
                        break;
                    }

                    Err(error) => {
                        let _ = output_tx.send(BackendEvent::Disconnected(error));
                        break;
                    }
                };
                if output_tx.send(event).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || drain_diagnostics(stderr, tx));
        Ok(Self {
            child,
            outbound: Some(write_tx),
            rx,
            next_id: 1,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub fn send(&mut self, value: &Value) -> io::Result<()> {
        let outbound = self
            .outbound
            .as_ref()
            .ok_or_else(|| io_error("连接已关闭"))?;
        outbound
            .try_send(value.clone())
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    io_error("后端写入队列繁忙，此请求未入队；请稍后重试")
                }
                mpsc::TrySendError::Disconnected(_) => io_error("后端写入线程已退出"),
            })
    }

    pub fn request(&mut self, method: &str, params: Value) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&protocol::request(id, method, params))?;
        Ok(id)
    }

    pub fn try_recv(&self) -> Result<BackendEvent, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        self.outbound.take();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            while self.rx.try_recv().is_ok() {}
            if self.child.try_wait()?.is_some() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        #[cfg(windows)]
        self._job.terminate()?;
        #[cfg(not(windows))]
        self.child.kill()?;
        self.child.wait()?;
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Magicodex 后端清理失败：{error}");
        }
    }
}

fn drain_diagnostics(stderr: impl io::Read, tx: SyncSender<BackendEvent>) {
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        match reader.fill_buf() {
            Ok([]) => break,
            Ok(chunk) => {
                let count = chunk
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(chunk.len(), |n| n + 1);
                let remaining = 4096usize.saturating_sub(line.len());
                line.extend_from_slice(&chunk[..count.min(remaining)]);
                let end = chunk[count - 1] == b'\n';
                reader.consume(count);
                if end {
                    let text = String::from_utf8_lossy(&line).into_owned();
                    // Diagnostics are bounded and separate from authoritative protocol events.
                    if tx.send(BackendEvent::Diagnostic(text)).is_err() {
                        break;
                    }
                    line.clear();
                }
            }
            Err(error) => {
                let _ = tx.send(BackendEvent::Diagnostic(format!(
                    "stderr 读取失败：{error}"
                )));
                break;
            }
        }
    }
}

#[cfg(windows)]
mod job {
    use std::{io, mem, os::windows::io::AsRawHandle, process::Child};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };

    pub struct Job(HANDLE);

    impl Job {
        pub fn attach(child: &Child) -> io::Result<Self> {
            // The handle is owned here and never shared with an unrelated process.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let job = Self(handle);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    mem::size_of_val(&info) as u32,
                ) == 0
                    || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
                {
                    return Err(io::Error::last_os_error());
                }
                resume_primary_thread(child.id())?;
                Ok(job)
            }
        }

        pub fn terminate(&self) -> io::Result<()> {
            if unsafe { TerminateJobObject(self.0, 1) } == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    fn resume_primary_thread(pid: u32) -> io::Result<()> {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let mut entry: THREADENTRY32 = mem::zeroed();
            entry.dwSize = mem::size_of_val(&entry) as u32;
            let mut found = Thread32First(snapshot, &mut entry) != 0;
            let mut result = Err(io::Error::other("找不到后端主线程"));
            while found {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if thread.is_null() {
                        result = Err(io::Error::last_os_error());
                    } else {
                        result = if ResumeThread(thread) == u32::MAX {
                            Err(io::Error::last_os_error())
                        } else {
                            Ok(())
                        };
                        CloseHandle(thread);
                    }
                    break;
                }
                found = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            result
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_translation_preserves_toml_types() {
        let value: toml::Value =
            "model='a'\n[features]\nmulti_agent=false\n[provider]\npaths=['a','b']"
                .parse()
                .unwrap();
        let mut overrides = Vec::new();
        flatten_config("", &value, &mut overrides).unwrap();
        let mut merged = String::new();
        for entry in &overrides {
            merged.push_str(entry);
            merged.push('\n');
        }
        assert_eq!(merged.parse::<toml::Value>().unwrap(), value);
        let unsupported: toml::Value = "['key.with.dot']\nvalue=1".parse().unwrap();
        assert!(flatten_config("", &unsupported, &mut Vec::new()).is_err());
    }
}
