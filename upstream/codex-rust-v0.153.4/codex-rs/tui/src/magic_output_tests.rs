use super::*;
use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Modifier;
use std::time::Duration;

const GLOW: [Style; 2] = [
    Style::new().fg(Color::Magenta),
    Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
];

#[test]
fn outlet_is_above_the_response_and_is_not_exported_as_text() {
    let now = Instant::now();
    let settings = MagicSettings::default();
    settings.set_enabled(/*enabled*/ true);
    let mut circle = MagicCircle::default();
    circle.submit("Prompt", now);
    circle.reply_delta("First response", now + Duration::from_secs(/*secs*/ 8));
    let outlet = MagicOutletCell {
        circle,
        style: MagicStyle::Classic,
        captured_at: now + Duration::from_secs(/*secs*/ 8),
        settings: settings.clone(),
        animations_enabled: true,
    };
    let lines = outlet.display_lines(/*width*/ 80);
    let rendered = lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("magic_downward_outlet", rendered);
    let cone = lines.last().map(Line::to_string).unwrap_or_default();
    let mut strokes = cone.chars().filter(|c| !c.is_whitespace()).peekable();
    assert!(
        strokes.peek().is_some() && strokes.all(|c| ('\u{2800}'..='\u{28ff}').contains(&c)),
        "the outlet ends with the light cone: {cone:?}"
    );
    assert_eq!(outlet.raw_lines(), Vec::<Line<'static>>::new());
    assert_eq!(
        outlet.transcript_lines(/*width*/ 80),
        Vec::<Line<'static>>::new()
    );
    settings.set_style(MagicStyle::Fire);
    assert_eq!(
        outlet.display_lines(/*width*/ 80),
        lines,
        "history keeps its style"
    );
    settings.set_enabled(/*enabled*/ false);
    assert_eq!(
        outlet.display_lines(/*width*/ 80),
        Vec::<Line<'static>>::new()
    );
}

#[test]
fn magic_preview_advances_without_newlines_and_does_not_duplicate_commits() {
    use crate::history_cell::HistoryRenderMode;
    use crate::streaming::controller::StreamController;
    use crate::terminal_hyperlinks::visible_lines;

    let text = |lines: Vec<Line<'static>>| lines.iter().map(Line::to_string).collect::<Vec<_>>();
    let mut controller =
        StreamController::new(Some(80), &std::env::temp_dir(), HistoryRenderMode::Rich);
    controller.push("First paragraph");
    assert!(controller.current_tail_lines().is_empty());
    assert_eq!(
        text(visible_lines(controller.magic_tail_lines(GLOW))),
        vec!["First paragraph".to_string()]
    );
    controller.push("\n\n第二段");
    while controller.queued_lines() > 0 {
        controller.on_commit_tick();
    }
    assert_eq!(
        text(visible_lines(controller.magic_tail_lines(GLOW))),
        vec!["第二段".to_string()]
    );
    let (_, source) = controller.finalize();
    assert_eq!(source, Some("First paragraph\n\n第二段\n".to_string()));
}

#[test]
fn newest_provisional_text_glows_and_cools() {
    let styles = |line: Line<'static>| {
        line.spans
            .into_iter()
            .map(|span| {
                (
                    span.content.into_owned(),
                    span.style.fg,
                    span.style.add_modifier,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        styles(glowing_tail("法阵已经完成".to_string(), GLOW)),
        vec![
            ("法".to_string(), None, Modifier::empty()),
            (
                "阵已经".to_string(),
                Some(Color::Magenta),
                Modifier::empty()
            ),
            ("完成".to_string(), Some(Color::Magenta), Modifier::BOLD),
        ]
    );
    assert_eq!(
        styles(glowing_tail("✦".to_string(), GLOW)),
        vec![
            (String::new(), None, Modifier::empty()),
            (String::new(), Some(Color::Magenta), Modifier::empty()),
            ("✦".to_string(), Some(Color::Magenta), Modifier::BOLD),
        ]
    );
}

#[test]
fn magic_preview_retains_native_table_holdback() {
    use crate::history_cell::HistoryRenderMode;
    use crate::streaming::controller::StreamController;

    let mut controller =
        StreamController::new(Some(80), &std::env::temp_dir(), HistoryRenderMode::Rich);
    controller.push("| Header |\n| --- |\n| unfinished");
    assert_eq!(
        controller.magic_tail_lines(GLOW),
        controller.current_tail_lines()
    );
    assert_eq!(
        controller.magic_tail_starts_stream(),
        controller.tail_starts_stream()
    );
}
