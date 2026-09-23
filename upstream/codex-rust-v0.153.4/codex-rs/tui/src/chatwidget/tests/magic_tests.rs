use super::*;
use crate::magic_style::MagicStyle;
use pretty_assertions::assert_eq;

fn composer_buffer(chat: &ChatWidget) -> Buffer {
    let composer = chat
        .bottom_pane
        .as_renderable_with_composer_right_reserve(/*composer_right_reserve*/ 0);
    let area = Rect::new(
        /*x*/ 0,
        /*y*/ 0,
        /*width*/ 100,
        composer.desired_height(/*width*/ 100),
    );
    let mut buffer = Buffer::empty(area);
    composer.render(area, &mut buffer);
    buffer
}

#[tokio::test]
async fn magic_commands_are_local_and_preserve_the_native_composer() {
    let (mut chat, _events, mut ops) = make_chatwidget_manual(/*model_override*/ None).await;
    while ops.try_recv().is_ok() {}
    let before = composer_buffer(&chat);
    for command in ["on", "fire", "水", "off"] {
        chat.dispatch_command_with_args(SlashCommand::Magic, command.to_string(), Vec::new());
        assert_eq!(composer_buffer(&chat), before);
    }
    assert_eq!(chat.magic.style(), MagicStyle::Water);
    chat.on_task_started();
    chat.dispatch_command_with_args(SlashCommand::Magic, "on".to_string(), Vec::new());
    assert!(chat.magic.enabled());
    chat.dispatch_command_with_args(SlashCommand::Magic, "off".to_string(), Vec::new());
    assert!(!chat.magic.enabled());
    while let Ok(op) = ops.try_recv() {
        assert!(
            !matches!(op, Op::UserTurn { .. }),
            "local magic command reached the model"
        );
    }
}

#[tokio::test]
async fn magic_does_not_change_user_prompt_contents() {
    let (mut chat, _events, mut ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.dispatch_command_with_args(SlashCommand::Magic, "on".to_string(), Vec::new());
    chat.thread_id = Some(ThreadId::new());
    let text = "原始 prompt\nKeep every character.";
    chat.submit_user_message(text.into());
    let Op::UserTurn { items, .. } = next_submit_op(&mut ops) else {
        panic!("expected a native user turn");
    };
    assert_eq!(
        items,
        vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }]
    );
}

#[tokio::test]
async fn reasoning_does_not_feed_or_freeze_the_magic_circle() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.dispatch_command_with_args(SlashCommand::Magic, "on".to_string(), Vec::new());
    chat.on_task_started();
    let before = chat.magic_circle.clone();
    chat.on_agent_reasoning_delta("PRIVATE_REASONING_DO_NOT_RENDER".to_string());
    assert_eq!(chat.magic_circle, before);
    chat.on_agent_message_delta("An actual assistant reply".to_string());
    assert_ne!(chat.magic_circle, before);
}

#[tokio::test]
async fn magic_answer_starts_below_one_outlet_and_previews_partial_text() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.last_rendered_width.set(Some(80));
    chat.handle_magic_command("on");
    chat.on_task_started();
    while events.try_recv().is_ok() {}
    chat.on_agent_message_delta(" ".to_string());
    chat.on_agent_message_delta("青蓝".to_string());
    chat.on_agent_message_delta("星环".to_string());
    let mut outlets = 0;
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            assert!(cell.as_any().is::<crate::magic_output::MagicOutletCell>());
            outlets += 1;
        }
    }
    assert_eq!(outlets, 1);
    let tail = chat
        .transcript
        .active_cell
        .as_ref()
        .expect("live answer preview");
    let text = tail
        .display_lines(/*width*/ 80)
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("青蓝星环"));
    assert!(chat.magic_output.emitted);

    complete_assistant_message(
        &mut chat,
        "answer",
        " 青蓝星环",
        Some(MessagePhase::FinalAnswer),
    );
    let mut canonical = None;
    while let Ok(event) = events.try_recv() {
        if let AppEvent::ConsolidateAgentMessage { source, .. } = event {
            canonical = Some(source);
        }
    }
    assert_eq!(canonical, Some(" 青蓝星环".to_string()));
    handle_turn_completed(&mut chat, "turn", /*duration_ms*/ None);
    assert!(
        chat.magic_output.emitted,
        "do not place a fresh circle below the completed answer"
    );
}

#[tokio::test]
async fn commentary_keeps_orbiting_until_the_final_answer_begins() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.handle_magic_command("on");
    chat.on_task_started();
    chat.magic_output.phase = Some(MessagePhase::Commentary);
    chat.on_agent_message_delta("Working on it".to_string());
    assert!(!chat.magic_output.emitted);
    complete_assistant_message(
        &mut chat,
        "comment",
        "Working on it",
        Some(MessagePhase::Commentary),
    );
    while events.try_recv().is_ok() {}
    chat.magic_output.phase = Some(MessagePhase::FinalAnswer);
    chat.on_agent_message_delta("Result".to_string());
    assert!(chat.magic_output.emitted);
    assert!(
        matches!(events.try_recv(), Ok(AppEvent::InsertHistoryCell(cell))
        if cell.as_any().is::<crate::magic_output::MagicOutletCell>())
    );
}

#[tokio::test]
async fn disabled_magic_keeps_native_newline_buffering() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.on_task_started();
    while events.try_recv().is_ok() {}
    chat.on_agent_message_delta("No newline yet".to_string());
    assert!(!chat.magic_output.emitted);
    assert!(!chat.active_cell_is_stream_tail());
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            assert!(!cell.as_any().is::<crate::magic_output::MagicOutletCell>());
        }
    }
}

#[tokio::test]
async fn magic_completed_only_answer_emits_outlet_before_canonical_markdown() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.handle_magic_command("on");
    chat.on_task_started();
    while events.try_recv().is_ok() {}
    let answer = "**完整回复**\n\n没有 delta，也不能丢字。";
    complete_assistant_message(&mut chat, "answer", answer, Some(MessagePhase::FinalAnswer));
    let mut sequence = Vec::new();
    while let Ok(event) = events.try_recv() {
        match event {
            AppEvent::InsertHistoryCell(cell)
                if cell.as_any().is::<crate::magic_output::MagicOutletCell>() =>
            {
                sequence.push("outlet");
            }
            AppEvent::ConsolidateAgentMessage { source, .. } => {
                assert_eq!(source, answer);
                sequence.push("answer");
            }
            _ => {}
        }
    }
    assert_eq!(sequence, vec!["outlet", "answer"]);
}

#[tokio::test]
async fn magic_toggle_during_answer_does_not_split_native_stream_cells() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.handle_magic_command("on");
    chat.on_task_started();
    chat.on_agent_message_delta("First paragraph\n\n".to_string());
    for _ in 0..10 {
        chat.on_commit_tick();
    }
    chat.on_agent_message_delta("第二段".to_string());
    while events.try_recv().is_ok() {}
    chat.handle_magic_command("off");
    assert!(!chat.active_cell_is_stream_tail());
    chat.handle_magic_command("on");
    assert!(chat.active_cell_is_stream_tail());
    chat.handle_magic_command("fire");
    assert_eq!(chat.magic_output.pending_notices.len(), 3);
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(event, AppEvent::InsertHistoryCell(_)));
    }
    let answer = "First paragraph\n\n第二段完整";
    complete_assistant_message(&mut chat, "answer", answer, Some(MessagePhase::FinalAnswer));
    let mut consolidated = false;
    let mut notices = 0;
    while let Ok(event) = events.try_recv() {
        match event {
            AppEvent::ConsolidateAgentMessage { source, .. } => {
                assert_eq!(source, answer);
                consolidated = true;
            }
            AppEvent::InsertHistoryCell(cell) => {
                assert!(
                    consolidated,
                    "notices must follow native markdown consolidation"
                );
                assert!(cell.as_any().is::<PlainHistoryCell>());
                notices += 1;
            }
            _ => {}
        }
    }
    assert!(consolidated);
    assert_eq!(notices, 3);
}

#[tokio::test]
async fn magic_enabled_mid_message_waits_for_the_next_answer() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.on_task_started();
    chat.on_agent_message_delta("Already streaming".to_string());
    chat.handle_magic_command("on");
    chat.on_agent_message_delta(" without a misplaced outlet".to_string());
    assert!(!chat.magic_output.emitted);
    complete_assistant_message(
        &mut chat,
        "answer",
        "Already streaming without a misplaced outlet",
        Some(MessagePhase::FinalAnswer),
    );
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            assert!(!cell.as_any().is::<crate::magic_output::MagicOutletCell>());
        }
    }
    chat.on_task_started();
    chat.on_agent_message_delta("Next answer".to_string());
    assert!(chat.magic_output.emitted);
}

#[tokio::test]
async fn magic_interrupted_long_answer_preserves_source_after_resize() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.last_rendered_width.set(Some(120));
    chat.handle_magic_command("on");
    chat.on_task_started();
    let mut answer = String::new();
    for index in 0..40 {
        let delta = format!("Line {index}: 宽字符和正文需要在窗口缩放后完整保留。\n\n");
        answer.push_str(&delta);
        chat.on_agent_message_delta(delta);
        chat.on_commit_tick();
    }
    chat.on_terminal_resize(/*width*/ 60);
    answer.push_str("尚未结束");
    chat.on_agent_message_delta("尚未结束".to_string());
    while events.try_recv().is_ok() {}
    handle_turn_interrupted(&mut chat, "turn-1");
    let sources = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            AppEvent::ConsolidateAgentMessage { source, .. } => Some(source),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(sources, vec![answer]);
    assert!(!chat.active_cell_is_stream_tail());
    assert!(chat.magic_output.emitted);
}

fn selected_style(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Option<MagicStyle> {
    std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event {
        AppEvent::MagicStyleSelected(style) => Some(style),
        _ => None,
    })
}

#[tokio::test]
async fn magic_list_previews_the_highlighted_style_and_esc_restores_it() {
    let (mut chat, mut events, mut ops) = make_chatwidget_manual(/*model_override*/ None).await;
    while ops.try_recv().is_ok() {}
    chat.dispatch_command_with_args(SlashCommand::Magic, "list".to_string(), Vec::new());
    assert_eq!(
        chat.bottom_pane.active_view_id(),
        Some(crate::magic_picker::VIEW_ID)
    );
    let popup = render_bottom_popup(&chat, /*width*/ 120);
    assert_chatwidget_snapshot!("magic_style_picker", popup);
    while events.try_recv().is_ok() {}
    chat.handle_key_event(KeyEvent::from(KeyCode::Down));
    assert_eq!(chat.magic.style(), MagicStyle::Wind);
    assert!(
        std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event, AppEvent::MagicStylePreviewed)),
        "moving the highlight redraws the preview"
    );
    chat.handle_key_event(KeyEvent::from(KeyCode::Down));
    assert_eq!(chat.magic.style(), MagicStyle::Fire);
    chat.handle_key_event(KeyEvent::from(KeyCode::Esc));
    assert!(!chat.bottom_pane.has_active_view());
    assert_eq!(chat.magic.style(), MagicStyle::Classic);
    assert!(
        !chat.magic.enabled(),
        "cancelling does not turn the circle on"
    );
    assert_eq!(selected_style(&mut events), None);
    while let Ok(op) = ops.try_recv() {
        assert!(
            !matches!(op, Op::UserTurn { .. }),
            "the picker reached the model"
        );
    }
}

#[tokio::test]
async fn magic_list_enter_keeps_the_highlighted_style_and_turns_the_circle_on() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.handle_magic_command("eerie");
    chat.handle_magic_command("off");
    chat.handle_magic_command("list");
    let popup = render_bottom_popup(&chat, /*width*/ 120);
    let current = popup
        .lines()
        .find(|line| line.contains("(current)"))
        .unwrap_or_default();
    assert!(current.contains("eerie"), "{popup}");
    chat.handle_key_event(KeyEvent::from(KeyCode::Down));
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert!(!chat.bottom_pane.has_active_view());
    let style = selected_style(&mut events).expect("Enter selects the highlighted style");
    assert_eq!(style, MagicStyle::Tech);
    chat.apply_magic_style(style);
    assert!(chat.magic.enabled());
    assert_eq!(chat.magic.style(), MagicStyle::Tech);
}

#[tokio::test]
async fn unknown_magic_argument_shows_usage_and_keeps_the_settings() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.handle_magic_command("ice");
    assert!(!chat.magic.enabled());
    assert_eq!(chat.magic.style(), MagicStyle::Classic);
    let notices = drain_insert_history(&mut events)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<Vec<_>>();
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].contains(crate::magic_circle::USAGE),
        "{notices:?}"
    );
}

#[tokio::test]
async fn magic_outlet_keeps_the_style_it_was_cast_with() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.handle_magic_command("WATER");
    let notices = drain_insert_history(&mut events)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<Vec<_>>();
    assert!(
        notices.len() == 1
            && notices[0].contains("Magic circle on")
            && notices[0].contains("water 水"),
        "{notices:?}"
    );
    chat.on_task_started();
    chat.on_agent_message_delta("水到渠成".to_string());
    let outlet = std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event {
        AppEvent::InsertHistoryCell(cell) => cell
            .as_any()
            .downcast_ref::<crate::magic_output::MagicOutletCell>()
            .map(|outlet| outlet.style),
        _ => None,
    });
    assert_eq!(outlet, Some(MagicStyle::Water));
    chat.handle_magic_command("雷");
    assert_eq!(chat.magic.style(), MagicStyle::Thunder);
    assert!(chat.magic.enabled());
}
