//! Wind: spiral arms of a fast vortex with dashed gusts; text is drawn into the eye.

use std::f64::consts::FRAC_PI_2;
use std::f64::consts::PI;
use std::f64::consts::TAU;

use crate::magic_canvas::Canvas;
use crate::magic_canvas::Ink;
use crate::magic_canvas::Path;
use crate::magic_canvas::noise;
use crate::magic_canvas::polar;

use super::Frame;
use super::Inscription;
use super::inscribe_prompt;
use super::inscribe_reply;

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, clock) = (frame.radius, frame.clock);
    let turn = clock.spin * 0.9;
    let grow = clock.reveal(/*at*/ 0.0, /*duration*/ 0.9);
    let arms = 3 + frame.layers.min(3);
    // The vortex turns clockwise, so its arms wind clockwise on their way into the eye.
    for arm in 0..arms {
        let base = arm as f64 * TAU / arms as f64 + turn;
        canvas.curve(
            /*samples*/ 70,
            grow,
            |t| polar(radius * (0.16 + 0.81 * t), base - t * PI * 1.2),
            |t| {
                if t < 0.3 {
                    Ink::Faint
                } else if t < 0.82 {
                    Ink::Line
                } else {
                    Ink::Bright
                }
            },
        );
    }
    for dash in 0..18_u32 {
        let start = f64::from(dash) * TAU / 18.0 - clock.spin * 0.45;
        let ink = if dash.is_multiple_of(3) {
            Ink::Line
        } else {
            Ink::Faint
        };
        canvas.arc((0.0, 0.0), radius, (start, TAU / 18.0 * 0.5 * grow), |_| {
            ink
        });
    }
    for gust in 0..(8 + 5 * frame.layers) as u64 {
        let orbit = radius * (0.25 + 0.7 * noise(&[gust, 1]));
        let speed = 0.9 + 1.8 * noise(&[gust, 2]);
        let theta = noise(&[gust, 3]) * TAU + clock.spin * speed;
        for trail in 0..4_u32 {
            let (x, y) = polar(orbit, theta - f64::from(trail) * 0.045);
            canvas.plot(x, y, if trail == 0 { Ink::Glint } else { Ink::Faint });
        }
    }
    if frame.layers >= 2 {
        canvas.arc((0.0, 0.0), radius * 0.16, (0.0, TAU), |_| Ink::Bright);
    }
    // The prompt is drawn in from the rim along the arms; the two spirals never share a radius.
    let prompt = Path::Spiral {
        outer: radius * 0.93,
        inner: radius * 0.62,
        phase: -FRAC_PI_2 + turn * 0.6,
        turns: 0.85,
    };
    inscribe_prompt(canvas, frame, prompt, Inscription::default());
    if frame.layers >= 1 {
        let reply = Path::Spiral {
            outer: radius * 0.52,
            inner: radius * 0.22,
            phase: turn * 0.6,
            turns: 0.75,
        };
        // The newest words sit at the eye, above it, and older ones unwind outward behind them.
        inscribe_reply(
            canvas,
            frame,
            reply,
            (1.0, -1.0),
            /*budget*/ 1.0,
            /*tone*/ None,
        );
    }
}

/// Three curling gusts.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    for arm in 0..3_u32 {
        let base = f64::from(arm) * TAU / 3.0;
        canvas.curve(
            /*samples*/ 30,
            /*progress*/ 1.0,
            |t| polar(2.0 + (radius - 2.0) * t, base + t * 2.2),
            |t| if t > 0.35 { Ink::Line } else { Ink::Faint },
        );
    }
}

/// A small twister narrowing toward the answer.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    let span = end - radius;
    for (phase, ink) in [(0.0, Ink::Line), (PI, Ink::Faint)] {
        canvas.curve(
            /*samples*/ 50,
            /*progress*/ 1.0,
            |t| ((7.0 - 5.5 * t) * (t * 9.0 + phase).sin(), radius + t * span),
            |_| ink,
        );
    }
}
