//! Fire: flickering flame tongues around the rim, a pentagram, rising embers and text that
//! shimmers like heat.

use std::f64::consts::FRAC_PI_2;
use std::f64::consts::PI;
use std::f64::consts::TAU;

use ratatui::style::Style;

use crate::circle::canvas::Canvas;
use crate::circle::canvas::Ink;
use crate::circle::canvas::Path;
use crate::circle::canvas::noise;
use crate::circle::canvas::polar;

use super::BAND;
use super::Frame;
use super::Inscription;
use super::LAYER_TIMES;
use super::corners;
use super::inscribe_prompt;
use super::inscribe_reply;

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let flicker = clock.tick(/*rate*/ 9.0);
    let base = radius - 6.5;
    let grow = clock.reveal(/*at*/ 0.0, /*duration*/ 0.7);
    let count = 26 + 6 * layers;
    for flame in 0..(count as f64 * grow) as u64 {
        let theta = flame as f64 * TAU / count as f64 + clock.spin * 0.12;
        let height = 2.5 + 4.0 * noise(&[flame, flicker]);
        let half = TAU / count as f64 * 0.42;
        let sway = (noise(&[flame, flicker, 7]) - 0.5) * 0.12;
        let tip = polar(base + height, theta + sway);
        canvas.line(
            polar(base, theta - half),
            tip,
            /*progress*/ 1.0,
            Ink::Bright,
        );
        canvas.line(
            polar(base, theta + half),
            tip,
            /*progress*/ 1.0,
            Ink::Bright,
        );
        canvas.plot(tip.0, tip.1, Ink::Accent);
    }
    canvas.arc((0.0, 0.0), base, (-FRAC_PI_2, TAU * grow), |_| Ink::Line);
    let band = base - BAND;
    if band >= 8.0 {
        canvas.arc(
            (0.0, 0.0),
            band,
            (FRAC_PI_2, TAU * clock.reveal(/*at*/ 0.2, /*duration*/ 0.6)),
            |_| Ink::Line,
        );
    }
    let star = band - BAND;
    if layers >= 1 && star >= 9.0 {
        let points = corners(/*sides*/ 5, star, clock.spin * 0.2 - FRAC_PI_2);
        let pentagram = [points[0], points[2], points[4], points[1], points[3]];
        let drawn = clock.reveal(LAYER_TIMES[0], /*duration*/ 1.0);
        canvas.polygon(&pentagram, drawn, Ink::Line);
        canvas.arc(
            (0.0, 0.0),
            star,
            (0.0, TAU * clock.reveal(LAYER_TIMES[0], /*duration*/ 0.6)),
            |_| Ink::Faint,
        );
    }
    if layers >= 2 {
        let rise = radius * 1.6;
        for ember in 0..(10 + 6 * layers) as u64 {
            let x = (noise(&[ember, 1]) * 2.0 - 1.0) * radius * 0.75;
            let speed = 5.0 + 9.0 * noise(&[ember, 2]);
            let y =
                radius * 0.8 - (clock.spin * speed + noise(&[ember, 3]) * rise).rem_euclid(rise);
            if x * x + y * y < (base - 1.0).powi(2) {
                let ink = if noise(&[ember, flicker]) > 0.4 {
                    Ink::Accent
                } else {
                    Ink::Bright
                };
                canvas.plot(x + 1.5 * (clock.spin * 3.0 + ember as f64).sin(), y, ink);
            }
        }
    }
    if layers >= 3 {
        for ray in 0..8_u64 {
            let theta = ray as f64 * TAU / 8.0 - clock.spin * 0.3;
            let length = 5.0 + 2.0 * noise(&[ray, flicker]);
            canvas.line(
                polar(2.5, theta),
                polar(length, theta),
                /*progress*/ 1.0,
                Ink::Accent,
            );
        }
    }
    canvas.arc((0.0, 0.0), /*radius*/ 2.2, (0.0, TAU), |_| Ink::Bright);

    let shimmer = |index: usize, style: Style| {
        if noise(&[index as u64, flicker, 3]) > 0.72 {
            style.bold()
        } else {
            style
        }
    };
    if band >= 8.0 {
        let path = Path::Circle {
            radius: base - BAND / 2.0,
            phase: -FRAC_PI_2 + 0.1 * clock.spin,
        };
        let inscription = Inscription {
            tone: Some(&shimmer),
            ..Inscription::default()
        };
        inscribe_prompt(canvas, frame, path, inscription);
    }
    if layers >= 1 && star >= 6.0 {
        let path = Path::Circle {
            radius: band - BAND / 2.0,
            phase: FRAC_PI_2 - 0.2 * clock.spin,
        };
        inscribe_reply(
            canvas,
            frame,
            path,
            (0.0, -1.0),
            /*budget*/ 0.9,
            /*tone*/ None,
        );
    }
}

/// A single flame with a hot core.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    for side in [-1.0, 1.0] {
        canvas.curve(
            /*samples*/ 30,
            /*progress*/ 1.0,
            |t| {
                let x = side * radius * 0.55 * (PI * t).sin() * (1.0 - 0.3 * t);
                (x, radius - 2.0 * radius * t)
            },
            |_| Ink::Line,
        );
        canvas.curve(
            /*samples*/ 20,
            /*progress*/ 1.0,
            |t| {
                (
                    side * radius * 0.25 * (PI * t).sin(),
                    radius - 1.2 * radius * t,
                )
            },
            |_| Ink::Accent,
        );
    }
}

/// A jet of flame and falling sparks.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    let span = end - radius;
    for side in [-1.0, 1.0] {
        canvas.curve(
            /*samples*/ 20,
            /*progress*/ 1.0,
            |t| (side * 5.0 * (1.0 - t), radius + t * span),
            |_| Ink::Bright,
        );
    }
    canvas.curve(
        /*samples*/ 20,
        /*progress*/ 1.0,
        |t| (0.3 * (t * 12.0).sin(), radius + t * span * 0.8),
        |_| Ink::Accent,
    );
    for spark in 0..6_u64 {
        canvas.plot(
            (noise(&[spark, 1]) - 0.5) * 8.0,
            radius + noise(&[spark, 2]) * span,
            Ink::Accent,
        );
    }
}
