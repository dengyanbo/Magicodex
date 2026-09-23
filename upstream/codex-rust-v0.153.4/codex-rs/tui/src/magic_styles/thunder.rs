//! Thunder: a crackling octagon, lightning that strikes toward the core, and text laid along
//! straight edges that flares when a bolt lands.

use std::f64::consts::FRAC_PI_8;
use std::f64::consts::TAU;

use ratatui::style::Style;

use crate::magic_canvas::Canvas;
use crate::magic_canvas::Ink;
use crate::magic_canvas::Path;
use crate::magic_canvas::noise;
use crate::magic_canvas::polar;

use super::BAND;
use super::Frame;
use super::Inscription;
use super::LAYER_TIMES;
use super::corners;
use super::inscribe_prompt;
use super::inscribe_reply;

/// A zigzag from `from` to `to`, displaced sideways by up to `jitter` dots.
fn bolt(
    canvas: &mut Canvas<'_>,
    (from, to): ((f64, f64), (f64, f64)),
    seed: u64,
    jitter: f64,
    ink: Ink,
) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy).max(1.0);
    let normal = (-dy / length, dx / length);
    let segments = 7_u32;
    let mut points = vec![from];
    for joint in 1..segments {
        let t = f64::from(joint) / f64::from(segments);
        let offset = (noise(&[seed, u64::from(joint)]) * 2.0 - 1.0) * jitter;
        points.push((
            from.0 + dx * t + normal.0 * offset,
            from.1 + dy * t + normal.1 * offset,
        ));
    }
    points.push(to);
    canvas.polyline(&points, /*progress*/ 1.0, ink);
}

/// A lightning strike: a sharp zigzag from `from` to `to` with one fork split off halfway.
fn lightning(canvas: &mut Canvas<'_>, (from, to): ((f64, f64), (f64, f64)), seed: u64, ink: Ink) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy).max(1.0);
    let (along, normal) = ((dx / length, dy / length), (-dy / length, dx / length));
    let segments = ((length / 4.5).ceil() as u32).max(4);
    let mut points = vec![from];
    for joint in 1..segments {
        let t = f64::from(joint) / f64::from(segments);
        let side = if joint.is_multiple_of(2) { 1.0 } else { -1.0 };
        let offset = side * (1.2 + 2.3 * noise(&[seed, u64::from(joint)]));
        points.push((
            from.0 + dx * t + normal.0 * offset,
            from.1 + dy * t + normal.1 * offset,
        ));
    }
    points.push(to);
    canvas.polyline(&points, /*progress*/ 1.0, ink);
    let split = points[points.len() / 2];
    let turn: f64 = if noise(&[seed, 99]) < 0.5 { 0.6 } else { -0.6 };
    let heading = (
        along.0 * turn.cos() - along.1 * turn.sin(),
        along.0 * turn.sin() + along.1 * turn.cos(),
    );
    let reach = length * 0.3;
    let fork = [
        split,
        (
            split.0 + heading.0 * reach * 0.5 + normal.0 * 1.5,
            split.1 + heading.1 * reach * 0.5 + normal.1 * 1.5,
        ),
        (split.0 + heading.0 * reach, split.1 + heading.1 * reach),
    ];
    canvas.polyline(&fork, /*progress*/ 1.0, Ink::Line);
}

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let outer = corners(/*sides*/ 8, radius, FRAC_PI_8);
    let crackle = clock.tick(/*rate*/ 1.5);
    let edges = (8.0 * clock.reveal(/*at*/ 0.0, /*duration*/ 0.7)).ceil() as usize;
    for edge in 0..edges.min(8) {
        let span = (outer[edge], outer[(edge + 1) % 8]);
        bolt(
            canvas,
            span,
            edge as u64 * 97 + crackle,
            /*jitter*/ 1.4,
            Ink::Line,
        );
    }
    let inner_radius = radius - BAND;
    let inner = corners(/*sides*/ 8, inner_radius, FRAC_PI_8);
    let drawn = clock.reveal(/*at*/ 0.2, /*duration*/ 0.6);
    canvas.polygon(&inner, drawn, Ink::Bright);
    let core = inner_radius - BAND;
    if layers >= 1 && core >= 8.0 {
        let square = corners(/*sides*/ 4, core, clock.spin * 0.35);
        let drawn = clock.reveal(LAYER_TIMES[0], /*duration*/ 0.8);
        canvas.polygon(&square, drawn, Ink::Line);
    }
    canvas.arc((0.0, 0.0), /*radius*/ 3.2, (0.0, TAU), |_| Ink::Bright);

    let strike = clock.tick(/*rate*/ 2.6);
    let flash = clock.spin * 2.6 - (strike as f64) < 0.35;
    let mut struck = false;
    for bolt_index in 0..(1 + layers.min(3)) as u64 {
        if noise(&[bolt_index, strike]) < 0.65 && (layers >= 1 || bolt_index == 0) {
            let corner = (noise(&[bolt_index, strike, 1]) * 8.0) as usize % 8;
            // Each strike runs from a corner of the rim straight in to the core ring.
            let target = polar(3.2, FRAC_PI_8 + corner as f64 * TAU / 8.0);
            let ink = if flash || clock.settled {
                Ink::Glint
            } else {
                Ink::Accent
            };
            lightning(
                canvas,
                (outer[corner], target),
                strike * 31 + bolt_index,
                ink,
            );
            struck = struck || flash;
        }
    }
    if layers >= 2 {
        for spark in 0..12_u64 {
            let theta = spark as f64 * TAU / 12.0 + noise(&[spark, strike]) * 0.3;
            if noise(&[spark, strike, 2]) > 0.55 {
                canvas.line(
                    polar(radius + 0.5, theta),
                    polar(radius + 3.0, theta + 0.08),
                    /*progress*/ 1.0,
                    Ink::Accent,
                );
            }
        }
    }

    let flare = |index: usize, style: Style| {
        if struck && noise(&[index as u64, strike]) > 0.6 {
            style.bold()
        } else {
            style
        }
    };
    let path = Path::Polygon {
        sides: 8,
        radius: radius - BAND / 2.0,
        phase: FRAC_PI_8,
    };
    let inscription = Inscription {
        tone: Some(&flare),
        ..Inscription::default()
    };
    inscribe_prompt(canvas, frame, path, inscription);
    if layers >= 1 && core >= 6.0 {
        let path = Path::Polygon {
            sides: 8,
            radius: inner_radius - BAND / 2.0,
            phase: FRAC_PI_8,
        };
        let start = (0.5 - clock.spin * 0.02).rem_euclid(1.0);
        inscribe_reply(
            canvas,
            frame,
            path,
            (start, -1.0),
            /*budget*/ 0.92,
            /*tone*/ None,
        );
    }
}

/// A forked bolt between two sparks.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    let bolt = [(3.0, -radius), (-2.0, 0.5), (2.0, -0.5), (-3.0, radius)];
    canvas.polyline(&bolt, /*progress*/ 1.0, Ink::Accent);
    let echo: Vec<(f64, f64)> = bolt.iter().map(|(x, y)| (x + 1.0, *y)).collect();
    canvas.polyline(&echo, /*progress*/ 1.0, Ink::Bright);
    for side in [-1.0, 1.0] {
        canvas.polyline(
            &[
                (side * radius, -side * 3.5),
                (side * (radius - 2.5), -side * 1.5),
                (side * (radius - 1.0), side * 0.5),
                (side * (radius - 3.5), side * 2.5),
            ],
            /*progress*/ 1.0,
            Ink::Line,
        );
    }
}

/// A lightning strike down into the answer.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    let steps = 5_u32;
    let mut points = vec![(0.0, radius)];
    for step in 1..=steps {
        let swing = if step % 2 == 1 { 3.0 } else { -3.0 };
        points.push((
            swing * (1.0 - f64::from(step) / f64::from(steps + 2)),
            radius + (end - radius) * f64::from(step) / f64::from(steps),
        ));
    }
    canvas.polyline(&points, /*progress*/ 1.0, Ink::Glint);
    let echo: Vec<(f64, f64)> = points.iter().map(|(x, y)| (x + 1.0, *y)).collect();
    canvas.polyline(&echo, /*progress*/ 1.0, Ink::Accent);
}
