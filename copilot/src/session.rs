//! Follows the Copilot CLI session log (`session-state/<id>/events.jsonl`) of the child.
//!
//! Copilot appends one JSON event per line as the session runs. Only public text is used: the
//! user's prompt and the assistant's messages. Reasoning fields are never read.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use serde_json::Value;

use crate::circle::sides::spells;

const SCAN_INTERVAL: Duration = Duration::from_millis(1500);
const MAX_READ: u64 = 8 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// A prompt the user submitted.
    Prompt(String),
    /// A complete assistant message; `tools` when it also asked to run tools.
    Reply {
        text: String,
        tools: bool,
    },
    TurnStart,
    TurnEnd,
    /// A permission or input request is waiting (`true`) or was answered (`false`).
    Attention(bool),
    Idle,
    Aborted,
    /// The session ended or the child switched to another session.
    Reset,
    /// A tool call started; `nested` when a subagent made it.
    Spell {
        id: String,
        tool: String,
        detail: String,
        mcp: bool,
        nested: bool,
    },
    /// A tool call or a subagent finished.
    SpellDone {
        id: String,
        ok: bool,
    },
    /// A subagent started for the call `id`.
    Summon {
        id: String,
        name: String,
    },
    /// A skill was invoked.
    Tome(String),
    /// What Copilot says it is doing, as its status line shows.
    Intent(String),
}

pub(crate) fn parse_event(line: &[u8]) -> Option<Event> {
    let event: Value = serde_json::from_slice(line).ok()?;
    let data = event.get("data");
    let text = |key: &str| {
        data.and_then(|data| data.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    match event.get("type")?.as_str()? {
        "user.message" => {
            let continuation = data
                .and_then(|data| data.get("isAutopilotContinuation"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let source = text("source");
            let prompt = text("content");
            let generated = continuation || matches!(source.as_str(), "autopilot" | "system");
            (!generated && !prompt.trim().is_empty()).then_some(Event::Prompt(prompt))
        }
        "assistant.message" => {
            let tools = data
                .and_then(|data| data.get("toolRequests"))
                .and_then(Value::as_array)
                .is_some_and(|requests| !requests.is_empty());
            Some(Event::Reply {
                text: text("content"),
                tools,
            })
        }
        "assistant.turn_start" => Some(Event::TurnStart),
        "assistant.turn_end" => Some(Event::TurnEnd),
        "permission.requested" | "elicitation.requested" => Some(Event::Attention(true)),
        "permission.completed" | "elicitation.completed" => Some(Event::Attention(false)),
        "session.idle" => Some(Event::Idle),
        "session.abort" | "session.error" => Some(Event::Aborted),
        "session.shutdown" => Some(Event::Reset),
        "tool.execution_start" => {
            let (id, tool) = (text("toolCallId"), text("toolName"));
            if id.is_empty() || tool.is_empty() {
                return None;
            }
            let arguments = data.and_then(|data| data.get("arguments"));
            if tool == "report_intent" {
                return spells::intent(arguments).map(Event::Intent);
            }
            let mcp = !text("mcpServerName").is_empty();
            Some(Event::Spell {
                detail: spells::detail(&tool, mcp, arguments),
                nested: !text("parentToolCallId").is_empty(),
                id,
                tool,
                mcp,
            })
        }
        "tool.execution_complete" | "subagent.completed" | "subagent.failed" => {
            let id = text("toolCallId");
            let ok = event.get("type")?.as_str()? != "subagent.failed"
                && data
                    .and_then(|data| data.get("success"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
            (!id.is_empty()).then_some(Event::SpellDone { id, ok })
        }
        "subagent.started" => {
            let name = [text("agentDisplayName"), text("agentName")]
                .into_iter()
                .find(|name| !name.is_empty())
                .unwrap_or_default();
            Some(Event::Summon {
                id: text("toolCallId"),
                name,
            })
        }
        "skill.invoked" => {
            let name = text("name");
            (!name.is_empty()).then_some(Event::Tome(name))
        }
        _ => None,
    }
}

struct Tail {
    id: String,
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

impl Tail {
    fn read(&mut self) -> Vec<Event> {
        let Ok(mut file) = File::open(&self.path) else {
            return Vec::new();
        };
        let Ok(length) = file.metadata().map(|meta| meta.len()) else {
            return Vec::new();
        };
        if length < self.offset {
            // Rewritten from the start; only follow what comes next.
            self.offset = length;
            self.partial.clear();
        }
        if length == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut chunk = Vec::new();
        let Ok(read) = file.by_ref().take(MAX_READ).read_to_end(&mut chunk) else {
            return Vec::new();
        };
        self.offset += read as u64;
        self.partial.extend_from_slice(&chunk);
        let mut events = Vec::new();
        let mut start = 0;
        while let Some(end) = self.partial[start..].iter().position(|byte| *byte == b'\n') {
            let line = &self.partial[start..start + end];
            events.extend(parse_event(line));
            start += end + 1;
        }
        self.partial.drain(..start);
        events
    }
}

/// Finds the session the child is using and yields its new events.
pub(crate) struct Tracker {
    root: PathBuf,
    child: u32,
    launched: SystemTime,
    preferred: Option<String>,
    current: Option<Tail>,
    next_scan: Instant,
}

impl Tracker {
    /// `preferred` is the id passed with `--session-id`; its log is read from the start.
    pub(crate) fn new(root: PathBuf, child: u32, preferred: Option<String>) -> Self {
        let current = preferred.as_ref().map(|id| Tail {
            id: id.clone(),
            path: root.join(id).join("events.jsonl"),
            offset: 0,
            partial: Vec::new(),
        });
        Self {
            root,
            child,
            launched: SystemTime::now() - Duration::from_secs(2),
            preferred,
            current,
            next_scan: Instant::now() + SCAN_INTERVAL,
        }
    }

    pub(crate) fn poll(&mut self, now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        if now >= self.next_scan {
            self.next_scan = now + SCAN_INTERVAL;
            if let Some((id, dir)) = self.active_session()
                && self.current.as_ref().is_none_or(|tail| tail.id != id)
            {
                let path = dir.join("events.jsonl");
                // A session that existed before launch was resumed: skip its history.
                let fresh = self.preferred.as_deref() == Some(id.as_str())
                    || std::fs::metadata(&dir)
                        .and_then(|meta| meta.created())
                        .is_ok_and(|created| created >= self.launched);
                let offset = if fresh {
                    0
                } else {
                    std::fs::metadata(&path).map_or(0, |meta| meta.len())
                };
                if self.current.is_some() {
                    events.push(Event::Reset);
                }
                crate::app::log(format!("following session {id} from byte {offset}"));
                self.current = Some(Tail {
                    id,
                    path,
                    offset,
                    partial: Vec::new(),
                });
            }
        }
        if let Some(tail) = self.current.as_mut() {
            events.extend(tail.read());
        }
        events
    }

    /// The session whose `inuse.<pid>.lock` belongs to the child's process tree.
    fn active_session(&self) -> Option<(String, PathBuf)> {
        let family = process_family(self.child);
        let mut best: Option<(SystemTime, String, PathBuf)> = None;
        for entry in std::fs::read_dir(&self.root).ok()?.flatten() {
            let dir = entry.path();
            // Directory listings can carry stale times on NTFS; ask the directory itself.
            let recent = std::fs::metadata(&dir)
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| modified >= self.launched);
            if !recent || !dir.is_dir() {
                continue;
            }
            for lock in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let name = lock.file_name().to_string_lossy().into_owned();
                let owner = name
                    .strip_prefix("inuse.")
                    .and_then(|rest| rest.strip_suffix(".lock"))
                    .and_then(|pid| pid.parse::<u32>().ok());
                if owner.is_some_and(|pid| family.contains(&pid)) {
                    let stamp = lock
                        .metadata()
                        .and_then(|meta| meta.modified())
                        .unwrap_or(SystemTime::UNIX_EPOCH);
                    if best.as_ref().is_none_or(|(newest, _, _)| stamp > *newest) {
                        let id = entry.file_name().to_string_lossy().into_owned();
                        best = Some((stamp, id, dir.clone()));
                    }
                }
            }
        }
        best.map(|(_, id, dir)| (id, dir))
    }
}

/// `root` and all of its descendants.
#[cfg(windows)]
fn process_family(root: u32) -> HashSet<u32> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::CreateToolhelp32Snapshot;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::Process32FirstW;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::Process32NextW;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::TH32CS_SNAPPROCESS;

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot != INVALID_HANDLE_VALUE {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                children
                    .entry(entry.th32ParentProcessID)
                    .or_default()
                    .push(entry.th32ProcessID);
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
    }
    let mut family = HashSet::from([root]);
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if family.insert(*child) {
                queue.push(*child);
            }
        }
    }
    family
}

#[cfg(not(windows))]
fn process_family(root: u32) -> HashSet<u32> {
    HashSet::from([root])
}

/// The Copilot state directory the child uses.
pub(crate) fn copilot_home(env: &[(std::ffi::OsString, std::ffi::OsString)]) -> PathBuf {
    let lookup = |name: &str| {
        env.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| PathBuf::from(value))
    };
    lookup("COPILOT_HOME").unwrap_or_else(|| {
        lookup("USERPROFILE")
            .or_else(|| lookup("HOME"))
            .unwrap_or_default()
            .join(".copilot")
    })
}

pub(crate) fn session_root(home: &Path) -> PathBuf {
    home.join("session-state")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn events_keep_public_text_only() {
        let line = br#"{"type":"assistant.message","data":{"content":"Checking files","reasoningText":"SECRET","toolRequests":[{"name":"glob"}]},"id":"1"}"#;
        assert_eq!(
            parse_event(line),
            Some(Event::Reply {
                text: "Checking files".into(),
                tools: true
            })
        );
        let prompt = br#"{"type":"user.message","data":{"content":"Fix it","transformedContent":"<x>Fix it"}}"#;
        assert_eq!(parse_event(prompt), Some(Event::Prompt("Fix it".into())));
        let continuation =
            br#"{"type":"user.message","data":{"content":"go on","isAutopilotContinuation":true}}"#;
        assert_eq!(parse_event(continuation), None);
        assert_eq!(
            parse_event(br#"{"type":"assistant.reasoning","data":{"content":"SECRET"}}"#),
            None
        );
        assert_eq!(parse_event(b"not json"), None);
        assert_eq!(
            parse_event(br#"{"type":"permission.requested","data":{}}"#),
            Some(Event::Attention(true))
        );
    }

    #[test]
    fn tool_subagent_and_skill_events_become_spells() {
        // As Copilot 1.0.89 writes them to events.jsonl.
        let start = br#"{"type":"tool.execution_start","data":{"toolCallId":"call_1","toolName":"glob","arguments":{"pattern":"*.md"},"toolTitle":"Finding files"}}"#;
        assert_eq!(
            parse_event(start),
            Some(Event::Spell {
                id: "call_1".into(),
                tool: "glob".into(),
                detail: "*.md".into(),
                mcp: false,
                nested: false,
            })
        );
        let done =
            br#"{"type":"tool.execution_complete","data":{"toolCallId":"call_1","success":false}}"#;
        assert_eq!(
            parse_event(done),
            Some(Event::SpellDone {
                id: "call_1".into(),
                ok: false
            })
        );
        let nested = br#"{"type":"tool.execution_start","data":{"toolCallId":"c2","toolName":"search","mcpServerName":"github","parentToolCallId":"c1"}}"#;
        assert!(matches!(
            parse_event(nested),
            Some(Event::Spell {
                mcp: true,
                nested: true,
                ..
            })
        ));
        let summon = br#"{"type":"subagent.started","data":{"toolCallId":"c1","agentName":"explore","agentDisplayName":"Explore","agentDescription":"x"}}"#;
        assert_eq!(
            parse_event(summon),
            Some(Event::Summon {
                id: "c1".into(),
                name: "Explore".into()
            })
        );
        let failed = br#"{"type":"subagent.failed","data":{"toolCallId":"c1"}}"#;
        assert_eq!(
            parse_event(failed),
            Some(Event::SpellDone {
                id: "c1".into(),
                ok: false
            })
        );
        let skill = br#"{"type":"skill.invoked","data":{"name":"azure-image-gen","content":"SKILL BODY","path":"x"}}"#;
        assert_eq!(
            parse_event(skill),
            Some(Event::Tome("azure-image-gen".into()))
        );
        let intent = br#"{"type":"tool.execution_start","data":{"toolCallId":"c3","toolName":"report_intent","arguments":{"intent":"Exploring codebase"}}}"#;
        assert_eq!(
            parse_event(intent),
            Some(Event::Intent("Exploring codebase".into()))
        );
        assert_eq!(
            parse_event(br#"{"type":"tool.execution_start","data":{"toolName":"glob"}}"#),
            None
        );
    }

    #[test]
    fn the_preferred_session_is_followed_from_the_start_across_partial_writes() {
        let root = std::env::temp_dir().join(format!("magicopilot-tail-{}", std::process::id()));
        let dir = root.join("id-1");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let mut file = File::create(&path).unwrap();
        let mut tracker = Tracker::new(root.clone(), std::process::id(), Some("id-1".into()));
        write!(file, "{{\"type\":\"user.message\",\"data\":{{\"content\":\"hi\"}}}}\n{{\"type\":\"assistant.turn_").unwrap();
        file.flush().unwrap();
        let now = Instant::now();
        assert_eq!(tracker.poll(now), vec![Event::Prompt("hi".into())]);
        writeln!(file, "start\",\"data\":{{}}}}").unwrap();
        file.flush().unwrap();
        assert_eq!(tracker.poll(now), vec![Event::TurnStart]);
        assert_eq!(tracker.poll(now), Vec::new());
        drop(file);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_resumed_session_skips_its_history() {
        let root = std::env::temp_dir().join(format!("magicopilot-resume-{}", std::process::id()));
        let dir = root.join("old");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"user.message\",\"data\":{\"content\":\"old prompt\"}}\n",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(60));
        let mut tracker = Tracker::new(root.clone(), std::process::id(), None);
        tracker.launched = SystemTime::now();
        std::thread::sleep(Duration::from_millis(60));
        // Resuming takes the lock, which marks the directory as recently used.
        std::fs::write(dir.join(format!("inuse.{}.lock", std::process::id())), "").unwrap();
        assert_eq!(tracker.poll(Instant::now() + SCAN_INTERVAL), Vec::new());
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(
            file,
            "{{\"type\":\"user.message\",\"data\":{{\"content\":\"new prompt\"}}}}"
        )
        .unwrap();
        drop(file);
        assert_eq!(
            tracker.poll(Instant::now()),
            vec![Event::Prompt("new prompt".into())]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
