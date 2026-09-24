//! What the columns beside a charging circle show: a page of the grimoire on each side, a
//! pillar next to the circle, rune particles in the free cells, and a familiar.
//!
//! Everything comes from what the circle already knows: the turn's clock, the layers it has
//! unlocked, and the public tool, subagent and skill events. Nothing here reads reasoning.

mod familiar;
mod pages;
mod particles;
mod pillars;
pub(crate) mod spells;

use std::time::Duration;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::magic_circle::CIRCLE_ROWS;
use crate::magic_circle::MAX_WIDTH;
use crate::magic_circle::MagicCircle;
use crate::magic_circle::MagicScene;
use crate::magic_circle::MagicView;
use crate::magic_circle::OUTLET_ROWS;
use crate::magic_circle::odd;
use crate::magic_style::MagicStyle;
use crate::render::renderable::Renderable;

/// Cells that any style may draw on either side of the circle's centre column, its outlet
/// included. The tests measure every style against it.
pub(crate) const FOOTPRINT: u16 = 23;
/// Blank cells between the circle and what is beside it.
const GAP: u16 = 1;
const PILLAR_WIDTH: u16 = 3;
/// Blank cells between a pillar and its page.
const PAGE_GAP: u16 = 2;
const PAGE_MAX: u16 = 34;
/// Rows the sides need; a permission prompt shrinks the region below this.
const MIN_ROWS: u16 = 12;
/// Rows at the top kept for the region's notices.
const NOTICE_ROWS: u16 = 2;

/// How much a width allows, from the narrower side's columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Tier {
    /// Pillars and particles only.
    Decor,
    /// Pages with the essentials.
    Compact,
    /// Pages with details, and the familiar.
    Full,
}

/// Where the sides go around a circle that `MagicView` draws in the same area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) tier: Tier,
    pub(crate) left: Rect,
    pub(crate) right: Rect,
    /// Rows `[top, bottom)` of the circle itself.
    pub(crate) circle_rows: (u16, u16),
}

impl Layout {
    pub(crate) fn of(area: Rect, outlet: bool) -> Option<Self> {
        if area.height < MIN_ROWS {
            return None;
        }
        let canvas = odd(area.width.min(MAX_WIDTH));
        let center = area.x + (area.width - canvas) / 2 + canvas / 2;
        let left_end = center.checked_sub(FOOTPRINT + GAP)?.max(area.x);
        let right_start = (center + FOOTPRINT + GAP + 1).min(area.right());
        let left = Rect::new(area.x, area.y, left_end - area.x, area.height);
        let right = Rect::new(right_start, area.y, area.right() - right_start, area.height);
        let tier = match left.width.min(right.width) {
            0..8 => return None,
            8..20 => Tier::Decor,
            20..30 => Tier::Compact,
            _ => Tier::Full,
        };
        let cone = if outlet { OUTLET_ROWS } else { 0 };
        let rows = odd(area.height.saturating_sub(cone).min(CIRCLE_ROWS));
        let top = area.y + (area.height - rows - cone) / 2;
        Some(Self {
            tier,
            left,
            right,
            circle_rows: (top, top + rows),
        })
    }

    /// A pillar next to each side of the circle.
    fn pillars(&self) -> [Rect; 2] {
        let (top, bottom) = self.circle_rows;
        let height = (bottom - top).saturating_sub(4);
        [
            Rect::new(
                self.left.right() - PILLAR_WIDTH,
                top + 2,
                PILLAR_WIDTH,
                height,
            ),
            Rect::new(self.right.x, top + 2, PILLAR_WIDTH, height),
        ]
    }

    /// A page between each pillar and the outer edge, below the notices.
    fn pages(&self) -> [Rect; 2] {
        let y = self.left.y + NOTICE_ROWS;
        let height = self.left.height.saturating_sub(NOTICE_ROWS + 1);
        let left_end = self.left.right().saturating_sub(PILLAR_WIDTH + PAGE_GAP);
        let left_start = (self.left.x + 1).max(left_end.saturating_sub(PAGE_MAX));
        let right_start = self.right.x + PILLAR_WIDTH + PAGE_GAP;
        let right_end = self
            .right
            .right()
            .saturating_sub(1)
            .min(right_start + PAGE_MAX);
        [
            Rect::new(left_start, y, left_end.saturating_sub(left_start), height),
            Rect::new(
                right_start,
                y,
                right_end.saturating_sub(right_start),
                height,
            ),
        ]
    }
}

/// Motion shared by the side art; everything stands still without animations.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Motion {
    /// Seconds that drive motion; zero without animations.
    pub(crate) spin: f64,
    pub(crate) animated: bool,
    /// Layers the circle has unlocked, 0 to 4.
    pub(crate) layers: usize,
    /// How far the outlet has run, 0 to 1, once the answer has arrived.
    pub(crate) outlet: Option<f64>,
}

impl Motion {
    /// Integer time step that changes `rate` times a second.
    pub(crate) fn tick(&self, rate: f64) -> u64 {
        (self.spin * rate) as u64
    }

    /// Fraction of a pillar that glows: a fifth per layer, all of it at the outlet.
    pub(crate) fn lit(&self) -> f64 {
        if self.outlet.is_some() {
            1.0
        } else {
            (self.layers.min(4) + 1) as f64 / 5.0
        }
    }
}

/// A line of side text, already cut to its room.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) text: String,
    pub(crate) style: Style,
}

impl Run {
    /// `text` at `(x, y)`, cut to `width` cells with an ellipsis when it had to be cut.
    pub(crate) fn fit(x: u16, y: u16, text: &str, width: u16, style: Style) -> Option<Self> {
        let width = usize::from(width);
        if width == 0 || text.is_empty() {
            return None;
        }
        let text = if text.width() <= width {
            text.to_string()
        } else {
            let mut cut = String::new();
            let mut used = 0;
            for c in text.chars() {
                let size = c.width().unwrap_or(0);
                if used + size + 1 > width {
                    break;
                }
                cut.push(c);
                used += size;
            }
            cut.push('…');
            cut
        };
        Some(Self { x, y, text, style })
    }

    fn area(&self) -> Rect {
        Rect::new(self.x, self.y, self.text.width() as u16, 1)
    }

    fn draw(&self, buf: &mut Buffer) {
        let area = buf.area;
        if !(area.y..area.bottom()).contains(&self.y) {
            return;
        }
        let mut x = self.x;
        for c in self.text.chars() {
            let width = c.width().unwrap_or(0) as u16;
            if width == 0 {
                continue;
            }
            if x < area.x || x + width > area.right() {
                break;
            }
            let mut glyph = [0; 4];
            buf.set_stringn(
                x,
                self.y,
                c.encode_utf8(&mut glyph),
                usize::from(width),
                self.style,
            );
            x += width;
        }
    }
}

/// `rect` widened by `cells` on both sides.
fn widen(rect: Rect, cells: u16) -> Rect {
    let x = rect.x.saturating_sub(cells);
    Rect::new(x, rect.y, rect.width + (rect.x - x) + cells, rect.height)
}

/// The answer's arrival, while the outlet shows. The live circle in Codex never shows one;
/// its outlet goes to the transcript without the sides.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Outlet {
    /// Time from the prompt to the final answer.
    pub(crate) took: Duration,
    /// How far the outlet has run, 0 to 1.
    pub(crate) progress: f64,
}

/// The sides of a charging or settled circle.
pub(crate) struct SideView<'a> {
    pub(crate) circle: &'a MagicCircle,
    pub(crate) style: MagicStyle,
    pub(crate) animations: bool,
    pub(crate) outlet: Option<Outlet>,
}

impl SideView<'_> {
    /// Draws beside the circle in `area`, the area its `MagicView` was drawn in.
    pub(crate) fn render(&self, area: Rect, buf: &mut Buffer, now: Instant) {
        let Some(elapsed) = self.circle.elapsed(now) else {
            return;
        };
        let Some(layout) = Layout::of(area, self.outlet.is_some()) else {
            return;
        };
        let palette = self.style.palette();
        let motion = Motion {
            spin: if self.animations {
                elapsed.as_secs_f64()
            } else {
                0.0
            },
            animated: self.animations,
            layers: self.circle.layers(now),
            outlet: self.outlet.map(|outlet| outlet.progress),
        };
        let chronicle = &self.circle.chronicle;
        let mut runs = Vec::new();
        if layout.tier >= Tier::Compact {
            let [left, right] = layout.pages();
            pages::grimoire(&mut runs, left, chronicle, layout.tier, &palette, motion);
            let status = pages::Status {
                elapsed,
                took: self.outlet.map(|outlet| outlet.took),
                layers: motion.layers,
            };
            let used = pages::status(&mut runs, right, chronicle, &status, layout.tier, &palette);
            if layout.tier == Tier::Full {
                familiar::draw(&mut runs, right, used, chronicle, motion, &palette);
            }
        }
        let pillars = layout.pillars();
        let mut keep_out: Vec<Rect> = runs.iter().map(|run| widen(run.area(), 1)).collect();
        keep_out.extend(pillars.iter().map(|pillar| widen(*pillar, 1)));
        for (zone, inward) in [(layout.left, true), (layout.right, false)] {
            particles::draw(buf, zone, inward, &keep_out, self.style, motion, &palette);
        }
        for pillar in pillars {
            pillars::draw(buf, pillar, self.style, motion, &palette);
        }
        for run in &runs {
            run.draw(buf);
        }
    }
}

/// The live circle above the composer, with its sides while a turn charges.
pub(crate) struct LiveView<'a> {
    pub(crate) circle: &'a MagicCircle,
    pub(crate) style: MagicStyle,
    pub(crate) animations_enabled: bool,
}

impl LiveView<'_> {
    fn view(&self) -> MagicView<'_> {
        MagicView {
            circle: self.circle,
            style: self.style,
            animations_enabled: self.animations_enabled,
            scene: MagicScene::Live,
        }
    }
}

impl Renderable for LiveView<'_> {
    fn desired_height(&self, width: u16) -> u16 {
        self.view().desired_height(width)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let now = Instant::now();
        self.view().render_at(area, buf, now);
        SideView {
            circle: self.circle,
            style: self.style,
            animations: self.animations_enabled,
            outlet: None,
        }
        .render(area, buf, now);
    }
}

#[cfg(test)]
#[path = "magic_sides_tests.rs"]
mod tests;
