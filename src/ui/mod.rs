pub mod input;

use crate::{
    app::{App, View},
    magic,
    session::Status,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn safe_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .filter(|c| !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        .collect()
}

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < 35 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("终端太小，请扩大到至少 35×10。\nCtrl+C 中断 · Ctrl+Q 退出"),
            area,
        );
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(5),
            Constraint::Length(2),
        ])
        .split(area);
    let status_color = match app.session.status {
        Status::Failed | Status::Disconnected => Color::LightRed,
        Status::Completed => Color::Yellow,
        _ => Color::Cyan,
    };
    let header = Text::from(vec![
        Line::from(vec![
            Span::styled(
                " MAGICODEX ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                " {} · {} · ",
                if app.options.demo {
                    "DEMO / 模拟事件"
                } else {
                    app.options.backend.name()
                },
                safe_text(&app.session.model)
            )),
            Span::styled(
                if app.session.approvals.is_empty() {
                    app.session.status.label()
                } else {
                    "等待你的审批 / 输入"
                },
                Style::default().fg(status_color),
            ),
        ]),
        Line::raw(safe_text(&format!(
            "{}  {}",
            app.options.cwd.display(),
            app.session.policy
        ))),
    ]);
    frame.render_widget(Paragraph::new(header), chunks[0]);
    let small = !magic::supported_size(chunks[1].width, chunks[1].height);
    match app.view {
        View::Magic if !small => magic::draw(
            frame,
            chunks[1],
            &app.session,
            app.animation_clock(),
            app.reveal_progress(),
            app.options.ascii,
            &app.input.text,
        ),
        View::Answer => {
            let answer = app.session.answer();
            let text = if answer.is_empty() {
                "暂无最终正文。F3 查看全部真实事件。".into()
            } else {
                safe_text(&answer)
            };
            draw_text(
                frame,
                chunks[1],
                text,
                if app.session.has_final_answer() {
                    " 最终正文 · PgUp/PgDn 滚动 "
                } else {
                    " 回合文本 / 后端未标注阶段 · PgUp/PgDn "
                },
                app.scroll,
            );
        }
        _ => draw_text(
            frame,
            chunks[1],
            safe_text(&app.session.transcript()),
            if small && app.view == View::Magic {
                " 窗口尺寸不适合点阵画布，使用文本视图 "
            } else {
                " 原文日志 · PgUp/PgDn 滚动 "
            },
            app.scroll,
        ),
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 咒语 · Enter 提交 / Alt+Enter 换行 ")
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(chunks[2]);
    frame.render_widget(block, chunks[2]);
    let (lines, x, y) = app
        .input
        .display(inner.width as usize, inner.height as usize, false);
    frame.render_widget(Paragraph::new(lines.join("\n")), inner);
    if app.session.approvals.is_empty() {
        frame.set_cursor_position((
            inner.x + x.min(inner.width.saturating_sub(1)),
            inner.y + y.min(inner.height.saturating_sub(1)),
        ));
    }
    let notice = if app.session.trimmed {
        format!("原文窗口已截断，Ctrl+S 导出核对。 {}", app.session.notice)
    } else {
        app.session.notice.clone()
    };
    let footer = Text::from(vec![
        Line::raw(
            "F2 阵  F3 原文  F4 正文  F6 动效  Ctrl+S 导出  Ctrl+C 中断  Ctrl+N 新会话  Ctrl+Q 退出",
        ),
        Line::styled(
            safe_text(&notice),
            Style::default().fg(if app.session.status == Status::Failed {
                Color::LightRed
            } else {
                Color::Yellow
            }),
        ),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[3]);
    if let Some(request) = app.session.approvals.front() {
        let popup = Rect::new(area.x + 2, area.y + 2, area.width - 4, area.height - 4);
        frame.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 需要你的决定 · 不会自动批准 ")
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let parts = Layout::vertical([Constraint::Min(2), Constraint::Length(4)]).split(inner);
        let detail = app.approval_detail();
        draw_text(frame, parts[0], safe_text(&detail), "", app.approval_scroll);
        if request.is_input() {
            let secret = app
                .current_question()
                .is_some_and(|q| q["isSecret"].as_bool() == Some(true));
            let prompt = Block::default()
                .borders(Borders::TOP)
                .title(" 输入答案或选项编号 · Enter 下一题/提交 · Esc 取消并中断 ");
            let inner = prompt.inner(parts[1]);
            frame.render_widget(prompt, parts[1]);
            let (lines, x, y) =
                app.approval_input
                    .display(inner.width as usize, inner.height as usize, secret);
            frame.render_widget(Paragraph::new(lines.join("\n")), inner);
            frame.set_cursor_position((
                inner.x + x.min(inner.width.saturating_sub(1)),
                inner.y + y.min(inner.height.saturating_sub(1)),
            ));
        } else {
            let verb = if app.approval_scrolled_to_end(parts[0].width, parts[0].height) {
                "Y 仅允许此次/本回合    N 拒绝    Esc 拒绝并中断"
            } else {
                "请先用 PgDn 阅读完整请求；N 可直接拒绝，Esc 中断"
            };
            frame.render_widget(
                Paragraph::new(format!("{verb}\n{}", safe_text(&app.session.notice))),
                parts[1],
            );
        }
    }
}

fn draw_text(frame: &mut Frame, area: Rect, text: String, title: &str, scroll: u16) {
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_output_cannot_emit_escape_sequences() {
        let text = safe_text("\u{1b}]52;c;evil\u{7}\u{1b}[2J\u{202e}正文\n");
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\u{7}'));
        assert!(!text.contains('\u{202e}'));
        assert!(text.ends_with("正文\n"));
    }
}
