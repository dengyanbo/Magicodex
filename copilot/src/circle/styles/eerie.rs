//! Eerie: a writhing ring stitched with sutures, a watching eye that blinks, and glitches that
//! tear rows sideways; the reply runs the wrong way round.

use std::f64::consts::FRAC_PI_2;
use std::f64::consts::PI;
use std::f64::consts::TAU;

use ratatui::style::Style;

use crate::circle::canvas::Canvas;
use crate::circle::canvas::Ink;
use crate::circle::canvas::Path;
use crate::circle::canvas::noise;
use crate::circle::canvas::polar;
use crate::circle::canvas::wave_radius;

use super::Frame;
use super::Inscription;
use super::inscribe_prompt;
use super::inscribe_reply;

/// An almond eye centred on `center`, looking toward `look` (each axis in `-1..=1`).
fn eye(
    canvas: &mut Canvas<'_>,
    center: (f64, f64),
    (width, height): (f64, f64),
    look: (f64, f64),
    open: bool,
) {
    let (cx, cy) = center;
    if !open {
        canvas.curve(
            /*samples*/ 24,
            /*progress*/ 1.0,
            |t| {
                (
                    cx - width + 2.0 * width * t,
                    cy + height * 0.25 * (PI * t).sin(),
                )
            },
            |_| Ink::Bright,
        );
        for lash in 0..5_u32 {
            let x = cx - width * 0.6 + f64::from(lash) * width * 0.3;
            canvas.line(
                (x, cy + height * 0.3),
                (x - 0.6, cy + height * 0.3 + 2.0),
                /*progress*/ 1.0,
                Ink::Line,
            );
        }
        return;
    }
    for lid in [-1.0, 1.0] {
        canvas.curve(
            /*samples*/ 26,
            /*progress*/ 1.0,
            |t| {
                (
                    cx - width + 2.0 * width * t,
                    cy + lid * height * (PI * t).sin(),
                )
            },
            |_| Ink::Bright,
        );
    }
    let pupil = (cx + look.0 * width * 0.6, cy + look.1 * height * 0.6);
    if height < 4.0 {
        // A small eye keeps a slit pupil so the white of the eye still shows.
        for dy in -1..=1 {
            canvas.plot(pupil.0, pupil.1 + f64::from(dy) * 0.9, Ink::Glint);
        }
        return;
    }
    canvas.arc(pupil, height * 0.85, (0.0, TAU), |_| Ink::Accent);
    for dx in -1..=1 {
        for dy in -2..=2 {
            canvas.plot(
                pupil.0 + f64::from(dx) * 0.9,
                pupil.1 + f64::from(dy) * 0.8,
                Ink::Glint,
            );
        }
    }
}

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let glitch = clock.tick(/*rate*/ 4.0);
    if !clock.settled && noise(&[glitch, 5]) > 0.72 {
        let from = noise(&[glitch, 6]) * canvas.dot_height();
        let rows = 4.0 + noise(&[glitch, 7]) * 6.0;
        let dx = ((noise(&[glitch, 8]) - 0.5) * 8.0).trunc();
        canvas.set_shift(Some((from, from + rows, dx)));
    }
    let writhe = [(1.8, 3.0, clock.spin * 0.7), (1.1, 5.0, -clock.spin * 1.3)];
    let rim = Path::Wave {
        base: radius - 2.0,
        waves: writhe,
        phase: 0.0,
    };
    let inner_writhe = [(1.3, 4.0, -clock.spin * 0.9), (0.9, 7.0, clock.spin)];
    let inner_rim = Path::Wave {
        base: radius - 12.0,
        waves: inner_writhe,
        phase: 0.0,
    };
    canvas.curve(
        /*samples*/ 150,
        clock.reveal(/*at*/ 0.0, /*duration*/ 0.9),
        |t| rim.point(t),
        |_| Ink::Line,
    );
    canvas.curve(
        /*samples*/ 130,
        clock.reveal(/*at*/ 0.2, /*duration*/ 0.9),
        |t| inner_rim.point(t),
        |_| Ink::Faint,
    );
    if layers >= 1 {
        for suture in 0..14_u64 {
            let theta = noise(&[suture, 11]) * TAU;
            let at = wave_radius(radius - 2.0, writhe, theta);
            canvas.line(
                polar(at - 1.8, theta),
                polar(at + 1.8, theta),
                /*progress*/ 1.0,
                Ink::Accent,
            );
        }
    }
    let blinking = !clock.settled && clock.spin.rem_euclid(3.2) < 0.25;
    let look = (
        (clock.spin * 0.9).sin() * 0.35,
        (clock.spin * 0.6 + 1.0).sin() * 0.2,
    );
    eye(
        canvas,
        (0.0, 0.0),
        (radius * 0.42, radius * 0.2),
        look,
        !blinking,
    );
    if layers >= 2 {
        // Watchers are sewn into the inner ring, between the prompt and the reply.
        for watcher in 0..3_u64 {
            let theta = noise(&[watcher, 21]) * TAU;
            let open = clock.settled || (clock.spin + watcher as f64 * 1.1).rem_euclid(2.7) >= 0.2;
            let at = wave_radius(radius - 12.0, inner_writhe, theta);
            eye(canvas, polar(at, theta), (5.0, 2.4), (0.0, 0.0), open);
        }
    }

    let accent = frame.palette.accent;
    let corrupt = |index: usize, style: Style| {
        if noise(&[index as u64, glitch, 9]) > 0.93 {
            accent
        } else {
            style
        }
    };
    let path = Path::Wave {
        base: radius - 7.0,
        waves: writhe,
        phase: -FRAC_PI_2 + 0.1 * clock.spin,
    };
    let inscription = Inscription {
        tone: Some(&corrupt),
        ..Inscription::default()
    };
    inscribe_prompt(canvas, frame, path, inscription);
    if layers >= 1 {
        // Inside the inner ring the newest text leads clockwise, so the reply reads backwards
        // wherever the prompt reads forwards.
        let path = Path::Wave {
            base: radius - 17.0,
            waves: inner_writhe,
            phase: FRAC_PI_2 + 0.14 * clock.spin,
        };
        inscribe_reply(
            canvas,
            frame,
            path,
            (0.0, 1.0),
            /*budget*/ 0.9,
            Some(&corrupt),
        );
    }
    canvas.set_shift(None);
}

/// A lone watching eye.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    eye(
        canvas,
        (0.0, 0.0),
        (radius * 1.1, radius * 0.62),
        (0.15, 0.0),
        /*open*/ true,
    );
}

/// Uneven drips from the rim.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    for (x, reach) in [(-3.5, 0.5), (0.0, 1.0), (2.5, 0.7)] {
        let bottom = radius + (end - radius - 2.0) * reach;
        canvas.line((x, radius), (x, bottom), /*progress*/ 1.0, Ink::Line);
        canvas.arc(
            (x, bottom + 1.0),
            /*radius*/ 1.2,
            (0.0, TAU),
            |_| Ink::Bright,
        );
    }
}
