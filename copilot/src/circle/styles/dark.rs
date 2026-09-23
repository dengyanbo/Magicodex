//! Dark: an abyssal vortex of curved arms around an event horizon with a blood-red crescent;
//! the prompt fades as it orbits and the reply spirals down into the void.

use std::f64::consts::FRAC_PI_2;
use std::f64::consts::PI;
use std::f64::consts::TAU;

use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::circle::canvas::Canvas;
use crate::circle::canvas::Ink;
use crate::circle::canvas::Path;
use crate::circle::canvas::noise;
use crate::circle::canvas::polar;

use super::Frame;
use super::Inscription;
use super::inscribe_prompt;
use super::inscribe_reply;

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let grow = clock.reveal(/*at*/ 0.0, /*duration*/ 1.0);
    let arms = 10 + 2 * layers;
    // The abyss turns counter-clockwise, so everything it swallows spirals in that way.
    for arm in 0..arms {
        let start = arm as f64 * TAU / arms as f64 - clock.spin * 0.28;
        let lit = arm.is_multiple_of(3);
        canvas.curve(
            /*samples*/ 50,
            grow,
            |t| polar(radius * (0.98 - 0.62 * t), start - t * 1.7),
            |t| {
                if lit && t < 0.5 {
                    Ink::Line
                } else {
                    Ink::Faint
                }
            },
        );
    }
    let horizon = radius * 0.34;
    canvas.arc((0.0, 0.0), horizon, (0.0, TAU), |_| Ink::Bright);
    let crescent = clock.spin * 0.4;
    for dot in 0..24_u32 {
        let theta = crescent + (f64::from(dot) / 23.0 - 0.5) * 2.2;
        let (x, y) = polar(horizon + 1.4, theta);
        canvas.plot(x, y, Ink::Accent);
        let (x, y) = polar(horizon + 2.4, theta);
        let ink = if (f64::from(dot) - 11.5).abs() < 6.0 {
            Ink::Accent
        } else {
            Ink::Faint
        };
        canvas.plot(x, y, ink);
    }
    if layers >= 2 {
        for mote in 0..(10 + 4 * layers) as u64 {
            let fall =
                (clock.spin * (0.12 + 0.1 * noise(&[mote, 1])) + noise(&[mote, 2])).rem_euclid(1.0);
            let orbit = radius * (1.0 - fall) + horizon * fall;
            let theta = noise(&[mote, 3]) * TAU - clock.spin * 0.3 - fall * 2.0;
            let (x, y) = polar(orbit, theta);
            canvas.plot(x, y, if fall < 0.7 { Ink::Faint } else { Ink::Accent });
        }
    }
    if layers >= 3 {
        for tendril in 0..5_u32 {
            let theta = f64::from(tendril) * TAU / 5.0 + clock.spin * 0.1;
            let sway = clock.spin * 2.0 + f64::from(tendril);
            canvas.curve(
                /*samples*/ 20,
                /*progress*/ 1.0,
                |t| {
                    polar(
                        radius - 1.0 - t * 6.0,
                        theta + 0.25 * (t * 6.0 + sway).sin(),
                    )
                },
                |_| Ink::Line,
            );
        }
    }

    let total = frame
        .prompt
        .graphemes(/*is_extended*/ true)
        .filter(|glyph| glyph.width() > 0)
        .count()
        .max(1);
    // Each repeat of the prompt dims toward its end, as if swallowed by the dark.
    let fade = |index: usize, style: Style| {
        if (index % total) * 10 > total * 6 {
            style.dim()
        } else {
            style
        }
    };
    let path = Path::Circle {
        radius: radius - 4.5,
        phase: -FRAC_PI_2 - 0.12 * clock.spin,
    };
    let inscription = Inscription {
        tone: Some(&fade),
        ..Inscription::default()
    };
    inscribe_prompt(canvas, frame, path, inscription);
    if layers >= 1 {
        // New words enter at the outer end and older ones sink toward the horizon.
        let path = Path::Spiral {
            outer: radius - 12.0,
            inner: horizon + 4.0,
            phase: -0.3 - clock.spin * 0.28,
            turns: -0.8,
        };
        inscribe_reply(
            canvas,
            frame,
            path,
            (0.0, 1.0),
            /*budget*/ 1.0,
            /*tone*/ None,
        );
    }
}

/// A blood-red crescent over the dotted outline of an eclipsed disc.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    for dot in 0..16_u32 {
        let (x, y) = polar(radius, f64::from(dot) * TAU / 16.0);
        canvas.plot(x, y, Ink::Faint);
    }
    // The lit edge sits on the left; a shifted inner arc thins it toward both horns.
    canvas.arc((0.0, 0.0), radius, (FRAC_PI_2 - 0.35, PI + 0.7), |_| {
        Ink::Accent
    });
    canvas.arc(
        (2.2, 0.0),
        radius - 1.2,
        (FRAC_PI_2 + 0.1, PI - 0.2),
        |_| Ink::Accent,
    );
}

/// Two shadow tendrils and a falling drop.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    let span = end - radius;
    for side in [-1.0, 1.0] {
        canvas.curve(
            /*samples*/ 30,
            /*progress*/ 1.0,
            |t| {
                (
                    side * (4.0 - 3.0 * t + 1.5 * (t * 7.0).sin()),
                    radius + t * span,
                )
            },
            |_| Ink::Line,
        );
    }
    canvas.arc(
        (0.0, end - 1.5),
        /*radius*/ 1.5,
        (0.0, TAU),
        |_| Ink::Accent,
    );
}
