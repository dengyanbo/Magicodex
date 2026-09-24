use super::*;
use crate::magic_style::MagicChoice;
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

fn selected_choice(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Option<MagicChoice> {
    std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event {
        AppEvent::MagicStyleSelected(choice) => Some(choice),
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
    assert_eq!(selected_choice(&mut events), None);
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
    let choice = selected_choice(&mut events).expect("Enter selects the highlighted style");
    assert_eq!(choice, MagicChoice::Style(MagicStyle::Tech));
    chat.apply_magic_choice(choice);
    assert!(chat.magic.enabled());
    assert_eq!(chat.magic.style(), MagicStyle::Tech);
}

#[tokio::test]
async fn magic_list_offers_the_random_choice_last() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    let highlight_random = |chat: &mut ChatWidget| {
        chat.handle_magic_command("list");
        for _ in MagicStyle::ALL {
            chat.handle_key_event(KeyEvent::from(KeyCode::Down));
        }
    };
    highlight_random(&mut chat);
    assert!(
        chat.magic.is_random(),
        "the highlight previews the random choice"
    );
    assert_ne!(
        chat.magic.style(),
        MagicStyle::Classic,
        "with another style"
    );
    chat.handle_key_event(KeyEvent::from(KeyCode::Esc));
    assert_eq!(chat.magic.choice(), MagicChoice::Style(MagicStyle::Classic));
    assert_eq!(selected_choice(&mut events), None);

    highlight_random(&mut chat);
    let preview = chat.magic.style();
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    let choice = selected_choice(&mut events).expect("Enter selects the random choice");
    assert_eq!(choice, MagicChoice::Random);
    chat.apply_magic_choice(choice);
    assert_eq!(
        (chat.magic.choice(), chat.magic.style()),
        (MagicChoice::Random, preview),
        "the previewed draw is kept"
    );
    assert!(chat.magic.enabled());
    chat.handle_magic_command("list");
    let popup = render_bottom_popup(&chat, /*width*/ 120);
    let current = popup
        .lines()
        .find(|line| line.contains("(current)"))
        .unwrap_or_default();
    assert!(current.contains("11. random"), "{popup}");
}

#[tokio::test]
async fn magic_random_draws_another_style_for_every_turn() {
    let (mut chat, mut events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    chat.handle_magic_command("随机");
    let notices = drain_insert_history(&mut events)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<Vec<_>>();
    assert!(
        notices.len() == 1
            && notices[0].contains("Magic circle on")
            && notices[0].contains("random 随机"),
        "{notices:?}"
    );
    assert_eq!(chat.magic.choice(), MagicChoice::Random);
    assert_ne!(
        chat.magic.style(),
        MagicStyle::Classic,
        "turning random on draws another style"
    );
    let mut seen = vec![chat.magic.style()];
    for turn in 0..30 {
        let id = format!("turn-{turn}");
        let before = chat.magic.style();
        handle_turn_started(&mut chat, &id);
        assert_eq!(
            chat.magic.style(),
            before,
            "the turn casts the style drawn for it"
        );
        handle_turn_completed(&mut chat, &id, /*duration_ms*/ None);
        assert_ne!(chat.magic.style(), before, "{id} drew the same style again");
        seen.push(chat.magic.style());
    }
    seen.sort_by_key(|style| *style as u8);
    seen.dedup();
    assert!(seen.len() >= 6, "30 turns drew only {seen:?}");
    chat.handle_magic_command("fire");
    handle_turn_started(&mut chat, "fixed");
    handle_turn_completed(&mut chat, "fixed", /*duration_ms*/ None);
    assert_eq!(
        chat.magic.choice(),
        MagicChoice::Style(MagicStyle::Fire),
        "a chosen style stays"
    );
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

#[tokio::test]
async fn tool_calls_are_written_beside_the_charging_circle() {
    use crate::magic_sides::spells::SpellKind;
    use crate::magic_sides::spells::SpellState;

    let (mut chat, _events, _ops) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.dispatch_command_with_args(SlashCommand::Magic, "on".to_string(), Vec::new());
    chat.on_task_started();
    let search = begin_exec(&mut chat, "call-search", "rg magic_circle src");
    let test = begin_exec(&mut chat, "call-test", "cargo test --quiet");
    end_exec(&mut chat, search, "src/lib.rs", "", /*exit_code*/ 0);
    end_exec(&mut chat, test, "", "boom", /*exit_code*/ 1);
    let spells = &chat.magic_circle.chronicle.spells;
    assert_eq!(spells.len(), 2);
    assert_eq!(
        (spells[0].kind, spells[0].state),
        (SpellKind::Track, SpellState::Done)
    );
    assert_eq!(spells[0].detail, "magic_circle");
    assert_eq!(
        (spells[1].kind, spells[1].state),
        (SpellKind::Ritual, SpellState::Failed)
    );
    assert_eq!(spells[1].detail, "cargo test --quiet");
}
