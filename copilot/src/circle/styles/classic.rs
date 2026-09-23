//! Classic: hexagram circle with two text bands, a rim glint and layer-by-layer draw-in.

use std::f64::consts::FRAC_PI_2;
use std::f64::consts::TAU;

use crate::circle::canvas::Canvas;
use crate::circle::canvas::Ink;
use crate::circle::canvas::Path;
use crate::circle::style::Palette;

use super::BAND;
use super::Frame;
use super::Inscription;
use super::LAYER_TIMES;
use super::inscribe_prompt;
use super::inscribe_reply;

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let origin = (0.0, 0.0);
    let glint_at = clock.spin * 2.1;
    canvas.arc(
        origin,
        radius,
        (-FRAC_PI_2, TAU * clock.reveal(/*at*/ 0.0, /*duration*/ 0.6)),
        |theta| {
            // A comet-like highlight runs clockwise around the rim while the turn waits.
            let trail = (glint_at - theta).rem_euclid(TAU);
            if clock.settled || trail >= 0.8 {
                Ink::Line
            } else if trail < 0.35 {
                Ink::Glint
            } else {
                Ink::Bright
            }
        },
    );
    let beat = clock.settled || ((clock.age / 0.6) as u64).is_multiple_of(2);
    let core = if beat { Ink::Bright } else { Ink::Line };
    canvas.plot(-0.5, 0.0, core);
    canvas.plot(0.5, 0.0, core);

    let band = radius - BAND;
    if band >= 8.0 {
        canvas.arc(
            origin,
            band,
            (
                -FRAC_PI_2,
                TAU * clock.reveal(/*at*/ 0.15, /*duration*/ 0.6),
            ),
            |_| Ink::Line,
        );
    }
    let reply_ring = radius - 2.0 * BAND;
    let reply_band = layers >= 1 && reply_ring >= 6.0;
    if reply_band {
        canvas.arc(
            origin,
            reply_ring,
            (
                FRAC_PI_2,
                -TAU * clock.reveal(LAYER_TIMES[0], /*duration*/ 0.6),
            ),
            |_| Ink::Line,
        );
    }
    if reply_band && reply_ring >= 11.0 {
        let turn = -0.12 * clock.spin;
        let points: [(f64, f64); 6] = std::array::from_fn(|vertex| {
            let theta = turn - FRAC_PI_2 + vertex as f64 * TAU / 6.0;
            (reply_ring * theta.cos(), reply_ring * theta.sin())
        });
        let drawn = clock.reveal(LAYER_TIMES[0] + 0.3, /*duration*/ 1.2) * 6.0;
        for (index, (from, to)) in [(0, 2), (2, 4), (4, 0), (1, 3), (3, 5), (5, 1)]
            .into_iter()
            .enumerate()
        {
            let progress = (drawn - index as f64).clamp(0.0, 1.0);
            canvas.line(points[from], points[to], progress, Ink::Line);
        }
        let inner = reply_ring / 2.0;
        if layers >= 2 {
            let nodes = clock.reveal(LAYER_TIMES[1], /*duration*/ 0.6);
            for point in points.into_iter().take((nodes * 6.0).ceil() as usize) {
                canvas.arc(point, /*radius*/ 2.2, (0.0, TAU), |_| Ink::Bright);
            }
            canvas.arc(origin, inner, (-FRAC_PI_2, TAU * nodes), |_| Ink::Faint);
        }
        if layers >= 3 && inner >= 6.0 {
            let spokes = clock.reveal(LAYER_TIMES[2], /*duration*/ 0.8);
            for vertex in 0..6 {
                let theta = turn + f64::from(vertex) * TAU / 6.0;
                let (cos, sin) = (theta.cos(), theta.sin());
                canvas.line(
                    (3.5 * cos, 3.5 * sin),
                    (inner * cos, inner * sin),
                    spokes,
                    Ink::Faint,
                );
            }
            canvas.arc(
                origin,
                /*radius*/ 3.5,
                (0.0, TAU * spokes),
                |_| Ink::Line,
            );
        }
    }
    if layers >= 4 {
        canvas.arc(
            origin,
            radius - 2.0,
            (
                FRAC_PI_2,
                TAU * clock.reveal(LAYER_TIMES[3], /*duration*/ 0.8),
            ),
            |_| Ink::Faint,
        );
    }

    if band >= 8.0 {
        let path = Path::Circle {
            radius: radius - BAND / 2.0,
            phase: -FRAC_PI_2 + 0.15 * clock.spin,
        };
        inscribe_prompt(canvas, frame, path, Inscription::default());
    }
    if reply_band {
        let path = Path::Circle {
            radius: radius - BAND * 1.5,
            phase: FRAC_PI_2 + 0.25 * clock.spin,
        };
        // Newest text leads at the head of the orbit; older words trail counter-clockwise.
        inscribe_reply(
            canvas,
            frame,
            path,
            (0.0, -1.0),
            1.0 - 0.7 / TAU,
            /*tone*/ None,
        );
    }
}

/// A small double ring around a star.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64, palette: &Palette) {
    canvas.arc((0.0, 0.0), radius, (0.0, TAU), |_| Ink::Line);
    canvas.arc((0.0, 0.0), radius - 2.5, (0.0, TAU), |_| Ink::Faint);
    if let Some(cell) = canvas.cell_of(/*x*/ 0.0, /*y*/ 0.0) {
        canvas.write(cell, "✦", /*width*/ 1, palette.bright);
    }
}

/// A gate on the rim and a widening light cone that ends where the answer begins.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    canvas.arc(
        (0.0, radius),
        /*radius*/ 2.4,
        (0.0, TAU),
        |_| Ink::Bright,
    );
    let top = (radius + 2.4).ceil();
    let mut y = top;
    while y <= end {
        let depth = y - top;
        let ink = if depth < 3.0 { Ink::Glint } else { Ink::Bright };
        canvas.plot(-0.5, y, ink);
        canvas.plot(0.5, y, ink);
        let spread = depth * 0.55;
        if spread >= 1.5 {
            canvas.plot(-0.5 - spread, y, Ink::Faint);
            canvas.plot(0.5 + spread, y, Ink::Faint);
        }
        y += 1.0;
    }
}
