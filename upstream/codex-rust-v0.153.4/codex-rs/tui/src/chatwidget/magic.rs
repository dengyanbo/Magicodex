//! Local slash-command control for the presentation-only magic circle.

use super::*;

impl ChatWidget {
    pub(super) fn handle_magic_command(&mut self, args: &str) {
        match args.trim() {
            "on" | "off" => {
                let enabled = args.trim() == "on";
                self.magic_enabled
                    .store(enabled, std::sync::atomic::Ordering::Relaxed);
                let state = if enabled { "on" } else { "off" };
                self.add_plain_history_lines(vec![
                    format!("Magic circle: {state} (classic).").into(),
                ]);
            }
            "list" => {
                self.add_plain_history_lines(
                    crate::magic_circle::STYLES
                        .iter()
                        .map(|(id, description)| format!("{id} — {description}").into())
                        .collect(),
                );
            }
            _ => self.add_error_message(crate::magic_circle::USAGE.to_string()),
        }
        self.request_redraw();
    }
}
