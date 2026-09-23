//! Earth: a square stone seal with text cut into its four sides, the ☷ trigram, corner stones
//! and a diamond that grinds round in steps.

use std::f64::consts::FRAC_PI_4;
use std::f64::consts::PI;

use crate::magic_canvas::Canvas;
use crate::magic_canvas::Ink;
use crate::magic_canvas::Path;

use super::BAND;
use super::Frame;
use super::Inscription;
use super::LAYER_TIMES;
use super::corners;
use super::inscribe_prompt;
use super::inscribe_reply;

fn square(half: f64) -> [(f64, f64); 4] {
    [(-half, -half), (half, -half), (half, half), (-half, half)]
}

/// The three broken bars of the earth trigram, centred on the origin.
fn trigram(canvas: &mut Canvas<'_>, spacing: f64, reach: f64) {
    for row in [-spacing, 0.0, spacing] {
        canvas.line(
            (-reach, row),
            (-1.5, row),
            /*progress*/ 1.0,
            Ink::Bright,
        );
        canvas.line((1.5, row), (reach, row), /*progress*/ 1.0, Ink::Bright);
    }
}

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let half = radius - 1.0;
    let grow = clock.reveal(/*at*/ 0.0, /*duration*/ 0.8);
    canvas.polygon(&square(half), grow, Ink::Line);
    canvas.polygon(&square(half - 1.5), grow, Ink::Line);
    let inner = half - BAND;
    if inner >= 6.0 {
        let drawn = clock.reveal(/*at*/ 0.2, /*duration*/ 0.8);
        canvas.polygon(&square(inner), drawn, Ink::Bright);
    }
    let core = inner - BAND;
    if layers >= 1 && core >= 5.0 {
        // Stone does not glide: the diamond turns a twelfth of a half-turn at a time.
        let step = (clock.spin / 1.4).floor();
        let diamond = corners(/*sides*/ 4, core * 1.3, FRAC_PI_4 + step * PI / 12.0);
        let drawn = clock.reveal(LAYER_TIMES[0], /*duration*/ 0.8);
        canvas.polygon(&diamond, drawn, Ink::Faint);
    }
    if layers >= 2 {
        for (x, y) in square(half) {
            let (cx, cy) = (x - x.signum() * 2.0, y - y.signum() * 2.0);
            for dx in -2..=2 {
                for dy in -2..=2 {
                    canvas.plot(cx + f64::from(dx), cy + f64::from(dy), Ink::Accent);
                }
            }
        }
    }
    if layers >= 3 {
        for mark in 1..8_u32 {
            let t = -half + f64::from(mark) * half / 4.0;
            for (x, y) in [
                (t, -half - 2.0),
                (t, half + 2.0),
                (-half - 2.0, t),
                (half + 2.0, t),
            ] {
                canvas.plot(x, y, Ink::Faint);
            }
        }
    }
    trigram(canvas, /*spacing*/ 4.0, /*reach*/ 5.0);

    let path = Path::Square {
        half: half - BAND / 2.0,
        offset: 0.875 + clock.spin * 0.004,
    };
    inscribe_prompt(canvas, frame, path, Inscription::default());
    if layers >= 1 && core >= 5.0 {
        let path = Path::Square {
            half: inner - BAND / 2.0,
            offset: 0.375 - clock.spin * 0.006,
        };
        inscribe_reply(
            canvas,
            frame,
            path,
            (0.0, -1.0),
            /*budget*/ 0.92,
            /*tone*/ None,
        );
    }
}

/// A small square seal holding the trigram.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    let half = radius - 1.0;
    canvas.polygon(
        &[
            (-half, -half * 0.9),
            (half, -half * 0.9),
            (half, half * 0.9),
            (-half, half * 0.9),
        ],
        /*progress*/ 1.0,
        Ink::Line,
    );
    trigram(canvas, /*spacing*/ 3.0, /*reach*/ 4.0);
}

/// A stone pillar with a single crack.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    let top = radius - 1.0;
    let mut y = top + 1.0;
    while y <= end {
        for x in [-2.5, -1.5, -0.5, 0.5, 1.5, 2.5] {
            let crack = (y - (top + 5.0)).abs() < 0.6 && x > 0.0;
            if !crack {
                canvas.plot(
                    x,
                    y,
                    if f64::abs(x) < 2.0 {
                        Ink::Line
                    } else {
                        Ink::Faint
                    },
                );
            }
        }
        y += 1.0;
    }
}
