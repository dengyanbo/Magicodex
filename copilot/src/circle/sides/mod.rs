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

use crate::circle::state::CIRCLE_ROWS;
use crate::circle::state::MAX_WIDTH;
use crate::circle::state::MagicCircle;
use crate::circle::state::OUTLET_ROWS;
use crate::circle::state::odd;
use crate::circle::style::MagicStyle;

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
        if self.y < area.y || self.y >= area.bottom() {
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

/// The answer's arrival, while the outlet shows.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circle::state::MagicScene;
    use crate::circle::state::MagicView;

    /// A circle that charged fully before its first reply, and the moment `age` after its prompt.
    fn charged(age: f64) -> (MagicCircle, Instant) {
        let start = Instant::now();
        let mut circle = MagicCircle::default();
        circle.submit("Magicodex 法阵两侧：魔导书、法阵柱与使魔", start);
        circle.complete_reply(
            "先检查渲染循环，再展开法阵",
            start + Duration::from_secs(20),
        );
        (circle, start + Duration::from_secs_f64(age))
    }

    fn cast(circle: &mut MagicCircle) {
        let chronicle = &mut circle.chronicle;
        chronicle.cast("1", "glob", "*.md", false, false);
        chronicle.resolve("1", true);
        chronicle.cast("2", "view", "app.rs", false, false);
        chronicle.resolve("2", false);
        chronicle.cast("3", "powershell", "cargo test", false, false);
        chronicle.oracle();
    }

    fn circle_only(
        circle: &MagicCircle,
        style: MagicStyle,
        area: Rect,
        now: Instant,
        outlet: bool,
        animations: bool,
    ) -> Buffer {
        let mut buf = Buffer::empty(area);
        let scene = if outlet {
            MagicScene::Outlet
        } else {
            MagicScene::Live
        };
        MagicView {
            circle,
            style,
            animations_enabled: animations,
            scene,
        }
        .render_at(area, &mut buf, now);
        buf
    }

    fn with_sides(
        circle: &MagicCircle,
        style: MagicStyle,
        area: Rect,
        now: Instant,
        animations: bool,
        outlet: Option<Outlet>,
    ) -> Buffer {
        let mut buf = circle_only(circle, style, area, now, outlet.is_some(), animations);
        SideView {
            circle,
            style,
            animations,
            outlet,
        }
        .render(area, &mut buf, now);
        buf
    }

    fn text(buf: &Buffer, columns: std::ops::Range<u16>) -> String {
        let mut text = String::new();
        for y in 0..buf.area.height {
            let mut skip = 0;
            for x in columns.clone() {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let symbol = buf[(x, y)].symbol();
                text.push_str(symbol);
                skip = symbol.width().saturating_sub(1);
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn every_style_stays_inside_the_footprint() {
        let center = 59;
        for style in MagicStyle::ALL {
            for age in [21.0, 25.3, 33.7] {
                for (height, outlet) in [(21, false), (24, true)] {
                    let (circle, now) = charged(age);
                    let buf = circle_only(
                        &circle,
                        style,
                        Rect::new(0, 0, 120, height),
                        now,
                        outlet,
                        true,
                    );
                    for y in 0..height {
                        for x in 0..120 {
                            if buf[(x, y)].symbol() != " " {
                                assert!(
                                    x.abs_diff(center) <= FOOTPRINT,
                                    "{} at age {age} draws column {x}",
                                    style.id()
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_width_decides_what_fits() {
        let tier = |width| Layout::of(Rect::new(0, 0, width, 21), false).map(|layout| layout.tier);
        assert_eq!(tier(60), None);
        assert_eq!(tier(80), Some(Tier::Decor));
        assert_eq!(tier(100), Some(Tier::Compact));
        assert_eq!(tier(120), Some(Tier::Full));
        assert_eq!(Layout::of(Rect::new(0, 0, 120, MIN_ROWS - 1), false), None);
        let layout = Layout::of(Rect::new(0, 0, 120, 21), false).unwrap();
        assert_eq!(layout.left.right(), 59 - FOOTPRINT - GAP);
        assert_eq!(layout.right.x, 59 + FOOTPRINT + GAP + 1);
    }

    #[test]
    fn the_sides_leave_the_circle_alone() {
        let area = Rect::new(0, 0, 120, 21);
        for style in MagicStyle::ALL {
            let (mut circle, now) = charged(24.0);
            cast(&mut circle);
            let plain = circle_only(&circle, style, area, now, false, true);
            let full = with_sides(&circle, style, area, now, true, None);
            for y in 0..area.height {
                for x in 59 - FOOTPRINT - GAP..=59 + FOOTPRINT + GAP {
                    assert_eq!(
                        full[(x, y)],
                        plain[(x, y)],
                        "{} cell ({x}, {y})",
                        style.id()
                    );
                }
            }
            for side in [0..35, 84..120] {
                assert!(
                    text(&full, side)
                        .chars()
                        .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
                    "{} art",
                    style.id()
                );
            }
        }
    }

    #[test]
    fn the_pages_tell_the_turn() {
        let (mut circle, now) = charged(24.0);
        cast(&mut circle);
        let full = with_sides(
            &circle,
            MagicStyle::Fire,
            Rect::new(0, 0, 120, 21),
            now,
            true,
            None,
        );
        let (left, right) = (text(&full, 0..35), text(&full, 84..120));
        for expected in [
            "咏唱记录",
            "寻踪术",
            "*.md",
            "√",
            "×",
            "召唤仪式",
            "cargo test",
        ] {
            assert!(left.contains(expected), "{expected} missing:\n{left}");
        }
        for expected in [
            "施法状态",
            "T+24.0s",
            "满盈",
            "法术 3 · 使魔 0 · 神谕 1",
            "( °o° )",
            "使魔 · 跑腿 召唤仪式",
        ] {
            assert!(right.contains(expected), "{expected} missing:\n{right}");
        }
        let compact = with_sides(
            &circle,
            MagicStyle::Fire,
            Rect::new(0, 0, 100, 21),
            now,
            true,
            None,
        );
        let (left, right) = (text(&compact, 0..25), text(&compact, 74..100));
        assert!(left.contains("寻踪术") && !left.contains("*.md"), "{left}");
        assert!(
            right.contains("阶段 · 满盈") && !right.contains("使魔 ·"),
            "{right}"
        );
        let narrow = with_sides(
            &circle,
            MagicStyle::Fire,
            Rect::new(0, 0, 80, 21),
            now,
            true,
            None,
        );
        assert!(
            !text(&narrow, 0..80).contains("寻踪术"),
            "only art fits in 80 columns"
        );
        let settled = Some(Outlet {
            took: Duration::from_secs(23),
            progress: 0.1,
        });
        let outlet = with_sides(
            &circle,
            MagicStyle::Fire,
            Rect::new(0, 0, 120, 24),
            now,
            true,
            settled,
        );
        let right = text(&outlet, 84..120);
        assert!(
            right.contains("神谕降临 · 用时 23.0s") && right.contains("使魔 · 欢呼！"),
            "{right}"
        );
    }

    #[test]
    fn without_animations_the_sides_stand_still() {
        // Only the sides: the tech circle shows a real clock even without animations.
        let area = Rect::new(0, 0, 80, 21);
        let sides = |buf: &Buffer| -> Vec<ratatui::buffer::Cell> {
            let band = 39 - FOOTPRINT - GAP..=39 + FOOTPRINT + GAP;
            (0..area.height)
                .flat_map(|y| (0..area.width).map(move |x| (x, y)))
                .filter(|(x, _)| !band.contains(x))
                .map(|position| buf[position].clone())
                .collect()
        };
        for style in MagicStyle::ALL {
            let (circle, now) = charged(24.0);
            let later = now + Duration::from_millis(1700);
            let still = sides(&with_sides(&circle, style, area, now, false, None));
            assert_eq!(
                still,
                sides(&with_sides(&circle, style, area, later, false, None)),
                "{}",
                style.id()
            );
            let moving = sides(&with_sides(&circle, style, area, now, true, None));
            assert_ne!(
                moving,
                sides(&with_sides(&circle, style, area, later, true, None)),
                "{} moves",
                style.id()
            );
        }
    }
}
