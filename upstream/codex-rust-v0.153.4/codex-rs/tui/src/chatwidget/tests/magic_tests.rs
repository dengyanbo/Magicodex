use super::*;
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
    for command in ["on", "list", "off"] {
        chat.dispatch_command_with_args(SlashCommand::Magic, command.to_string(), Vec::new());
        assert_eq!(composer_buffer(&chat), before);
    }
    chat.on_task_started();
    chat.dispatch_command_with_args(SlashCommand::Magic, "on".to_string(), Vec::new());
    assert!(
        chat.magic_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    chat.dispatch_command_with_args(SlashCommand::Magic, "off".to_string(), Vec::new());
    assert!(
        !chat
            .magic_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    );
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
