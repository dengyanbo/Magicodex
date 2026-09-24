//! Rune particles drifting through the free cells beside the circle.

use std::f64::consts::TAU;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::Motion;
use crate::magic_canvas::Canvas;
use crate::magic_canvas::Ink;
use crate::magic_canvas::noise;
use crate::magic_style::MagicStyle;
use crate::magic_style::Palette;

/// One side's particles, in dots of that side.
struct Flow {
    style: MagicStyle,
    seed: u64,
    width: f64,
    height: f64,
    /// Whether the circle is to the right (the left side) rather than to the left.
    inward: bool,
    motion: Motion,
}

impl Flow {
    /// Where particle `k` is now; it may emit a few dots, each with its ink.
    fn particle(&self, k: u64, plot: &mut impl FnMut(f64, f64, Ink)) {
        let r = |part: u64| noise(&[self.seed, k, part]);
        let t = self.motion.spin;
        let charge = 0.5 + 0.125 * self.motion.layers.min(4) as f64;
        let (outer, inner) = if self.inward {
            (0.0, self.width - 1.0)
        } else {
            (self.width - 1.0, 0.0)
        };
        let across = |f: f64| outer + (inner - outer) * f;
        let drift = |speed: f64| (r(1) + t * speed * (0.6 + 0.8 * r(2)) * charge).fract();
        let back = if self.inward { -1.0 } else { 1.0 };
        let (width, height) = (self.width, self.height);
        match self.style {
            MagicStyle::Classic => {
                let wobble = (t * 2.0 + r(5) * TAU).sin() * 1.5;
                let ink = if r(9) < 0.15 { Ink::Glint } else { Ink::Faint };
                plot(across(drift(0.18)), r(3) * height + wobble, ink);
            }
            MagicStyle::Wind => {
                let x = across(drift(0.8));
                let y = r(3) * height + (t * 1.5 + k as f64).sin() * 2.0;
                plot(x, y, Ink::Line);
                plot(x + back, y, Ink::Faint);
                plot(x + 2.0 * back, y, Ink::Faint);
            }
            MagicStyle::Fire => {
                let f = drift(0.25);
                let ink = if r(4) < 0.35 { Ink::Accent } else { Ink::Line };
                plot(across(r(3) * 0.7 + f * 0.3), height - 1.0 - f * height, ink);
            }
            MagicStyle::Water => {
                let ink = if r(4) < 0.3 { Ink::Accent } else { Ink::Faint };
                plot(across(r(3)), drift(0.3) * height, ink);
            }
            MagicStyle::Thunder => {
                let tick = self.motion.tick(6.0);
                if noise(&[self.seed, k, tick, 5]) < 0.45 {
                    let x = noise(&[self.seed, k, tick, 6]) * width;
                    let y = noise(&[self.seed, k, tick, 7]) * height;
                    plot(x, y, Ink::Accent);
                    plot(x + 1.0, y + 1.0, Ink::Line);
                }
            }
            MagicStyle::Earth => {
                let f = drift(0.08);
                plot(
                    across(r(3) * 0.8 + f * 0.2),
                    (r(4) + f * 0.5).fract() * height,
                    Ink::Faint,
                );
            }
            MagicStyle::Holy => {
                let f = drift(0.12);
                let ink = if r(9) < 0.2 { Ink::Glint } else { Ink::Faint };
                plot(across(r(3) * 0.6 + f * 0.4), height - 1.0 - f * height, ink);
            }
            MagicStyle::Dark => {
                // Pulled in ever faster, toward the middle of the height.
                let pull = drift(0.2).powi(2);
                let start = r(3) * height;
                let ink = if r(4) < 0.25 { Ink::Accent } else { Ink::Faint };
                plot(
                    across(pull),
                    start + (height / 2.0 - start) * pull * 0.5,
                    ink,
                );
            }
            MagicStyle::Eerie => {
                if noise(&[self.seed, k, self.motion.tick(1.5)]) < 0.4 {
                    let ink = if r(5) < 0.3 { Ink::Accent } else { Ink::Faint };
                    plot(r(3) * width, r(4) * height, ink);
                }
            }
            MagicStyle::Tech => {
                // Data runs along cell rows.
                let x = across(drift(0.35));
                let y = (r(3) * height / 4.0).floor() * 4.0 + 1.0;
                plot(x, y, Ink::Line);
                plot(x + 2.0 * back, y, Ink::Faint);
            }
        }
    }
}

fn seed(style: MagicStyle, inward: bool) -> u64 {
    let index = MagicStyle::ALL
        .iter()
        .position(|each| *each == style)
        .unwrap_or(0);
    (index as u64 + 1) * 0x9E37_79B9 + u64::from(inward)
}

/// Draws particles over `zone`, drifting toward the circle, outside `keep_out`. At the outlet
/// they are drawn into the bottom corner next to it, where the answer pours out.
pub(super) fn draw(
    buf: &mut Buffer,
    zone: Rect,
    inward: bool,
    keep_out: &[Rect],
    style: MagicStyle,
    motion: Motion,
    palette: &Palette,
) {
    if zone.width == 0 || zone.height == 0 {
        return;
    }
    let mut canvas = Canvas::new(zone.width, zone.height, 0.0);
    for rect in keep_out {
        let overlap = rect.intersection(zone);
        for row in overlap.y..overlap.bottom() {
            let cell = (usize::from(overlap.x - zone.x), usize::from(row - zone.y));
            canvas.reserve(cell, usize::from(overlap.width));
        }
    }
    let flow = Flow {
        style,
        seed: seed(style, inward),
        width: f64::from(zone.width) * 2.0,
        height: f64::from(zone.height) * 4.0,
        inward,
        motion,
    };
    let density = 0.025 + 0.007 * motion.layers.min(4) as f64;
    let count = (f64::from(zone.width) * f64::from(zone.height) * density).round() as u64;
    let converge = if motion.animated {
        motion.outlet.unwrap_or(0.0)
    } else {
        0.0
    };
    let pull = (converge / 0.6).min(1.0).powi(2);
    if pull >= 1.0 {
        return;
    }
    let inner = if inward { flow.width - 1.0 } else { 0.0 };
    let origin = f64::from(zone.width) - 0.5;
    let mut plot = |x: f64, y: f64, ink: Ink| {
        let x = x + (inner - x) * pull;
        let y = y + (flow.height - 1.0 - y) * pull;
        if (0.0..flow.width).contains(&x) && (0.0..flow.height).contains(&y) {
            canvas.plot(x - origin, y, ink);
        }
    };
    for k in 0..count {
        flow.particle(k, &mut plot);
    }
    canvas.paint(zone, buf, palette);
}
