//! Holy: a breathing sunburst of rays behind a halo, an octagram, a cross of light, and spaced
//! golden text.

use std::f64::consts::FRAC_PI_2;
use std::f64::consts::FRAC_PI_4;
use std::f64::consts::TAU;

use crate::magic_canvas::Canvas;
use crate::magic_canvas::Ink;
use crate::magic_canvas::Path;
use crate::magic_canvas::polar;

use super::BAND;
use super::Frame;
use super::Inscription;
use super::LAYER_TIMES;
use super::corners;
use super::inscribe_prompt;
use super::inscribe_reply;

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let breath = 0.5 + 0.5 * (clock.spin * 1.8).sin();
    let halo = radius - BAND;
    canvas.arc(
        (0.0, 0.0),
        halo,
        (-FRAC_PI_2, TAU * clock.reveal(/*at*/ 0.1, /*duration*/ 0.6)),
        |_| Ink::Bright,
    );
    canvas.arc(
        (0.0, 0.0),
        halo - 1.6,
        (-FRAC_PI_2, TAU * clock.reveal(/*at*/ 0.2, /*duration*/ 0.6)),
        |_| Ink::Faint,
    );
    canvas.arc((0.0, 0.0), radius, (0.0, TAU), |_| Ink::Faint);
    let rays = if layers >= 2 { 48 } else { 24 };
    let shown = (rays as f64 * clock.reveal(/*at*/ 0.0, /*duration*/ 1.0)) as usize;
    for ray in 0..shown {
        let theta = ray as f64 * TAU / rays as f64 + clock.spin * 0.05;
        let long = ray.is_multiple_of(2);
        let (reach, ink) = if long {
            let ink = if breath > 0.55 {
                Ink::Accent
            } else {
                Ink::Line
            };
            (radius + 1.5 + 1.2 * breath, ink)
        } else {
            (radius - 2.5, Ink::Line)
        };
        canvas.line(
            polar(halo + 1.5, theta),
            polar(reach, theta),
            /*progress*/ 1.0,
            ink,
        );
    }
    let inner = halo - BAND;
    if layers >= 1 && inner >= 8.0 {
        canvas.arc(
            (0.0, 0.0),
            inner,
            (0.0, TAU * clock.reveal(LAYER_TIMES[0], /*duration*/ 0.6)),
            |_| Ink::Line,
        );
        let drawn = clock.reveal(LAYER_TIMES[0] + 0.3, /*duration*/ 1.0);
        let turn = clock.spin * 0.08;
        canvas.polygon(&corners(/*sides*/ 4, inner * 0.92, turn), drawn, Ink::Line);
        canvas.polygon(
            &corners(/*sides*/ 4, inner * 0.92, turn + FRAC_PI_4),
            drawn,
            Ink::Faint,
        );
    }
    let cross = if breath > 0.5 {
        Ink::Glint
    } else {
        Ink::Bright
    };
    for ray in 0..8_u32 {
        let theta = f64::from(ray) * TAU / 8.0;
        let length = if ray.is_multiple_of(2) { 6.5 } else { 3.5 };
        canvas.line(
            polar(1.2, theta),
            polar(length, theta),
            /*progress*/ 1.0,
            cross,
        );
    }

    let path = Path::Circle {
        radius: radius - BAND / 2.0 - 0.5,
        phase: -FRAC_PI_2 + 0.05 * clock.spin,
    };
    let inscription = Inscription {
        spaced: true,
        ..Inscription::default()
    };
    inscribe_prompt(canvas, frame, path, inscription);
    if layers >= 1 && inner >= 8.0 {
        let path = Path::Circle {
            radius: halo - BAND / 2.0 - 0.8,
            phase: FRAC_PI_2 - 0.08 * clock.spin,
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

/// An eight-pointed star of light.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    for ray in 0..8_u32 {
        let theta = f64::from(ray) * TAU / 8.0 - FRAC_PI_2;
        let (length, ink) = if ray.is_multiple_of(2) {
            (radius, Ink::Accent)
        } else {
            (radius * 0.55, Ink::Line)
        };
        canvas.line(
            polar(1.0, theta),
            polar(length, theta),
            /*progress*/ 1.0,
            ink,
        );
    }
    canvas.arc((0.0, 0.0), radius * 0.35, (0.0, TAU), |_| Ink::Bright);
}

/// A column of light that widens as it descends.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    let mut y = radius + 1.0;
    while y <= end {
        let depth = (y - radius) / (end - radius).max(1.0);
        let half = 2.0 + depth * 3.0;
        canvas.plot(-0.5, y, Ink::Glint);
        canvas.plot(0.5, y, Ink::Glint);
        canvas.plot(-half, y, Ink::Accent);
        canvas.plot(half, y, Ink::Accent);
        if (y as i64).rem_euclid(2) == 0 {
            canvas.plot(-half / 2.0, y, Ink::Line);
            canvas.plot(half / 2.0, y, Ink::Line);
        }
        y += 1.0;
    }
}
