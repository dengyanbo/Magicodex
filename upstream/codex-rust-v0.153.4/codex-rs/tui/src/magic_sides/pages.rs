//! The two pages of the grimoire: this turn's spells on the left, the casting on the right.

use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::Style;

use super::Motion;
use super::Run;
use super::Tier;
use super::spells::Chronicle;
use super::spells::SpellState;
use crate::magic_style::Palette;

/// Stages of the charge, one per unlocked layer.
pub(super) const STAGES: [&str; 5] = ["描线", "蓄力", "共鸣", "聚灵", "满盈"];
/// Cells a spell name takes, so the details line up.
const NAME_WIDTH: u16 = 8;

/// Appends `text` at `(x, y)`, cut to what is left of `page`.
fn push(runs: &mut Vec<Run>, page: Rect, (x, y): (u16, u16), text: &str, style: Style) {
    if y < page.bottom() {
        runs.extend(Run::fit(x, y, text, page.right().saturating_sub(x), style));
    }
}

/// Header and rule; returns the first row below them.
fn heading(runs: &mut Vec<Run>, page: Rect, title: &str, palette: &Palette) -> u16 {
    push(runs, page, (page.x, page.y), title, palette.bright);
    let rule = "─".repeat(usize::from(page.width));
    push(runs, page, (page.x, page.y + 1), &rule, palette.faint);
    page.y + 2
}

/// The left page: the latest spells, the newest at the bottom.
pub(super) fn grimoire(
    runs: &mut Vec<Run>,
    page: Rect,
    chronicle: &Chronicle,
    tier: Tier,
    palette: &Palette,
    motion: Motion,
) {
    if page.width < 8 || page.height < 3 {
        return;
    }
    let first = heading(runs, page, "◊ 咏唱记录", palette);
    let footer = u16::from(chronicle.helpers > 0);
    let slots = usize::from(page.bottom().saturating_sub(first + footer));
    if chronicle.spells.is_empty() {
        push(runs, page, (page.x, first), "（静候咒文……）", palette.faint);
    }
    let skip = chronicle.spells.len().saturating_sub(slots);
    for (row, spell) in (first..).zip(&chronicle.spells[skip..]) {
        let (glyph, style) = match spell.state {
            SpellState::Casting if motion.tick(3.0) % 2 == 1 => ("○", palette.glint),
            SpellState::Casting => ("●", palette.glint),
            SpellState::Done => ("√", palette.line),
            // The classic style keeps to its two colours, so a failure shows by its mark.
            SpellState::Failed => ("×", palette.faint),
        };
        push(runs, page, (page.x, row), glyph, style);
        let name = if spell.state == SpellState::Casting {
            palette.bright
        } else {
            palette.line
        };
        push(runs, page, (page.x + 2, row), spell.kind.name(), name);
        if tier == Tier::Full && !spell.detail.is_empty() {
            let x = page.x + 2 + NAME_WIDTH + 1;
            push(runs, page, (x, row), &spell.detail, palette.faint);
        }
    }
    if footer == 1 {
        let text = format!("  使魔代施 {} 次", chronicle.helpers);
        push(
            runs,
            page,
            (page.x, page.bottom() - 1),
            &text,
            palette.faint,
        );
    }
}

/// What the right page reports besides the chronicle.
pub(super) struct Status {
    pub(super) elapsed: Duration,
    /// Time to the final answer, while the outlet shows.
    pub(super) took: Option<Duration>,
    pub(super) layers: usize,
}

/// The right page: the clock, the stage, the counts and the intent. Returns the first row
/// below what it wrote.
pub(super) fn status(
    runs: &mut Vec<Run>,
    page: Rect,
    chronicle: &Chronicle,
    status: &Status,
    tier: Tier,
    palette: &Palette,
) -> u16 {
    if page.width < 8 || page.height < 3 {
        return page.y;
    }
    let mut row = heading(runs, page, "◊ 施法状态", palette);
    match status.took {
        Some(took) => {
            let text = format!("神谕降临 · 用时 {}", seconds(took));
            push(runs, page, (page.x, row), &text, palette.glint);
        }
        None => {
            push(runs, page, (page.x, row), "咏唱", palette.line);
            let clock = format!("T+{}", seconds(status.elapsed));
            push(runs, page, (page.x + 6, row), &clock, palette.bright);
        }
    }
    row += 1;
    let stage = status.layers.min(STAGES.len() - 1);
    if tier == Tier::Full {
        for (index, name) in STAGES.iter().enumerate() {
            let style = if status.took.is_some() || index < stage {
                palette.line
            } else if index == stage {
                palette.glint
            } else {
                palette.faint
            };
            push(runs, page, (page.x + 5 * index as u16, row), name, style);
        }
    } else {
        let text = format!("阶段 · {}", STAGES[stage]);
        push(runs, page, (page.x, row), &text, palette.line);
    }
    row += 1;
    let counts = if tier == Tier::Full {
        format!(
            "法术 {} · 使魔 {} · 神谕 {}",
            chronicle.cast, chronicle.summons, chronicle.oracles
        )
    } else {
        format!("法术 {} · 神谕 {}", chronicle.cast, chronicle.oracles)
    };
    push(runs, page, (page.x, row), &counts, palette.line);
    row += 1;
    if tier == Tier::Full
        && let Some(intent) = &chronicle.intent
    {
        push(
            runs,
            page,
            (page.x, row),
            &format!("意图 · {intent}"),
            palette.faint,
        );
        row += 1;
    }
    row
}

fn seconds(duration: Duration) -> String {
    let seconds = duration.as_secs_f64();
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        let whole = duration.as_secs();
        format!("{}m{:02}s", whole / 60, whole % 60)
    }
}
