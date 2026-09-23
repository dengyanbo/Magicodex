//! Local slash-command control for the presentation-only magic circle.

use super::*;
use crate::magic_style::MagicStyle;

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
            _ => match MagicStyle::parse(args) {
                Some(style) => self.apply_magic_style(style),
                None => self.push_magic_notice(Box::new(history_cell::new_error_event(
                    crate::magic_circle::USAGE.to_string(),
                ))),
            },
        }
    }

    /// Keeps `style` for the rest of the app run and shows the circle.
    pub(crate) fn apply_magic_style(&mut self, style: MagicStyle) {
        self.magic.set_style(style);
        self.set_magic_display(/*enabled*/ true);
    }

    fn set_magic_display(&mut self, enabled: bool) {
        self.magic.set_enabled(enabled);
        self.sync_active_stream_tail();
        self.app_event_tx.send(AppEvent::MagicDisplayChanged);
        let state = if enabled { "on" } else { "off" };
        self.push_magic_notice(Box::new(history_cell::new_info_event(
            format!("Magic circle {state}"),
            Some(format!("· {}", self.magic.style().label())),
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
}
