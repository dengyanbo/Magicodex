use crate::{
    session::{Session, Status},
    ui::safe_text,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Paragraph, Wrap,
        canvas::{Canvas, Line as CanvasLine, Points},
    },
};
use std::f64::consts::{PI, TAU};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn point(radius: f64, angle: f64) -> (f64, f64) {
    (radius * angle.cos(), radius * angle.sin())
}

pub fn supported_size(width: u16, height: u16) -> bool {
    // Ratatui 0.29 的 Braille 网格以 u16 计算单元数。
    width >= 80
        && height >= 18
        && u32::from(width.saturating_sub(2)) * u32::from(height.saturating_sub(2))
            <= u32::from(u16::MAX)
}

fn short(text: &str, max: usize) -> String {
    let clean = safe_text(text).replace(['\n', '\t'], " ");
    let mut result = String::new();
    let mut width = 0;
    for glyph in clean.graphemes(true) {
        width += glyph.width();
        if width > max {
            break;
        }
        result.push_str(glyph);
    }
    result
}

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    session: &Session,
    clock: f64,
    reveal: Option<f64>,
    ascii: bool,
    draft: &str,
) {
    let active = session.status == Status::Running && session.approvals.is_empty();
    let angle = if active { clock * 0.22 } else { 0.0 };
    let color = match session.status {
        Status::Failed | Status::Disconnected => Color::Rgb(210, 104, 109),
        Status::Interrupted => Color::Rgb(132, 141, 163),
        Status::Completed => Color::Rgb(213, 184, 107),
        _ => Color::Rgb(86, 204, 209),
    };
    let dim = Color::Rgb(40, 94, 114);
    let gold = Color::Rgb(214, 178, 102);
    let x_bound = 100.0 * f64::from(area.width.max(1))
        / (2.0 * f64::from(area.height.saturating_sub(2).max(1)));
    let progress = reveal.unwrap_or(1.0).clamp(0.0, 1.0);
    let radius = x_bound.min(100.0) * 0.88 * (1.0 - 0.16 * (progress * PI).sin());
    let inscription = if !session.turn_active && !draft.is_empty() {
        ("草稿", short(draft, 22))
    } else {
        ("咏唱", short(&session.prompt, 22))
    };
    let mut fragments = vec![inscription];
    for item in session
        .items
        .iter()
        .rev()
        .filter(|i| !matches!(i.kind.as_str(), "userMessage" | "system"))
        .filter(|i| Some(i.turn_id.as_str()) == session.turn_id.as_deref())
        .take(3)
    {
        let label = match item.kind.as_str() {
            "commandExecution" => "执行",
            "fileChange" => "改写",
            "reasoning" => "推理",
            "agentMessage" => "回复",
            "plan" => "计划",
            "mcpToolCall" => "工具",
            _ => "事件",
        };
        fragments.push((label, short(&item.text, 20)));
    }
    let tools: Vec<_> = session
        .items
        .iter()
        .filter(|i| {
            matches!(
                i.kind.as_str(),
                "commandExecution" | "fileChange" | "mcpToolCall"
            ) && Some(i.turn_id.as_str()) == session.turn_id.as_deref()
        })
        .rev()
        .take(8)
        .collect();
    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" MAGICODEX · ARCANE ENGINE ")
                .border_style(Style::default().fg(dim)),
        )
        .marker(if ascii { Marker::Dot } else { Marker::Braille })
        .x_bounds([-x_bound, x_bound])
        .y_bounds([-100.0, 100.0])
        .paint(|ctx| {
            for (ring, factor) in [1.0, 0.94, 0.72, 0.65, 0.43].iter().enumerate() {
                let rotation = angle * if ring % 2 == 0 { 1.0 } else { -0.7 };
                let points: Vec<_> = (0..400)
                    .filter(|n| ring < 2 || n % 100 < 85)
                    .map(|n| point(radius * factor, f64::from(n) / 400.0 * TAU + rotation))
                    .collect();
                ctx.draw(&Points {
                    coords: &points,
                    color: if ring % 2 == 0 { color } else { dim },
                });
            }
            for offset in [0.0, TAU / 6.0] {
                for index in 0..3 {
                    let a = point(radius * 0.70, angle + offset + f64::from(index) * TAU / 3.0);
                    let b = point(
                        radius * 0.70,
                        angle + offset + f64::from(index + 1) * TAU / 3.0,
                    );
                    ctx.draw(&CanvasLine {
                        x1: a.0,
                        y1: a.1,
                        x2: b.0,
                        y2: b.1,
                        color: dim,
                    });
                }
            }
            for index in 0..24 {
                let theta = f64::from(index) * TAU / 24.0 - angle * 0.5;
                let p = point(radius * 0.85, theta);
                let q = point(radius * 0.91, theta);
                ctx.draw(&CanvasLine {
                    x1: p.0,
                    y1: p.1,
                    x2: q.0,
                    y2: q.1,
                    color: gold,
                });
                if index % 3 == 0 {
                    let glyph = if ascii {
                        "+"
                    } else {
                        ["◇", "△", "⊙", "⋈"][index as usize / 3 % 4]
                    };
                    ctx.print(p.0, p.1, Span::styled(glyph, Style::default().fg(gold)));
                }
            }
            if active || reveal.is_some() {
                for index in 0..18 {
                    let phase = (clock * 0.24 + f64::from(index) / 18.0).fract();
                    let p = point(
                        radius
                            * if reveal.is_some() {
                                (1.0 - progress) * (0.55 + phase * 0.3)
                            } else {
                                0.12 + phase * 0.56
                            },
                        angle + f64::from(index) * TAU / 7.0,
                    );
                    ctx.draw(&Points {
                        coords: &[p],
                        color,
                    });
                }
            }
            for (index, item) in tools.iter().enumerate() {
                let p = point(radius * 0.71, index as f64 * TAU / 8.0 - angle);
                let running = matches!(item.status.as_str(), "inProgress" | "running");
                ctx.print(
                    p.0,
                    p.1,
                    Span::styled(
                        format!("[{}:{}]", index + 1, if running { "*" } else { "+" }),
                        Style::default().fg(if running { gold } else { color }),
                    ),
                );
            }
            ctx.layer();
            for (index, (source, text)) in fragments.iter().enumerate() {
                if text.is_empty() {
                    continue;
                }
                let p = point(
                    radius * [0.91, 0.60, 0.51, 0.77][index],
                    angle * 0.4 + index as f64 * TAU / 4.0 + 0.5,
                );
                let label = format!("{source} · {text}");
                let units_per_cell = 2.0 * x_bound / f64::from(area.width.saturating_sub(2).max(1));
                let x = (p.0 - label.width() as f64 * units_per_cell / 2.0)
                    .clamp(-x_bound + units_per_cell, x_bound * 0.4);
                ctx.print(
                    x,
                    p.1,
                    Span::styled(
                        label,
                        Style::default().fg(if index == 0 { gold } else { color }),
                    ),
                );
            }
        });
    frame.render_widget(canvas, area);

    let answer = session.answer();
    let has_answer = !answer.is_empty();
    let width = if has_answer {
        (f64::from(area.width) * (0.44 + progress * 0.22)) as u16
    } else {
        28.min(area.width.saturating_sub(4))
    };
    let height = if has_answer {
        (area.height / 2).max(5)
    } else {
        5
    };
    let center = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height.min(area.height),
    );
    frame.render_widget(Clear, center);
    let title = if has_answer {
        " 显现 · F4 阅读完整正文 "
    } else {
        " 阵心 "
    };
    let text = if has_answer {
        safe_text(&answer)
    } else if session.status == Status::Ready {
        "输入你的咒语\nEnter 提交 · Alt+Enter 换行".into()
    } else {
        session.status.label().into()
    };
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(Line::from(title))
                .border_style(Style::default().fg(color)),
        ),
        center,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_stays_on_circle() {
        for i in 0..100 {
            let p = point(80.0, f64::from(i));
            assert!((p.0.hypot(p.1) - 80.0).abs() < 0.00001);
        }
    }

    #[test]
    fn canvas_stays_within_ratatui_braille_grid_capacity() {
        assert!(supported_size(80, 842));
        assert!(!supported_size(80, 843));
        assert!(!supported_size(79, 40));
    }
}
