//! A pillar on each side of the circle, lighting up from its base as the circle gains layers.
//!
//! Pillars are six dots wide. Coordinates here are dots from the pillar's top left corner.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::Motion;
use crate::magic_canvas::Canvas;
use crate::magic_canvas::Ink;
use crate::magic_canvas::noise;
use crate::magic_style::MagicStyle;
use crate::magic_style::Palette;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Structure: faint until lit.
    Frame,
    /// Inlays: brighter than the frame once lit.
    Core,
    /// The style's second colour, faint until lit.
    Accent,
    /// Sparks, only where lit.
    Spark,
    /// The second colour whatever the charge, e.g. flames.
    Flame,
}

struct Pen<'c, 'a> {
    canvas: &'c mut Canvas<'a>,
    height: f64,
    /// Dots from this row down are lit.
    lit_from: f64,
    /// A highlight rising through the lit part.
    band: Option<f64>,
    pulse: bool,
}

impl Pen<'_, '_> {
    fn dot(&mut self, x: f64, y: f64, part: Part) {
        let (x, y) = (x.round(), y.round());
        if !(0.0..=5.0).contains(&x) || !(0.0..self.height).contains(&y) {
            return;
        }
        let lit = y >= self.lit_from;
        let hot = self.pulse || self.band.is_some_and(|band| (y - band).abs() < 2.0);
        let ink = match part {
            Part::Flame if hot => Ink::Glint,
            Part::Flame => Ink::Accent,
            Part::Spark if !lit => return,
            _ if !lit => Ink::Faint,
            Part::Frame if hot => Ink::Bright,
            Part::Frame => Ink::Line,
            Part::Core | Part::Accent if hot => Ink::Glint,
            Part::Core => Ink::Bright,
            Part::Accent => Ink::Accent,
            Part::Spark => Ink::Glint,
        };
        self.canvas.plot(x - 2.5, y, ink);
    }

    fn column(&mut self, x: f64, from: f64, to: f64, part: Part) {
        let mut y = from;
        while y < to {
            self.dot(x, y, part);
            y += 1.0;
        }
    }

    fn row(&mut self, y: f64, part: Part) {
        for x in 0..6 {
            self.dot(f64::from(x), y, part);
        }
    }
}

pub(super) fn draw(
    buf: &mut Buffer,
    area: Rect,
    style: MagicStyle,
    motion: Motion,
    palette: &Palette,
) {
    if area.width < 3 || area.height < 4 {
        return;
    }
    let mut canvas = Canvas::new(area.width, area.height, 0.0);
    let height = f64::from(area.height) * 4.0;
    let mut pen = Pen {
        canvas: &mut canvas,
        height,
        lit_from: height * (1.0 - motion.lit()),
        band: motion
            .animated
            .then(|| height - (motion.spin * 7.0) % (height + 8.0)),
        pulse: motion
            .outlet
            .is_some_and(|progress| (progress * 14.0).sin() > 0.3),
    };
    match style {
        MagicStyle::Classic => classic(&mut pen),
        MagicStyle::Wind => helix(&mut pen, motion, 3.0),
        MagicStyle::Fire => fire(&mut pen, motion),
        MagicStyle::Water => water(&mut pen, motion),
        MagicStyle::Thunder => thunder(&mut pen, motion),
        MagicStyle::Earth => earth(&mut pen),
        MagicStyle::Holy => holy(&mut pen, motion),
        MagicStyle::Dark => tendrils(&mut pen, motion),
        MagicStyle::Eerie => seam(&mut pen, motion),
        MagicStyle::Tech => meter(&mut pen, motion),
    }
    canvas.paint(area, buf, palette);
}

/// An obelisk with a capstone and alternating runes.
fn classic(pen: &mut Pen) {
    let h = pen.height;
    pen.dot(2.0, 0.0, Part::Core);
    pen.dot(3.0, 0.0, Part::Core);
    for (x, y) in [(1.0, 1.0), (4.0, 1.0), (0.0, 2.0), (5.0, 2.0)] {
        pen.dot(x, y, Part::Frame);
    }
    pen.column(0.0, 3.0, h - 1.0, Part::Frame);
    pen.column(5.0, 3.0, h - 1.0, Part::Frame);
    pen.row(h - 1.0, Part::Frame);
    let (mut y, mut rune) = (6.0, 0);
    while y + 2.0 < h - 2.0 {
        let (a, b) = if rune % 2 == 0 {
            (2.0, 3.0)
        } else {
            (3.0, 2.0)
        };
        pen.dot(a, y, Part::Core);
        pen.dot(b, y + 1.0, Part::Core);
        pen.dot(a, y + 2.0, Part::Core);
        y += 6.0;
        rune += 1;
    }
}

/// Two strands winding around each other, rising at `speed`.
fn helix(pen: &mut Pen, motion: Motion, speed: f64) {
    for y in 0..pen.height as u64 {
        let y = y as f64;
        let wave = (y * 0.45 - motion.spin * speed).sin() * 2.4;
        pen.dot(2.5 + wave, y, Part::Frame);
        pen.dot(2.5 - wave, y, Part::Core);
    }
}

/// A brazier whose flames grow with the charge, and embers rising in its shaft.
fn fire(pen: &mut Pen, motion: Motion) {
    let h = pen.height;
    let flame = 1.0 + 5.0 * motion.lit();
    let tick = motion.tick(8.0);
    for y in 0..6_u64 {
        let height = y as f64 - (6.0 - flame);
        if height < 0.0 {
            continue;
        }
        for x in 1..5_u64 {
            if noise(&[x, y, tick]) < 0.3 + 0.1 * height {
                pen.dot(x as f64, y as f64, Part::Flame);
            }
        }
    }
    pen.dot(0.0, 6.0, Part::Frame);
    pen.dot(5.0, 6.0, Part::Frame);
    pen.row(7.0, Part::Frame);
    pen.column(1.0, 8.0, h - 1.0, Part::Frame);
    pen.column(4.0, 8.0, h - 1.0, Part::Frame);
    pen.row(h - 1.0, Part::Frame);
    let shaft = (h as u64).saturating_sub(10).max(1);
    for k in 0..3_u64 {
        let rise = (motion.tick(6.0) + k * 5) % shaft;
        pen.dot(2.0 + (k % 2) as f64, h - 2.0 - rise as f64, Part::Spark);
    }
}

/// A fall of water between two walls, filling a pool from the base.
fn water(pen: &mut Pen, motion: Motion) {
    let h = pen.height;
    pen.row(0.0, Part::Core);
    pen.column(0.0, 1.0, h, Part::Frame);
    pen.column(5.0, 1.0, h, Part::Frame);
    let t = motion.tick(6.0) as i64;
    let level = pen.lit_from.ceil().max(1.0) as i64;
    for y in 1..h as i64 {
        if (y - t).rem_euclid(4) < 2 {
            pen.dot(2.0, y as f64, Part::Frame);
        }
        if (y - t + 2).rem_euclid(4) < 2 {
            pen.dot(3.0, y as f64, Part::Frame);
        }
        if y >= level {
            for x in 1..5_i64 {
                if (x + y + t).rem_euclid(3) == 0 {
                    pen.dot(x as f64, y as f64, Part::Accent);
                }
            }
        }
    }
}

/// A coil with rings, arcing at the top once well charged.
fn thunder(pen: &mut Pen, motion: Motion) {
    let h = pen.height;
    pen.column(2.0, 0.0, h, Part::Frame);
    pen.column(3.0, 0.0, h, Part::Frame);
    let mut y = 2.0;
    while y < h {
        pen.row(y, Part::Core);
        y += 5.0;
    }
    if motion.lit() >= 0.6 {
        let tick = motion.tick(10.0);
        for y in 0..6_u64 {
            pen.dot((noise(&[y, tick]) * 5.0).round(), y as f64, Part::Flame);
        }
    }
}

/// Stacked stone blocks with a weathered face.
fn earth(pen: &mut Pen) {
    let h = pen.height;
    pen.column(0.0, 0.0, h, Part::Frame);
    pen.column(5.0, 0.0, h, Part::Frame);
    let mut y = 0.0;
    while y < h {
        pen.row(y, Part::Frame);
        y += 8.0;
    }
    pen.row(h - 1.0, Part::Frame);
    for y in 0..h as u64 {
        for x in 1..5_u64 {
            if y % 8 != 0 && noise(&[x, y, 7]) < 0.2 {
                pen.dot(x as f64, y as f64, Part::Core);
            }
        }
    }
}

/// A beam of light under a star, with sparks rising beside it.
fn holy(pen: &mut Pen, motion: Motion) {
    let h = pen.height;
    pen.column(2.0, 2.0, h, Part::Core);
    pen.column(3.0, 2.0, h, Part::Core);
    let mut y = 3.0;
    while y < h {
        pen.dot(1.0, y, Part::Frame);
        pen.dot(4.0, y, Part::Frame);
        y += 2.0;
    }
    for (x, y) in [(2.0, 0.0), (3.0, 0.0), (1.0, 1.0), (4.0, 1.0)] {
        pen.dot(x, y, Part::Flame);
    }
    for k in 0..3_u64 {
        let rise = (motion.tick(5.0) + k * 9) % h as u64;
        let x = if k % 2 == 0 { 0.0 } else { 5.0 };
        pen.dot(x, h - 1.0 - rise as f64, Part::Spark);
    }
}

/// Two tendrils swaying apart, with drops falling between them.
fn tendrils(pen: &mut Pen, motion: Motion) {
    let h = pen.height;
    for y in 0..h as u64 {
        let y = y as f64;
        let wave = (y * 0.35 + motion.spin * 1.5).sin() * 2.2;
        pen.dot(2.5 + wave, y, Part::Frame);
        pen.dot(2.5 - wave, y, Part::Core);
    }
    for k in 0..2_u64 {
        let fall = (motion.tick(4.0) + k * 7) % h as u64;
        pen.dot(2.0 + k as f64, fall as f64, Part::Accent);
    }
}

/// A stitched seam with an eye that wanders and blinks.
fn seam(pen: &mut Pen, motion: Motion) {
    let h = pen.height;
    pen.column(2.0, 0.0, h, Part::Frame);
    let mut y = 1.0;
    while y < h {
        let skew = (noise(&[y as u64, 3]) * 2.0).round() - 1.0;
        pen.dot(1.0 + skew.max(0.0), y, Part::Core);
        pen.dot(3.0, y, Part::Core);
        pen.dot(4.0 + skew.min(0.0), y, Part::Core);
        y += 4.0;
    }
    let room = (h as u64).saturating_sub(8).max(1);
    let eye = ((motion.tick(0.4) * 7) % room + 4) as f64;
    for (x, dy) in [
        (1.0, 0.0),
        (4.0, 0.0),
        (2.0, -1.0),
        (3.0, -1.0),
        (2.0, 1.0),
        (3.0, 1.0),
    ] {
        pen.dot(x, eye + dy, Part::Flame);
    }
    if motion.tick(2.0) % 9 != 8 {
        pen.dot(2.0 + (motion.tick(1.0) % 2) as f64, eye, Part::Spark);
    }
}

/// A segmented gauge in a frame, with a scan dot on its edge.
fn meter(pen: &mut Pen, motion: Motion) {
    let h = pen.height;
    pen.column(0.0, 0.0, h, Part::Frame);
    pen.column(5.0, 0.0, h, Part::Frame);
    pen.row(0.0, Part::Frame);
    pen.row(h - 1.0, Part::Frame);
    let mut base = h - 2.0;
    while base - 2.0 >= 1.0 {
        for dy in 0..3 {
            for x in 1..5 {
                pen.dot(f64::from(x), base - f64::from(dy), Part::Core);
            }
        }
        base -= 4.0;
    }
    let scan = h - 1.0 - (motion.tick(8.0) % h as u64) as f64;
    pen.dot(0.0, scan, Part::Spark);
}
