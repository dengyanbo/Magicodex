//! Water: an undulating rim, ripples spreading across a central pond, bobbing droplets, and
//! text that rides the waves.

use std::f64::consts::FRAC_PI_2;
use std::f64::consts::PI;
use std::f64::consts::TAU;

use crate::circle::canvas::Canvas;
use crate::circle::canvas::Ink;
use crate::circle::canvas::Path;
use crate::circle::canvas::polar;

use super::Frame;
use super::Inscription;
use super::inscribe_prompt;
use super::inscribe_reply;

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let rim = Path::Wave {
        base: radius - 2.5,
        waves: [(1.3, 12.0, -clock.spin * 2.2), (0.9, 5.0, clock.spin * 1.3)],
        phase: 0.0,
    };
    let grow = clock.reveal(/*at*/ 0.0, /*duration*/ 0.8);
    canvas.curve(/*samples*/ 220, grow, |t| rim.point(t), |_| Ink::Line);
    let inner_rim = Path::Wave {
        base: radius - 11.0,
        waves: [(1.0, 9.0, clock.spin * 1.7), (0.6, 4.0, -clock.spin)],
        phase: 0.0,
    };
    canvas.curve(
        /*samples*/ 160,
        clock.reveal(/*at*/ 0.2, /*duration*/ 0.8),
        |t| inner_rim.point(t),
        |_| Ink::Faint,
    );
    let pond = radius - 20.0;
    if pond >= 6.0 {
        let ripples = (1 + layers).min(3);
        for ripple in 0..ripples {
            let phase = (clock.spin / 2.6 + ripple as f64 / ripples as f64).rem_euclid(1.0);
            let ink = if phase < 0.25 {
                Ink::Bright
            } else if phase < 0.6 {
                Ink::Line
            } else {
                Ink::Faint
            };
            canvas.arc((0.0, 0.0), 1.5 + phase * pond, (0.0, TAU), |_| ink);
        }
    }
    if layers >= 2 {
        // Droplets bob on the inner current, between the two lines of text.
        for drop in 0..6_u32 {
            let theta = f64::from(drop) * TAU / 6.0 + clock.spin * 0.15;
            let orbit = radius - 11.5 + 0.8 * (clock.spin * 2.0 + f64::from(drop)).sin();
            let (x, y) = polar(orbit, theta);
            canvas.arc(
                (x, y + 0.5),
                /*radius*/ 1.4,
                (0.0, TAU),
                |_| Ink::Accent,
            );
            canvas.line(
                (x, y - 2.5),
                (x, y - 0.8),
                /*progress*/ 1.0,
                Ink::Accent,
            );
        }
    }
    let prompt = Path::Wave {
        base: radius - 7.0,
        waves: [(1.5, 5.0, 1.0 - clock.spin * 2.2), (0.0, 0.0, 0.0)],
        phase: -FRAC_PI_2 + 0.12 * clock.spin,
    };
    inscribe_prompt(canvas, frame, prompt, Inscription::default());
    if layers >= 1 {
        let reply = Path::Wave {
            base: radius - 15.5,
            waves: [(1.2, 4.0, clock.spin * 1.8), (0.0, 0.0, 0.0)],
            phase: FRAC_PI_2 - 0.2 * clock.spin,
        };
        inscribe_reply(
            canvas,
            frame,
            reply,
            (0.0, -1.0),
            /*budget*/ 0.9,
            /*tone*/ None,
        );
    }
}

/// A droplet above a ripple.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    for side in [-1.0, 1.0] {
        canvas.curve(
            /*samples*/ 30,
            /*progress*/ 1.0,
            |t| {
                let swell = (0.5 - 0.5 * (PI * t).cos()).powf(0.7);
                (
                    side * radius * 0.45 * (PI * t).sin(),
                    -radius + 1.6 * radius * swell,
                )
            },
            |_| Ink::Line,
        );
    }
    canvas.curve(
        /*samples*/ 40,
        /*progress*/ 1.0,
        |t| {
            (
                -radius + 2.0 * radius * t,
                radius - 1.0 + 0.8 * (t * TAU * 2.0).sin(),
            )
        },
        |_| Ink::Accent,
    );
}

/// Streams of droplets falling into a small wave.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    for (x, reach) in [(-0.5, 1.0), (0.5, 1.0), (-4.0, 0.6), (4.0, 0.7)] {
        let ink = if f64::abs(x) < 1.0 {
            Ink::Line
        } else {
            Ink::Faint
        };
        let mut y = radius + 1.0;
        while y <= radius + (end - radius) * reach {
            canvas.plot(x, y, ink);
            y += 2.0;
        }
    }
    canvas.curve(
        /*samples*/ 30,
        /*progress*/ 1.0,
        |t| (-6.0 + 12.0 * t, end - 0.5 + 0.8 * (t * TAU * 1.5).sin()),
        |_| Ink::Accent,
    );
}
