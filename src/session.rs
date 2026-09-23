use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};

const MAX_ENTRIES: usize = 256;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_ITEM_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Ready,
    Running,
    Interrupting,
    Completed,
    Interrupted,
    Failed,
    Disconnected,
}

impl Status {
    pub fn busy(self) -> bool {
        matches!(self, Self::Connecting | Self::Running | Self::Interrupting)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "连接中",
            Self::Ready => "准备就绪",
            Self::Running => "施法中 · 等待真实事件",
            Self::Interrupting => "正在请求中断",
            Self::Completed => "回合已完成",
            Self::Interrupted => "已中断",
            Self::Failed => "失败",
            Self::Disconnected => "已断开 · 后端任务状态需确认",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub turn_id: String,
    pub kind: String,
    pub title: String,
    pub text: String,
    pub phase: Option<String>,
    pub status: String,
    pub parts: BTreeMap<(bool, u64), String>,
    pub truncated: bool,
}

impl Item {
    fn new(id: &str, turn: &str, kind: &str) -> Self {
        Self {
            id: id.into(),
            turn_id: turn.into(),
            kind: kind.into(),
            title: kind.into(),
            text: String::new(),
            phase: None,
            status: "inProgress".into(),
            parts: BTreeMap::new(),
            truncated: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Approval {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

impl Approval {
    pub fn is_input(&self) -> bool {
        self.method == "item/tool/requestUserInput"
    }

    pub fn response(&self, allow: bool) -> Result<Value, String> {
        use serde_json::json;
        match self.method.as_str() {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                let choices = self
                    .params
                    .get("availableDecisions")
                    .and_then(Value::as_array);
                let decision = if allow {
                    "accept"
                } else if choices.is_some_and(|choices| {
                    !choices.iter().any(|c| c.as_str() == Some("decline"))
                        && choices.iter().any(|c| c.as_str() == Some("cancel"))
                }) {
                    "cancel"
                } else {
                    "decline"
                };
                if let Some(choices) = self
                    .params
                    .get("availableDecisions")
                    .and_then(Value::as_array)
                    && !choices
                        .iter()
                        .any(|choice| choice.as_str() == Some(decision))
                {
                    return Err(format!("后端没有提供 {decision} 选项；可按 Esc 中断"));
                }
                Ok(json!({"decision": decision}))
            }
            "item/permissions/requestApproval" => {
                let permissions = self
                    .params
                    .get("permissions")
                    .filter(|v| v.is_object())
                    .ok_or("权限请求缺少 permissions")?;
                Ok(
                    json!({"permissions": if allow { permissions.clone() } else { json!({}) }, "scope":"turn"}),
                )
            }
            "execCommandApproval" | "applyPatchApproval" => {
                Ok(json!({"decision": if allow { "approved" } else { "denied" }}))
            }
            _ => Err("此请求不是可直接允许/拒绝的审批".into()),
        }
    }
}

pub struct Session {
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub turn_active: bool,
    pub status: Status,
    pub model: String,
    pub policy: String,
    pub items: VecDeque<Item>,
    pub approvals: VecDeque<Approval>,
    pub prompt: String,
    pub notice: String,
    pub trimmed: bool,
    pub revision: u64,
    pub completion_serial: u64,
    serial: u64,
    unknown_methods: Vec<String>,
    closed_turns: VecDeque<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            thread_id: None,
            turn_id: None,
            turn_active: false,
            status: Status::Connecting,
            model: "后端默认".into(),
            policy: String::new(),
            items: VecDeque::new(),
            approvals: VecDeque::new(),
            prompt: String::new(),
            notice: String::new(),
            trimmed: false,
            revision: 0,
            completion_serial: 0,
            serial: 0,
            unknown_methods: Vec::new(),
            closed_turns: VecDeque::new(),
        }
    }
}

fn text_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("协议缺少字符串字段 {key}"))
}

fn bounded(text: &mut String, limit: usize) -> bool {
    if text.len() <= limit {
        return false;
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text.drain(..start);
    true
}

impl Session {
    pub fn system(&mut self, text: impl Into<String>) {
        self.serial += 1;
        let mut item = Item::new(&format!("local-{}", self.serial), "", "system");
        item.title = "Magicodex".into();
        item.text = text.into();
        item.status = "completed".into();
        self.items.push_back(item);
        self.revision += 1;
        self.enforce_limits();
    }

    pub fn fail(&mut self, text: impl Into<String>) {
        self.notice = text.into();
        self.status = Status::Failed;
        self.system(format!("错误：{}", self.notice));
    }

    pub fn submitted(&mut self, prompt: String) {
        self.prompt = prompt;
        self.notice.clear();
        self.status = Status::Running;
        self.turn_active = true;
        self.turn_id = None;
        self.revision += 1;
    }

    fn item(&mut self, id: &str, turn: &str, kind: &str) -> &mut Item {
        if let Some(index) = self
            .items
            .iter()
            .position(|item| item.id == id && item.turn_id == turn)
        {
            return &mut self.items[index];
        }
        self.items.push_back(Item::new(id, turn, kind));
        self.items.back_mut().expect("just inserted")
    }

    pub fn add_request(&mut self, request: Approval) -> Result<(), String> {
        if self.approvals.iter().any(|a| a.id == request.id) {
            return Err("收到重复的待处理请求 ID".into());
        }
        if self.approvals.len() >= 32 {
            return Err("待审批请求过多，已停止接受新的请求；请中断回合".into());
        }
        self.approvals.push_back(request);
        self.revision += 1;
        Ok(())
    }

    pub fn notification(&mut self, method: &str, params: &Value) -> Result<(), String> {
        if let (Some(expected), Some(actual)) = (&self.thread_id, params["threadId"].as_str())
            && expected != actual
        {
            return Ok(());
        }
        match method {
            "turn/started" => {
                let id = text_field(&params["turn"], "id")?;
                if self.closed_turns.iter().any(|closed| closed == id) {
                    return Ok(());
                }
                self.turn_id = Some(id.into());
                self.turn_active = true;
                if self.status != Status::Interrupting {
                    self.status = Status::Running;
                }
            }
            "turn/completed" => {
                let turn = &params["turn"];
                let id = text_field(turn, "id")?;
                if self.closed_turns.iter().any(|closed| closed == id) {
                    return Ok(());
                }
                if self.turn_id.as_deref().is_some_and(|current| current != id) {
                    return Ok(());
                }
                if let Some(items) = turn["items"].as_array() {
                    for item in items {
                        self.merge_item(id, item, true)?;
                    }
                }
                let old = self.status;
                self.status = match text_field(turn, "status")? {
                    "completed" => Status::Completed,
                    "interrupted" => Status::Interrupted,
                    "failed" => Status::Failed,
                    other => return Err(format!("不支持的回合结束状态：{other}")),
                };
                self.turn_id = Some(id.into());
                self.turn_active = false;
                self.closed_turns.push_back(id.into());
                if self.closed_turns.len() > 64 {
                    self.closed_turns.pop_front();
                }
                if self.status == Status::Completed && old != Status::Completed {
                    self.completion_serial += 1;
                }
                if let Some(message) = turn["error"]["message"].as_str() {
                    self.notice = message.into();
                    self.system(format!("回合错误：{message}"));
                }
                self.approvals.retain(|a| {
                    a.params["turnId"].as_str() != Some(id)
                        || (a.is_input() && a.params["isBlocking"].as_bool() == Some(false))
                });
            }
            "item/started" | "item/completed" => {
                self.merge_item(
                    text_field(params, "turnId")?,
                    &params["item"],
                    method == "item/completed",
                )?;
            }
            "item/agentMessage/delta"
            | "item/plan/delta"
            | "item/commandExecution/outputDelta"
            | "item/fileChange/outputDelta"
            | "item/reasoning/summaryTextDelta"
            | "item/reasoning/textDelta" => {
                let id = text_field(params, "itemId")?;
                let turn = text_field(params, "turnId")?;
                let delta = text_field(params, "delta")?;
                let kind = method.split('/').nth(1).ok_or("无效 delta 方法")?;
                let item = self.item(id, turn, kind);
                if kind == "reasoning" {
                    let summary = method.contains("summary");
                    let index = params[if summary {
                        "summaryIndex"
                    } else {
                        "contentIndex"
                    }]
                    .as_u64()
                    .unwrap_or(0);
                    item.parts
                        .entry((summary, index))
                        .or_default()
                        .push_str(delta);
                    let has_summary = item.parts.keys().any(|(s, _)| *s);
                    item.text = item
                        .parts
                        .iter()
                        .filter(|((s, _), _)| *s == has_summary)
                        .map(|(_, text)| text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                } else {
                    item.text.push_str(delta);
                }
            }
            "turn/diff/updated" => {
                let turn = text_field(params, "turnId")?;
                self.item("turn-diff", turn, "diff").text = text_field(params, "diff")?.into();
            }
            "turn/plan/updated" => {
                let turn = text_field(params, "turnId")?;
                self.item("turn-plan", turn, "plan").text =
                    serde_json::to_string_pretty(&params["plan"]).map_err(|e| e.to_string())?;
            }
            "item/mcpToolCall/progress" => {
                let turn = text_field(params, "turnId")?;
                let item = self.item(text_field(params, "itemId")?, turn, "mcpToolCall");
                item.text.push_str(text_field(params, "message")?);
                item.text.push('\n');
            }
            "serverRequest/resolved" => {
                let id = params.get("requestId").ok_or("撤销请求缺少 requestId")?;
                self.approvals.retain(|a| &a.id != id);
            }
            "mcpServer/startupStatus/updated" => {
                let name = text_field(params, "name")?;
                let status = text_field(params, "status")?;
                self.system(format!("MCP {name}：{status}"));
                if let Some(error) = params["error"].as_str() {
                    self.notice = format!("MCP {name}：{error}");
                    self.system(self.notice.clone());
                }
            }
            "error" => {
                let message = params["error"]["message"]
                    .as_str()
                    .or_else(|| params["message"].as_str())
                    .unwrap_or("后端发送了没有说明的错误");
                self.notice = message.into();
                self.system(format!("后端错误：{message}"));
                if params["willRetry"].as_bool() != Some(true) {
                    self.status = Status::Failed;
                }
            }
            "warning" | "configWarning" | "deprecationNotice" => {
                self.system(format!(
                    "{method}：{}",
                    params["message"]
                        .as_str()
                        .or_else(|| params["summary"].as_str())
                        .unwrap_or("查看后端诊断")
                ));
            }
            "thread/started"
            | "thread/status/changed"
            | "thread/tokenUsage/updated"
            | "item/reasoning/summaryPartAdded"
            | "account/updated"
            | "account/rateLimits/updated"
            | "remoteControl/status/changed" => {}
            _ => {
                if !self.unknown_methods.iter().any(|m| m == method)
                    && self.unknown_methods.len() < 32
                {
                    self.system(format!("未可视化的通知：{method}（不影响已支持的事件）"));
                    self.unknown_methods.push(method.into());
                }
            }
        }
        self.revision += 1;
        self.enforce_limits();
        Ok(())
    }

    fn merge_item(&mut self, turn: &str, value: &Value, completed: bool) -> Result<(), String> {
        let id = text_field(value, "id")?;
        let kind = text_field(value, "type")?;
        let item = self.item(id, turn, kind);
        item.kind = kind.into();
        item.status = value["status"]
            .as_str()
            .unwrap_or(if completed { "completed" } else { "inProgress" })
            .into();
        if let Some(phase) = value["phase"].as_str() {
            item.phase = Some(phase.into());
        }
        match kind {
            "agentMessage" | "plan" => {
                if let Some(text) = value["text"].as_str() {
                    item.text = text.into();
                }
            }
            "userMessage" => {
                item.text = value["content"]
                    .as_array()
                    .ok_or("userMessage 缺少 content")?
                    .iter()
                    .filter_map(|c| c["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            "reasoning" => {
                let summary = value["summary"].as_array();
                let content = value["content"].as_array();
                let parts = summary.filter(|s| !s.is_empty()).or(content);
                if let Some(parts) = parts {
                    item.text = parts
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n");
                }
                if completed {
                    item.parts.clear();
                }
            }
            "commandExecution" => {
                item.title = format!(
                    "命令：{}\n目录：{}\n退出码：{}",
                    value["command"].as_str().unwrap_or("未提供"),
                    value["cwd"].as_str().unwrap_or("未提供"),
                    value["exitCode"]
                );
                if let Some(output) = value["aggregatedOutput"].as_str() {
                    item.text = output.into();
                }
            }
            "fileChange" => {
                item.title = "文件修改".into();
                item.text =
                    serde_json::to_string_pretty(&value["changes"]).map_err(|e| e.to_string())?;
            }
            "mcpToolCall" => {
                item.title = format!(
                    "MCP {} · {}",
                    value["server"].as_str().unwrap_or("?"),
                    value["tool"].as_str().unwrap_or("?")
                );
                if completed {
                    item.text = serde_json::to_string_pretty(
                        &serde_json::json!({"result":value["result"],"error":value["error"]}),
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            _ => item.text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?,
        }
        Ok(())
    }

    fn enforce_limits(&mut self) {
        for item in &mut self.items {
            item.truncated |= bounded(&mut item.text, MAX_ITEM_BYTES);
            self.trimmed |= item.truncated;
            for part in item.parts.values_mut() {
                self.trimmed |= bounded(part, MAX_ITEM_BYTES);
            }
            if item.parts.len() > 64 {
                item.parts.clear();
                self.trimmed = true;
            }
        }
        while self.items.len() > MAX_ENTRIES
            || self
                .items
                .iter()
                .map(|i| {
                    i.text.len() + i.title.len() + i.parts.values().map(String::len).sum::<usize>()
                })
                .sum::<usize>()
                > MAX_TEXT_BYTES
        {
            self.items.pop_front();
            self.trimmed = true;
        }
    }

    pub fn has_final_answer(&self) -> bool {
        self.items.iter().any(|item| {
            item.kind == "agentMessage"
                && item.phase.as_deref() == Some("final_answer")
                && Some(item.turn_id.as_str()) == self.turn_id.as_deref()
        })
    }

    pub fn answer(&self) -> String {
        let has_final_phase = self.has_final_answer();
        self.items
            .iter()
            .filter(|i| {
                i.kind == "agentMessage"
                    && (i.phase.as_deref() == Some("final_answer")
                        || (!has_final_phase
                            && self.status == Status::Completed
                            && i.phase.is_none()))
                    && Some(i.turn_id.as_str()) == self.turn_id.as_deref()
            })
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn transcript(&self) -> String {
        let mut result = if self.trimmed {
            "[内存视图已截断；Ctrl+S 请求后端导出，不把此视图当作完整记录]\n\n".into()
        } else {
            String::new()
        };
        for item in &self.items {
            result.push_str(&format!(
                "── {} [{}] ──\n{}\n\n",
                item.title, item.status, item.text
            ));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn final_item_reconciles_delta_without_duplication() {
        let mut session = Session {
            turn_id: Some("t".into()),
            ..Session::default()
        };
        session
            .notification(
                "item/agentMessage/delta",
                &json!({"itemId":"a","turnId":"t","delta":"魔"}),
            )
            .unwrap();
        session.notification("item/completed", &json!({"turnId":"t","item":{"id":"a","type":"agentMessage","text":"魔法","phase":"final_answer"}})).unwrap();
        assert_eq!(session.answer(), "魔法");
        assert_ne!(session.status, Status::Completed);
        session
            .notification(
                "turn/completed",
                &json!({"turn":{"id":"t","status":"completed","items":[]}}),
            )
            .unwrap();
        assert_eq!(session.completion_serial, 1);
        session
            .notification(
                "turn/completed",
                &json!({"turn":{"id":"t","status":"completed","items":[]}}),
            )
            .unwrap();
        assert_eq!(session.completion_serial, 1);
    }

    #[test]
    fn failures_and_interruptions_never_signal_success() {
        for (wire, expected) in [
            ("failed", Status::Failed),
            ("interrupted", Status::Interrupted),
        ] {
            let mut session = Session::default();
            session
                .notification("turn/completed", &json!({"turn":{"id":"t","status":wire}}))
                .unwrap();
            assert_eq!(session.status, expected);
            assert_eq!(session.completion_serial, 0);
        }
    }

    #[test]
    fn reasoning_parts_keep_order_and_prefer_summary() {
        let mut s = Session::default();
        for (method, index, text) in [
            ("item/reasoning/textDelta", 0, "公开内容"),
            ("item/reasoning/summaryTextDelta", 1, "后段"),
            ("item/reasoning/summaryTextDelta", 0, "前段"),
        ] {
            s.notification(method, &json!({"itemId":"r","turnId":"t","delta":text,"summaryIndex":index,"contentIndex":index})).unwrap();
        }
        assert_eq!(s.items[0].text, "前段\n后段");
    }

    #[test]
    fn approvals_are_explicit_and_scoped() {
        let request = Approval {
            id: json!("a"),
            method: "item/permissions/requestApproval".into(),
            params: json!({"permissions":{"network":{"enabled":true}}}),
        };
        assert_eq!(
            request.response(false).unwrap(),
            json!({"permissions":{},"scope":"turn"})
        );
        assert_eq!(request.response(true).unwrap()["scope"], "turn");
        let mut s = Session::default();
        s.add_request(request.clone()).unwrap();
        assert!(s.add_request(request).is_err());
        s.notification("serverRequest/resolved", &json!({"requestId":"a"}))
            .unwrap();
        assert!(s.approvals.is_empty());
    }

    #[test]
    fn memory_window_is_bounded_and_marked() {
        let mut s = Session::default();
        for _ in 0..300 {
            s.system("test");
        }
        assert!(s.trimmed);
        assert_eq!(s.items.len(), MAX_ENTRIES);
        assert!(s.transcript().contains("已截断"));
    }

    #[test]
    fn old_completion_cannot_finish_a_newly_submitted_turn() {
        let mut s = Session::default();
        s.notification(
            "turn/completed",
            &json!({"turn":{"id":"old","status":"completed"}}),
        )
        .unwrap();
        s.submitted("next".into());
        s.notification(
            "turn/completed",
            &json!({"turn":{"id":"old","status":"completed"}}),
        )
        .unwrap();
        assert_eq!(s.status, Status::Running);
        assert!(s.turn_active);
        assert_eq!(s.completion_serial, 1);
    }

    #[test]
    fn error_notification_does_not_falsely_end_the_turn() {
        let mut s = Session::default();
        s.submitted("test".into());
        s.notification("error", &json!({"message":"failed","willRetry":false}))
            .unwrap();
        assert!(s.turn_active);
        s.notification(
            "turn/completed",
            &json!({"turn":{"id":"t","status":"failed"}}),
        )
        .unwrap();
        assert!(!s.turn_active);
    }

    #[test]
    fn unknown_message_phase_is_not_guessed_before_completion() {
        let mut s = Session {
            turn_id: Some("t".into()),
            ..Session::default()
        };
        s.notification(
            "item/agentMessage/delta",
            &json!({"turnId":"t","itemId":"a","delta":"原文"}),
        )
        .unwrap();
        assert!(s.answer().is_empty());
        s.notification(
            "turn/completed",
            &json!({"turn":{"id":"t","status":"completed"}}),
        )
        .unwrap();
        assert_eq!(s.answer(), "原文");
    }

    #[test]
    fn asynchronous_question_remains_until_answered_or_resolved() {
        let mut s = Session::default();
        s.add_request(Approval {
            id: json!("q"),
            method: "item/tool/requestUserInput".into(),
            params: json!({"turnId":"t","isBlocking":false}),
        })
        .unwrap();
        s.notification(
            "turn/completed",
            &json!({"turn":{"id":"t","status":"completed"}}),
        )
        .unwrap();
        assert_eq!(s.approvals.len(), 1);
        s.notification("serverRequest/resolved", &json!({"requestId":"q"}))
            .unwrap();
        assert!(s.approvals.is_empty());
    }
}
