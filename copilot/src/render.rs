//! Composes one frame: the magic region on top and the child's screen below it.

use std::time::Duration;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use unicode_width::UnicodeWidthChar;

use crate::circle::dissolve;
use crate::circle::sides::SideView;
use crate::circle::state::CIRCLE_ROWS;
use crate::circle::state::MagicCircle;
use crate::circle::state::MagicScene;
use crate::circle::state::MagicView;
use crate::circle::style::Choice;
use crate::circle::style::MagicStyle;
use crate::magic::Magic;
use crate::magic::Phase;
use crate::screen::ChildScreen;

/// Terminal colour for a child cell colour.
fn color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(index) => match index {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::Gray,
            8 => Color::DarkGray,
            9 => Color::LightRed,
            10 => Color::LightGreen,
            11 => Color::LightYellow,
            12 => Color::LightBlue,
            13 => Color::LightMagenta,
            14 => Color::LightCyan,
            15 => Color::White,
            _ => Color::Indexed(index),
        },
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Copies the child's screen into `area`; returns where its cursor is, if shown.
pub(crate) fn child(child: &ChildScreen, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
    let screen = child.screen();
    for row in 0..area.height {
        for col in 0..area.width {
            let target = &mut buf[(area.x + col, area.y + row)];
            let Some(cell) = screen.cell(row, col) else {
                target.reset();
                continue;
            };
            if cell.is_wide_continuation() {
                // Covered by the wide character to its left.
                target.reset();
                continue;
            }
            let mut modifier = Modifier::empty();
            for (on, flag) in [
                (cell.bold(), Modifier::BOLD),
                (cell.dim(), Modifier::DIM),
                (cell.italic(), Modifier::ITALIC),
                (cell.underline(), Modifier::UNDERLINED),
                (cell.inverse(), Modifier::REVERSED),
            ] {
                if on {
                    modifier |= flag;
                }
            }
            let contents = cell.contents();
            let symbol = if contents.is_empty() { " " } else { contents };
            target.set_symbol(symbol).set_style(
                Style::new()
                    .fg(color(cell.fgcolor()))
                    .bg(color(cell.bgcolor()))
                    .add_modifier(modifier),
            );
        }
    }
    let (row, col) = screen.cursor_position();
    (!screen.hide_cursor() && row < area.height && col < area.width)
        .then_some((area.x + col, area.y + row))
}

/// Writes `text` from `x` without passing `limit`, in display cells.
fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, limit: u16, style: Style) -> u16 {
    let mut used = 0;
    for c in text.chars() {
        let width = c.width().unwrap_or(0) as u16;
        if x + used + width > limit {
            break;
        }
        let mut glyph = [0; 4];
        buf.set_stringn(
            x + used,
            y,
            c.encode_utf8(&mut glyph),
            usize::from(width),
            style,
        );
        used += width;
    }
    used
}

fn preview_circle(now: Instant) -> (MagicCircle, Instant) {
    let start = now;
    let charged = start + Duration::from_secs(12);
    let mut circle = MagicCircle::default();
    circle.submit("Magicodex 法阵预览", start);
    circle.reply_delta("选择样式后立即生效", charged);
    (circle, charged)
}

fn picker(magic: &Magic, index: usize, area: Rect, buf: &mut Buffer, now: Instant) {
    let wide = area.width >= 100;
    let list_width = if wide { area.width - 46 } else { area.width };
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let x = area.x + 2;
    let limit = area.x + list_width;
    put(buf, x, area.y, "✦ Magic circle styles", limit, bold);
    if area.height > 1 {
        let hint = "↑↓ 预览 · Enter 选用 · 1–9/0 直接选 · Esc 取消";
        put(buf, x + 2, area.y + 1, hint, limit, dim);
    }
    let rows = area.height.saturating_sub(3) as usize;
    let first = index.saturating_sub(rows.saturating_sub(1));
    let items = MagicStyle::ALL
        .iter()
        .map(|style| {
            let current = !magic.random && *style == magic.style;
            (style.label(), style.description(), current)
        })
        .chain([(
            Choice::Random.label(),
            Choice::RANDOM_DESCRIPTION,
            magic.random,
        )]);
    for (offset, (label, description, current)) in items.enumerate().skip(first).take(rows) {
        let y = area.y + 3 + (offset - first) as u16;
        let selected = offset == index;
        let marker = if selected { "›" } else { " " };
        let current = if current { " (current)" } else { "" };
        // Digits pick the first ten items, 0 the tenth; the random choice has none.
        let number = match offset {
            0..=9 => ((offset + 1) % 10).to_string(),
            _ => "?".to_string(),
        };
        let name = format!("{marker} {number}. {label}{current}");
        let name_style = if selected {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let used = put(buf, x, y, &name, limit, name_style);
        let column = x + used.max(24) + 2;
        if column < limit {
            put(buf, column, y, description, limit, dim);
        }
    }
    if wide {
        let (circle, at) = preview_circle(now);
        MagicView {
            circle: &circle,
            style: magic.shown_style(),
            animations_enabled: false,
            scene: MagicScene::Live,
        }
        .render_at(
            Rect::new(
                area.x + list_width,
                area.y,
                area.width - list_width,
                area.height,
            ),
            buf,
            at,
        );
    }
}

/// The circle and, while a turn is under way, the art beside it, as they are at `at`.
fn scene(magic: &Magic, area: Rect, buf: &mut Buffer, at: Instant) {
    let outlet = magic.outlet(at);
    let scene = if outlet.is_some() {
        MagicScene::Outlet
    } else {
        MagicScene::Live
    };
    MagicView {
        circle: &magic.circle,
        style: magic.shown_style(),
        animations_enabled: magic.animations,
        scene,
    }
    .render_at(area, buf, at);
    if magic.phase != Phase::Idle {
        SideView {
            circle: &magic.circle,
            style: magic.shown_style(),
            animations: magic.animations,
            outlet,
        }
        .render(area, buf, at);
    }
}

/// Draws the magic region: the circle, or the picker, and any notice.
pub(crate) fn region(magic: &Magic, area: Rect, buf: &mut Buffer, now: Instant) {
    if area.height == 0 {
        return;
    }
    if let Some(picker_state) = magic.picker {
        picker(magic, picker_state.index, area, buf, now);
    } else if magic.enabled && area.height >= crate::magic::IDLE_ROWS {
        if let Phase::Fading { since, .. } = magic.phase {
            // The circle ends as it was when it stopped, dimming and scattering from its centre.
            let mut frame = Buffer::empty(area);
            scene(magic, area, &mut frame, since);
            let center = (
                f64::from(area.x) * 2.0 + f64::from(area.width),
                f64::from(area.y) * 4.0 + f64::from(area.height.min(CIRCLE_ROWS)) * 2.0,
            );
            dissolve::draw(&frame, buf, center, magic.fade(now), magic.animations);
        } else {
            scene(magic, area, buf, now);
        }
    }
    if let Some(text) = magic.notice(now)
        && magic.picker.is_none()
    {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let limit = area.x + area.width;
        for (row, line) in (area.y..area.y + area.height).zip(text.lines()) {
            let (bullet, style) = if row == area.y {
                ("• ", Style::new())
            } else {
                ("  ", dim)
            };
            let used = put(buf, area.x + 1, row, bullet, limit, dim);
            put(buf, area.x + 1 + used, row, line, limit, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn text(buf: &Buffer) -> Vec<String> {
        buf.content()
            .chunks(usize::from(buf.area.width))
            .map(|row| {
                let mut line = String::new();
                let mut skip = 0;
                for cell in row {
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    line.push_str(cell.symbol());
                    skip = cell.symbol().width().saturating_sub(1);
                }
                line.trim_end().to_string()
            })
            .collect()
    }

    #[test]
    fn the_child_is_drawn_below_the_region_with_its_styles() {
        let mut child = ChildScreen::new(6, 40);
        child.process("\x1b[1;1H\x1b[1;38;2;1;2;3mCopilot\x1b[0m 中文\x1b[2;3H".as_bytes());
        let area = Rect::new(0, 0, 40, 11);
        let mut buf = Buffer::empty(area);
        let now = Instant::now();
        let magic = Magic::new(true, MagicStyle::Classic, true);
        region(&magic, Rect::new(0, 0, 40, 5), &mut buf, now);
        let cursor = super::child(&child, Rect::new(0, 5, 40, 6), &mut buf);
        let rows = text(&buf);
        assert!(
            rows[..5].iter().any(|row| row.contains('✦')),
            "idle emblem: {rows:?}"
        );
        assert_eq!(rows[5], "Copilot 中文");
        assert_eq!(buf[(0, 5)].fg, Color::Rgb(1, 2, 3));
        assert!(buf[(0, 5)].modifier.contains(Modifier::BOLD));
        assert_eq!(cursor, Some((2, 6)));
    }

    #[test]
    fn the_picker_lists_every_style_with_a_preview() {
        let area = Rect::new(0, 0, 120, 21);
        let mut buf = Buffer::empty(area);
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Fire, true);
        magic.command("list", 40, now);
        region(&magic, area, &mut buf, now);
        let rows = text(&buf).join("\n");
        for style in MagicStyle::ALL {
            assert!(
                rows.contains(&style.label()),
                "{} missing:\n{rows}",
                style.id()
            );
        }
        assert!(rows.contains("› 3. fire 火 (current)"), "{rows}");
        assert!(rows.contains("  0. tech 科技"), "{rows}");
        assert!(rows.contains("  ?. random 随机"), "{rows}");
        let braille = rows
            .chars()
            .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
            .count();
        assert!(braille > 100, "preview circle:\n{rows}");
    }

    #[test]
    fn the_sides_show_only_while_a_turn_casts() {
        let area = Rect::new(0, 0, 120, 21);
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Classic, true);
        let drawn = |magic: &Magic, area: Rect| {
            let mut buf = Buffer::empty(area);
            region(magic, area, &mut buf, now);
            text(&buf).join("\n")
        };
        assert!(
            !drawn(&magic, Rect::new(0, 0, 120, 5)).contains("咏唱记录"),
            "idle stays clean"
        );
        magic.handle(crate::session::Event::Prompt("Draw".into()), now);
        let rows = drawn(&magic, area);
        assert!(
            rows.contains("咏唱记录") && rows.contains("施法状态"),
            "{rows}"
        );
        magic.command("list", 40, now);
        assert!(
            !drawn(&magic, area).contains("咏唱记录"),
            "the picker has the region"
        );
    }

    #[test]
    fn a_finished_circle_dims_and_scatters_before_the_idle_emblem() {
        use crate::session::Event;
        let area = Rect::new(0, 0, 120, 24);
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Fire, true);
        magic.handle(Event::Prompt("Draw a circle".into()), now);
        magic.handle(
            Event::Reply {
                text: "Done".into(),
                tools: false,
            },
            now,
        );
        magic.handle(Event::TurnEnd, now);
        let opened = now + Duration::from_secs(2);
        magic.tick(opened);
        let ended = opened + Duration::from_secs(15);
        magic.tick(ended);
        assert!(
            matches!(magic.phase, Phase::Fading { .. }),
            "{:?}",
            magic.phase
        );
        let drawn = |magic: &Magic, at: Instant| {
            let mut buf = Buffer::empty(area);
            region(magic, area, &mut buf, at);
            buf
        };
        let dots = |buf: &Buffer| -> u32 {
            buf.content()
                .iter()
                .filter_map(|cell| cell.symbol().chars().next())
                .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
                .map(|c| (u32::from(c) - 0x2800).count_ones())
                .sum()
        };
        let start = drawn(&magic, ended);
        let middle = drawn(&magic, ended + Duration::from_millis(800));
        let late = drawn(&magic, ended + Duration::from_millis(1200));
        assert!(dots(&start) > 500, "the settled outlet: {}", dots(&start));
        assert!(
            dots(&late) * 2 < dots(&start),
            "{} of {} dots",
            dots(&late),
            dots(&start)
        );
        assert!(
            middle.content().iter().all(
                |cell| cell.symbol().trim().is_empty() || cell.modifier.contains(Modifier::DIM)
            ),
            "everything dims"
        );
        magic.tick(ended + Duration::from_millis(1600));
        assert_eq!(magic.phase, Phase::Idle);
        let idle = drawn(&magic, ended + Duration::from_millis(1600));
        assert!(
            !text(&idle).join("\n").contains("施法状态"),
            "the sides are gone"
        );
    }

    #[test]
    fn the_command_hint_sits_beside_the_idle_emblem() {
        let area = Rect::new(0, 0, 120, 5);
        let now = Instant::now();
        let mut plain = Buffer::empty(area);
        let mut magic = Magic::new(true, MagicStyle::Classic, true);
        region(&magic, area, &mut plain, now);
        let mut buf = Buffer::empty(area);
        magic.set_command_hint(true);
        region(&magic, area, &mut buf, now);
        let rows = text(&buf);
        assert!(
            rows[0].starts_with(" • /magic on|off|list|random|<类型>"),
            "{rows:?}"
        );
        assert!(rows[1].starts_with("   magicopilot 的命令"), "{rows:?}");
        assert!(buf[(3, 1)].modifier.contains(Modifier::DIM));
        for y in 0..area.height {
            for x in 55..area.width {
                assert_eq!(buf[(x, y)], plain[(x, y)], "emblem cell ({x}, {y})");
            }
        }
    }
}
