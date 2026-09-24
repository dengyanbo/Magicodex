//! The familiar at the foot of the right page, acting out what the turn is doing.

use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::Motion;
use super::Run;
use super::spells::Chronicle;
use super::spells::SpellKind;
use crate::magic_style::Palette;

/// Cells of the body with its charm.
const WIDTH: u16 = 9;
/// Three rows of body and a caption.
const ROWS: u16 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mood {
    /// Waiting for the model.
    Chant,
    /// A tool is running.
    Run,
    /// A subagent is running.
    Summon,
    /// The answer has arrived.
    Cheer,
}

pub(super) fn mood(chronicle: &Chronicle, outlet: bool) -> Mood {
    if outlet {
        return Mood::Cheer;
    }
    match chronicle.casting() {
        Some(spell) if spell.kind == SpellKind::Summon => Mood::Summon,
        Some(_) => Mood::Run,
        None => Mood::Chant,
    }
}

/// The body, and a charm beside it at `(column, row)`.
fn frame(mood: Mood, step: u64) -> ([&'static str; 3], (u16, u16), &'static str) {
    let even = step.is_multiple_of(2);
    let beside = if even { (8, 0) } else { (8, 2) };
    match mood {
        Mood::Chant => {
            let eyes = if step % 6 == 5 { "( -.- )" } else { "( o.o )" };
            let note = if even { "♪" } else { "♫" };
            ([" /\\_/\\ ", eyes, " > ^ < "], (8, u16::from(even)), note)
        }
        Mood::Run => {
            let legs = if even { " /   \\ " } else { " \\   / " };
            (
                [" /\\_/\\ ", "( °o° )", legs],
                (7, 1),
                if even { "≡" } else { "=" },
            )
        }
        Mood::Summon => ([" /\\_/\\ ", "\\(o.o)/", " (   ) "], beside, "•"),
        Mood::Cheer => {
            let face = if even { "\\(^.^)/" } else { "\\(^o^)/" };
            ([" /\\_/\\ ", face, "  ) (  "], beside, "☼")
        }
    }
}

fn caption(mood: Mood, chronicle: &Chronicle) -> String {
    match mood {
        Mood::Chant => "使魔 · 吟唱中".to_string(),
        Mood::Run => {
            let spell = chronicle.casting().map_or("", |spell| spell.kind.name());
            format!("使魔 · 跑腿 {spell}")
        }
        Mood::Summon => "使魔 · 召唤同伴".to_string(),
        Mood::Cheer => "使魔 · 欢呼！".to_string(),
    }
}

/// Draws the familiar at the foot of `page`, when it fits below row `used`.
pub(super) fn draw(
    runs: &mut Vec<Run>,
    page: Rect,
    used: u16,
    chronicle: &Chronicle,
    motion: Motion,
    palette: &Palette,
) {
    if page.width < WIDTH + 2 || page.bottom() < used + ROWS + 1 {
        return;
    }
    let mood = mood(chronicle, motion.outlet.is_some());
    let (body, (column, row), charm) = frame(mood, motion.tick(2.0));
    let top = page.bottom() - ROWS;
    let x = page.x + (page.width - WIDTH) / 2;
    for (offset, line) in body.iter().enumerate() {
        runs.extend(Run::fit(x, top + offset as u16, line, WIDTH, palette.line));
    }
    runs.extend(Run::fit(x + column, top + row, charm, 1, palette.accent));
    let caption = caption(mood, chronicle);
    let left = page.x + page.width.saturating_sub(caption.width() as u16) / 2;
    runs.extend(Run::fit(
        left,
        top + 3,
        &caption,
        page.right() - left,
        palette.faint,
    ));
}
