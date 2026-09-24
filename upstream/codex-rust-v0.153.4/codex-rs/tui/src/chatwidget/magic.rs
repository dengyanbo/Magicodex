//! Local slash-command control for the presentation-only magic circle.

use super::*;
use crate::magic_sides::spells;
use crate::magic_style::MagicChoice;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandAction;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::McpToolCallStatus;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::ThreadItem;

impl ChatWidget {
    pub(super) fn handle_magic_command(&mut self, args: &str) {
        let args = args.trim();
        match args {
            "" | "list" => {
                self.bottom_pane
                    .show_selection_view(crate::magic_picker::picker_params(&self.magic));
                self.request_redraw();
            }
            "on" | "off" => self.set_magic_display(args == "on"),
            _ => match MagicChoice::parse(args) {
                Some(choice) => self.apply_magic_choice(choice),
                None => self.push_magic_notice(Box::new(history_cell::new_error_event(
                    crate::magic_circle::USAGE.to_string(),
                ))),
            },
        }
    }

    /// Keeps `choice` for the rest of the app run and shows the circle.
    pub(crate) fn apply_magic_choice(&mut self, choice: MagicChoice) {
        self.magic.set_choice(choice);
        self.set_magic_display(/*enabled*/ true);
    }

    fn set_magic_display(&mut self, enabled: bool) {
        self.magic.set_enabled(enabled);
        self.sync_active_stream_tail();
        self.app_event_tx.send(AppEvent::MagicDisplayChanged);
        let state = if enabled { "on" } else { "off" };
        self.push_magic_notice(Box::new(history_cell::new_info_event(
            format!("Magic circle {state}"),
            Some(format!("· {}", self.magic.choice().label())),
        )));
    }

    fn push_magic_notice(&mut self, notice: Box<dyn HistoryCell>) {
        if self.stream_controller.is_some() {
            // A notice inside the native streaming-cell run would break final consolidation.
            self.magic_output.pending_notices.push(notice);
        } else {
            self.add_boxed_history(notice);
        }
        self.request_redraw();
    }

    pub(super) fn prepare_magic_output(&mut self) {
        if self.magic.enabled()
            && self.magic_circle.is_active()
            && self.magic_output.is_answer()
            && !self.magic_output.emitted
            && !self.raw_output_mode
        {
            self.magic_output.emitted = true;
            self.add_to_history(crate::magic_output::MagicOutletCell {
                circle: self.magic_circle.clone(),
                style: self.magic.style(),
                captured_at: Instant::now(),
                settings: self.magic.clone(),
                animations_enabled: self.config.animations,
            });
            self.transcript.needs_final_message_separator = false;
        }
    }

    /// Writes a started tool call into the grimoire beside the live circle.
    pub(super) fn record_magic_item_started(&mut self, item: &ThreadItem) {
        if !self.magic_circle.is_active() {
            return;
        }
        let chronicle = &mut self.magic_circle.chronicle;
        match item {
            ThreadItem::CommandExecution {
                id,
                command,
                command_actions,
                ..
            } => {
                let (tool, detail) = match command_actions.first() {
                    Some(CommandAction::Read { name, .. }) => ("read", name.clone()),
                    Some(CommandAction::Search { query, command, .. }) => (
                        "search",
                        query.clone().unwrap_or_else(|| spells::first_line(command)),
                    ),
                    Some(CommandAction::ListFiles { path, command }) => (
                        "list",
                        path.clone().unwrap_or_else(|| spells::first_line(command)),
                    ),
                    Some(CommandAction::Unknown { command }) => {
                        ("shell", spells::first_line(command))
                    }
                    None => ("shell", spells::first_line(command)),
                };
                chronicle.cast(id, tool, &detail, /*mcp*/ false, /*nested*/ false);
            }
            ThreadItem::FileChange { id, changes, .. } => {
                let detail = changes
                    .first()
                    .map(|change| spells::file_name(&change.path))
                    .unwrap_or_default();
                chronicle.cast(
                    id,
                    "apply_patch",
                    &detail,
                    /*mcp*/ false,
                    /*nested*/ false,
                );
            }
            ThreadItem::McpToolCall { id, tool, .. } => {
                chronicle.cast(id, tool, tool, /*mcp*/ true, /*nested*/ false);
            }
            ThreadItem::DynamicToolCall {
                id,
                tool,
                arguments,
                ..
            } => {
                let detail = spells::detail(tool, /*mcp*/ false, Some(arguments));
                chronicle.cast(id, tool, &detail, /*mcp*/ false, /*nested*/ false);
            }
            ThreadItem::WebSearch(search) => {
                chronicle.cast(
                    &search.id,
                    "web_search",
                    &search.query,
                    /*mcp*/ false,
                    /*nested*/ false,
                );
            }
            ThreadItem::ImageView { id, .. } => {
                chronicle.cast(
                    id,
                    "view_image",
                    "",
                    /*mcp*/ false,
                    /*nested*/ false,
                );
            }
            ThreadItem::CollabAgentToolCall { id, tool, .. } => match tool {
                CollabAgentTool::SpawnAgent
                | CollabAgentTool::ResumeAgent
                | CollabAgentTool::FollowupTask => chronicle.summon(id, ""),
                _ => chronicle.cast(id, "agent", "", /*mcp*/ false, /*nested*/ false),
            },
            _ => {}
        }
    }

    /// Marks a finished tool call; each finished assistant message counts as an oracle.
    pub(super) fn record_magic_item_completed(&mut self, item: &ThreadItem) {
        if !self.magic_circle.is_active() {
            return;
        }
        let chronicle = &mut self.magic_circle.chronicle;
        let (id, ok) = match item {
            ThreadItem::AgentMessage { .. } => {
                chronicle.oracle();
                return;
            }
            ThreadItem::CommandExecution { id, status, .. } => {
                (id, matches!(status, CommandExecutionStatus::Completed))
            }
            ThreadItem::FileChange { id, status, .. } => {
                (id, matches!(status, PatchApplyStatus::Completed))
            }
            ThreadItem::McpToolCall { id, status, .. } => {
                (id, matches!(status, McpToolCallStatus::Completed))
            }
            ThreadItem::DynamicToolCall { id, status, .. } => {
                (id, matches!(status, DynamicToolCallStatus::Completed))
            }
            ThreadItem::CollabAgentToolCall { id, status, .. } => {
                (id, matches!(status, CollabAgentToolCallStatus::Completed))
            }
            ThreadItem::WebSearch(search) => (&search.id, true),
            ThreadItem::ImageView { id, .. } => (id, true),
            _ => return,
        };
        chronicle.resolve(id, ok);
    }
}
