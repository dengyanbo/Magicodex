//! The visual outlet committed immediately before an assistant answer.
//!
//! The circle is fixed in scrollback before the first answer fragment, so native streaming,
//! consolidation and resize reflow cannot move the answer above its source. The style chosen at
//! that moment stays with the outlet, and its emission below the settled circle leads into the
//! answer. Decorative lines are excluded from copy-friendly/raw output and the transcript
//! overlay.

use std::time::Instant;

use codex_protocol::models::MessagePhase;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::history_cell::HistoryCell;
use crate::magic_circle::CIRCLE_ROWS;
use crate::magic_circle::MagicCircle;
use crate::magic_circle::MagicScene;
use crate::magic_circle::MagicView;
use crate::magic_circle::OUTLET_ROWS;
use crate::magic_style::MagicSettings;
use crate::magic_style::MagicStyle;

#[derive(Default)]
pub(crate) struct MagicOutput {
    pub(crate) phase: Option<MessagePhase>,
    pub(crate) emitted: bool,
    pub(crate) message_started: bool,
    pub(crate) pending_notices: Vec<Box<dyn HistoryCell>>,
}

impl MagicOutput {
    pub(crate) fn is_answer(&self) -> bool {
        !matches!(self.phase, Some(MessagePhase::Commentary))
    }
}

#[derive(Debug)]
pub(crate) struct MagicOutletCell {
    pub(crate) circle: MagicCircle,
    pub(crate) style: MagicStyle,
    pub(crate) captured_at: Instant,
    pub(crate) settings: MagicSettings,
    pub(crate) animations_enabled: bool,
}

/// Newest provisional answer text glows in the style's `[warm, hot]` colours; it cools as more
/// text arrives and returns to native styling once the line is committed.
pub(crate) fn glowing_tail(text: String, [warm_style, hot_style]: [Style; 2]) -> Line<'static> {
    let mut starts = text
        .grapheme_indices(/*is_extended*/ true)
        .rev()
        .map(|(index, _)| index);
    let hot = starts.nth(1).unwrap_or(0);
    let warm = starts.nth(2).unwrap_or(0);
    Line::from(vec![
        text[..warm].to_string().into(),
        Span::styled(text[warm..hot].to_string(), warm_style),
        Span::styled(text[hot..].to_string(), hot_style),
    ])
}

impl HistoryCell for MagicOutletCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if !self.settings.enabled() || width < 16 {
            return Vec::new();
        }
        let render_width = width.min(120);
        let area = Rect::new(
            /*x*/ 0,
            /*y*/ 0,
            render_width,
            CIRCLE_ROWS + OUTLET_ROWS,
        );
        let mut buffer = Buffer::empty(area);
        MagicView {
            circle: &self.circle,
            style: self.style,
            animations_enabled: self.animations_enabled,
            scene: MagicScene::Outlet,
        }
        .render_at(area, &mut buffer, self.captured_at);
        let margin = " ".repeat(usize::from(width - render_width) / 2);
        let mut lines = Vec::new();
        for row in buffer.content().chunks(usize::from(render_width)) {
            let mut spans = vec![Span::raw(margin.clone())];
            let mut column = 0;
            while column < row.len() {
                let cell = &row[column];
                spans.push(Span::styled(cell.symbol().to_string(), cell.style()));
                column += cell.symbol().width().max(1);
            }
            lines.push(Line::from(spans));
        }
        let blank = |line: &Line<'_>| line.spans.iter().all(|span| span.content.trim().is_empty());
        while lines.first().is_some_and(blank) {
            lines.remove(0);
        }
        while lines.last().is_some_and(blank) {
            lines.pop();
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        Vec::new()
    }

    fn transcript_lines(&self, _width: u16) -> Vec<Line<'static>> {
        Vec::new()
    }

    fn has_stable_transcript_height(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "magic_output_tests.rs"]
mod tests;
