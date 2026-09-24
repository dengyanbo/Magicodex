//! Tool calls told as spells, and the record of what one turn has cast.

use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

use crate::magic_circle::display_text;

/// Spells kept per turn; the page shows the latest that fit.
const KEEP: usize = 32;
/// Graphemes kept from an argument; the page cuts it further to fit.
const DETAIL_LIMIT: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpellKind {
    Track,
    Insight,
    Inscribe,
    Ritual,
    Farsight,
    Summon,
    Tome,
    Pact,
    Memory,
    Query,
    Arcane,
}

impl SpellKind {
    /// The spell a tool casts, from the words of its name. MCP tools are pacts.
    pub(crate) fn of(tool: &str, mcp: bool) -> Self {
        if mcp {
            return Self::Pact;
        }
        let tool = tool.to_ascii_lowercase();
        let words: Vec<&str> = tool
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        let any = |set: &[&str]| words.iter().any(|word| set.contains(word));
        // Shells come before reading and writing: `read_powershell` reads a shell.
        if any(&["web", "fetch", "browse", "http", "url", "download"]) {
            Self::Farsight
        } else if any(&["agent", "agents", "task", "delegate", "subagent"]) {
            Self::Summon
        } else if any(&["skill", "skills"]) {
            Self::Tome
        } else if any(&["sql", "memory", "memories", "remember"]) {
            Self::Memory
        } else if any(&["ask"]) {
            Self::Query
        } else if any(&[
            "powershell",
            "pwsh",
            "bash",
            "sh",
            "shell",
            "exec",
            "terminal",
            "cmd",
            "command",
            "run",
        ]) {
            Self::Ritual
        } else if any(&[
            "glob", "grep", "rg", "search", "find", "locate", "lsp", "symbols",
        ]) {
            Self::Track
        } else if any(&["view", "read", "cat", "open", "show", "list", "ls", "get"]) {
            Self::Insight
        } else if any(&[
            "edit", "create", "write", "apply", "patch", "replace", "insert", "delete", "remove",
            "rename", "move", "update",
        ]) {
            Self::Inscribe
        } else {
            Self::Arcane
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Track => "寻踪术",
            Self::Insight => "洞察之眼",
            Self::Inscribe => "符文刻印",
            Self::Ritual => "召唤仪式",
            Self::Farsight => "千里眼",
            Self::Summon => "召唤使魔",
            Self::Tome => "翻阅秘典",
            Self::Pact => "契约之力",
            Self::Memory => "记忆水晶",
            Self::Query => "叩问之铃",
            Self::Arcane => "未名秘术",
        }
    }
}

/// Arguments as an object, also when they arrive as a JSON string.
fn object(arguments: Option<&Value>) -> Option<Value> {
    match arguments? {
        Value::String(text) => serde_json::from_str(text).ok(),
        other => Some(other.clone()),
    }
}

fn field(arguments: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| arguments.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

/// The argument that says most about a call: a search pattern, a file name, the first line
/// of a command, a URL, an agent or skill name. Other arguments are never shown.
pub(crate) fn detail(tool: &str, mcp: bool, arguments: Option<&Value>) -> String {
    let Some(arguments) = object(arguments) else {
        return String::new();
    };
    let text = match SpellKind::of(tool, mcp) {
        SpellKind::Track => field(&arguments, &["pattern", "query", "regex"]),
        SpellKind::Insight | SpellKind::Inscribe => {
            field(&arguments, &["path", "file_path", "filePath", "file"])
                .map(|path| file_name(&path))
        }
        SpellKind::Ritual => {
            field(&arguments, &["command", "cmd"]).map(|command| first_line(&command))
        }
        SpellKind::Farsight => field(&arguments, &["url", "query"]),
        SpellKind::Summon => field(
            &arguments,
            &["name", "agent_type", "agentType", "description"],
        ),
        SpellKind::Tome => field(&arguments, &["skill", "name"]),
        SpellKind::Memory => field(&arguments, &["description"]),
        SpellKind::Pact | SpellKind::Query | SpellKind::Arcane => None,
    };
    text.map(|text| clean(&text)).unwrap_or_default()
}

/// What a `report_intent` call announces; Copilot shows it as its status line.
// Copilot's session log reports intents; Codex has no such event.
#[allow(dead_code)]
pub(crate) fn intent(arguments: Option<&Value>) -> Option<String> {
    let intent = clean(&field(&object(arguments)?, &["intent"])?);
    (!intent.is_empty()).then_some(intent)
}

pub(crate) fn file_name(path: &str) -> String {
    let path = path.trim_end_matches(['/', '\\']);
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

pub(crate) fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// One printable line of at most [`DETAIL_LIMIT`] graphemes.
fn clean(text: &str) -> String {
    display_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .graphemes(/*is_extended*/ true)
        .take(DETAIL_LIMIT)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpellState {
    Casting,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Spell {
    pub(crate) id: String,
    pub(crate) kind: SpellKind,
    pub(crate) detail: String,
    pub(crate) state: SpellState,
}

/// What one turn has cast so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Chronicle {
    /// The latest top-level calls, oldest first.
    pub(crate) spells: Vec<Spell>,
    /// Top-level calls this turn, including those no longer kept.
    pub(crate) cast: usize,
    /// Calls made inside subagents, counted but not listed.
    pub(crate) helpers: usize,
    pub(crate) summons: usize,
    /// Public replies received this turn.
    pub(crate) oracles: usize,
    pub(crate) intent: Option<String>,
}

impl Chronicle {
    pub(crate) fn cast(&mut self, id: &str, tool: &str, detail: &str, mcp: bool, nested: bool) {
        if nested {
            self.helpers += 1;
            return;
        }
        if !id.is_empty() && self.spells.iter().any(|spell| spell.id == id) {
            return;
        }
        let kind = SpellKind::of(tool, mcp);
        let detail = if detail.is_empty() && kind == SpellKind::Arcane {
            clean(tool)
        } else {
            clean(detail)
        };
        self.push(Spell {
            id: id.to_string(),
            kind,
            detail,
            state: SpellState::Casting,
        });
    }

    fn push(&mut self, spell: Spell) {
        self.cast += 1;
        self.spells.push(spell);
        if self.spells.len() > KEEP {
            self.spells.remove(0);
        }
    }

    pub(crate) fn resolve(&mut self, id: &str, ok: bool) {
        if let Some(spell) = self.spells.iter_mut().find(|spell| spell.id == id) {
            spell.state = if ok {
                SpellState::Done
            } else {
                SpellState::Failed
            };
        }
    }

    /// A subagent started, usually by the call `id`, which it then names.
    pub(crate) fn summon(&mut self, id: &str, name: &str) {
        self.summons += 1;
        let name = clean(name);
        match self
            .spells
            .iter_mut()
            .find(|spell| !id.is_empty() && spell.id == id)
        {
            Some(spell) => {
                spell.kind = SpellKind::Summon;
                if !name.is_empty() {
                    spell.detail = name;
                }
            }
            None => self.push(Spell {
                id: id.to_string(),
                kind: SpellKind::Summon,
                detail: name,
                state: SpellState::Casting,
            }),
        }
    }

    /// A skill was invoked; one already listed through its tool call is not listed again.
    // Copilot's session log reports skills; Codex has no such event.
    #[allow(dead_code)]
    pub(crate) fn tome(&mut self, name: &str) {
        let name = clean(name);
        if self
            .spells
            .iter()
            .any(|spell| spell.kind == SpellKind::Tome && spell.detail == name)
        {
            return;
        }
        self.push(Spell {
            id: String::new(),
            kind: SpellKind::Tome,
            detail: name,
            state: SpellState::Done,
        });
    }

    pub(crate) fn oracle(&mut self) {
        self.oracles += 1;
    }

    // Copilot's session log reports intents; Codex has no such event.
    #[allow(dead_code)]
    pub(crate) fn intend(&mut self, intent: &str) {
        let intent = clean(intent);
        if !intent.is_empty() {
            self.intent = Some(intent);
        }
    }

    /// The newest call still running.
    pub(crate) fn casting(&self) -> Option<&Spell> {
        self.spells
            .iter()
            .rev()
            .find(|spell| spell.state == SpellState::Casting)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tools_are_named_by_the_words_of_their_names() {
        let cases = [
            ("glob", SpellKind::Track),
            ("grep", SpellKind::Track),
            ("view", SpellKind::Insight),
            ("edit", SpellKind::Inscribe),
            ("apply_patch", SpellKind::Inscribe),
            ("powershell", SpellKind::Ritual),
            ("read_powershell", SpellKind::Ritual),
            ("web_fetch", SpellKind::Farsight),
            ("web_search", SpellKind::Farsight),
            ("task", SpellKind::Summon),
            ("read_agent", SpellKind::Summon),
            ("skill", SpellKind::Tome),
            ("sql", SpellKind::Memory),
            ("store_memory", SpellKind::Memory),
            ("ask_user", SpellKind::Query),
            ("truncate", SpellKind::Arcane),
        ];
        for (tool, kind) in cases {
            assert_eq!(SpellKind::of(tool, false), kind, "{tool}");
        }
        assert_eq!(SpellKind::of("search_code", true), SpellKind::Pact);
    }

    #[test]
    fn details_show_one_telling_argument() {
        let of = |tool: &str, value: Value| detail(tool, false, Some(&value));
        assert_eq!(of("glob", json!({"pattern": "*.md"})), "*.md");
        assert_eq!(
            of("view", json!({"path": "C:\\repo\\src\\app.rs"})),
            "app.rs"
        );
        assert_eq!(
            of("powershell", json!({"command": "\n  git   status\nmore"})),
            "git status"
        );
        assert_eq!(of("glob", json!("{\"pattern\":\"src/**\"}")), "src/**");
        assert_eq!(of("ask_user", json!({"message": "secret"})), "");
        assert_eq!(of("grep", json!({"pattern": "a\u{1b}[31m\u{202e}b"})), "ab");
        assert_eq!(
            intent(Some(&json!({"intent": "Exploring codebase"}))).as_deref(),
            Some("Exploring codebase")
        );
    }

    #[test]
    fn a_turn_records_its_spells() {
        let mut chronicle = Chronicle::default();
        chronicle.cast("1", "glob", "*.md", false, false);
        chronicle.cast("1", "glob", "*.md", false, false);
        chronicle.cast("2", "task", "", false, false);
        chronicle.summon("2", "explore");
        chronicle.cast("3", "view", "a.rs", false, true);
        chronicle.resolve("1", true);
        chronicle.tome("azure-image-gen");
        chronicle.tome("azure-image-gen");
        assert_eq!(chronicle.cast, 3);
        assert_eq!((chronicle.helpers, chronicle.summons), (1, 1));
        assert_eq!(chronicle.spells[0].state, SpellState::Done);
        assert_eq!(chronicle.spells[1].kind, SpellKind::Summon);
        assert_eq!(chronicle.spells[1].detail, "explore");
        assert_eq!(
            chronicle.casting().map(|spell| spell.id.as_str()),
            Some("2")
        );
        for id in 0..40 {
            chronicle.cast(&format!("x{id}"), "grep", "", false, false);
        }
        assert_eq!(chronicle.spells.len(), KEEP);
        assert_eq!(chronicle.cast, 43);
    }
}
