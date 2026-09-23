//! Tech: a segmented HUD ring on a degree scale, a radar sweep, crosshair and corner brackets,
//! with readouts of the real elapsed time and whether a reply has arrived.

use std::f64::consts::FRAC_PI_2;
use std::f64::consts::TAU;

use unicode_segmentation::UnicodeSegmentation;

use crate::magic_canvas::Canvas;
use crate::magic_canvas::Ink;
use crate::magic_canvas::Path;
use crate::magic_canvas::polar;

use super::BAND;
use super::Frame;
use super::Inscription;
use super::corners;
use super::inscribe_prompt;
use super::inscribe_reply;

const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// `T+08.4s` built from static glyphs, so the canvas can borrow it for the frame.
fn elapsed(age: f64) -> Vec<&'static str> {
    let tenths = (age * 10.0).round().clamp(0.0, 99_999.0) as u64;
    let mut whole = Vec::new();
    let mut rest = tenths / 10;
    loop {
        whole.push(DIGITS[(rest % 10) as usize]);
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if whole.len() < 2 {
        whole.push("0");
    }
    whole.reverse();
    let mut glyphs = vec!["T", "+"];
    glyphs.extend(whole);
    glyphs.extend([".", DIGITS[(tenths % 10) as usize], "s"]);
    glyphs
}

pub(super) fn draw<'a>(canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    let (radius, layers, clock) = (frame.radius, frame.layers, frame.clock);
    let origin = (0.0, 0.0);
    let segments = (36.0 * clock.reveal(/*at*/ 0.0, /*duration*/ 0.7)) as u32;
    for segment in 0..segments {
        let start = f64::from(segment) * TAU / 36.0 + clock.spin * 0.12;
        let ink = if segment.is_multiple_of(6) {
            Ink::Bright
        } else {
            Ink::Line
        };
        canvas.arc(origin, radius, (start, TAU / 36.0 * 0.72), |_| ink);
    }
    for tick in 0..72_u32 {
        let theta = f64::from(tick) * TAU / 72.0;
        let length = if tick.is_multiple_of(6) { 2.6 } else { 1.2 };
        canvas.line(
            polar(radius - 1.2, theta),
            polar(radius - 1.2 - length, theta),
            /*progress*/ 1.0,
            Ink::Faint,
        );
    }
    let ring = radius - 10.5;
    canvas.arc(
        origin,
        ring,
        (0.0, TAU * clock.reveal(/*at*/ 0.2, /*duration*/ 0.6)),
        |_| Ink::Line,
    );
    let inner = ring - BAND;
    if layers >= 1 && inner >= 8.0 {
        for dash in 0..24_u32 {
            let start = f64::from(dash) * TAU / 24.0 - clock.spin * 0.2;
            canvas.arc(origin, inner, (start, TAU / 24.0 * 0.45), |_| Ink::Faint);
        }
        let sweep = clock.spin * 1.7;
        canvas.line(
            origin,
            polar(inner - 1.0, sweep),
            /*progress*/ 1.0,
            Ink::Glint,
        );
        for echo in 1..7_u32 {
            let theta = sweep - f64::from(echo) * 0.09;
            let mut reach = 3.0;
            while reach < inner - 1.0 {
                let (x, y) = polar(reach, theta);
                canvas.plot(x, y, Ink::Faint);
                reach += 3.0;
            }
        }
    }
    if layers >= 2 {
        let reach = if inner > 8.0 { inner - 2.0 } else { 8.0 };
        for (sx, sy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            canvas.line(
                (sx * 4.0, sy * 4.0),
                (sx * reach, sy * reach),
                /*progress*/ 1.0,
                Ink::Faint,
            );
        }
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let corner = (sx * (radius - 0.5), sy * (radius - 0.5) * 0.98);
            canvas.line(
                corner,
                (corner.0 - sx * 6.0, corner.1),
                /*progress*/ 1.0,
                Ink::Accent,
            );
            canvas.line(
                corner,
                (corner.0, corner.1 - sy * 5.0),
                /*progress*/ 1.0,
                Ink::Accent,
            );
        }
    }
    if layers >= 3 && inner >= 12.0 {
        let hexagon = corners(/*sides*/ 6, inner * 0.45, -clock.spin * 0.3);
        canvas.polygon(&hexagon, /*progress*/ 1.0, Ink::Line);
    }
    canvas.plot(-0.5, 0.0, Ink::Bright);
    canvas.plot(0.5, 0.0, Ink::Bright);

    let path = Path::Circle {
        radius: radius - 6.8,
        phase: -FRAC_PI_2 + 0.2 * clock.spin,
    };
    inscribe_prompt(canvas, frame, path, Inscription::default());
    if layers >= 1 && inner >= 8.0 {
        let path = Path::Circle {
            radius: ring - BAND / 2.0,
            phase: FRAC_PI_2 - 0.3 * clock.spin,
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
    // Readouts report real state: time since submission and whether reply text has arrived.
    let readout = frame.palette.accent.dim();
    if let Some((left, row)) = canvas.cell_of(-radius + 8.0, -radius + 0.5) {
        for (offset, glyph) in elapsed(clock.age).into_iter().enumerate() {
            canvas.write((left + offset, row), glyph, /*width*/ 1, readout);
        }
    }
    let status = if frame.reply.is_empty() {
        "WAIT"
    } else {
        "RECV"
    };
    if let Some((left, row)) = canvas.cell_of(radius - 14.0, radius - 0.5) {
        for (offset, glyph) in status.graphemes(/*is_extended*/ true).enumerate() {
            canvas.write(
                (left + offset, row),
                glyph,
                /*width*/ 1,
                frame.palette.accent.bold(),
            );
        }
    }
}

/// Targeting brackets around a crosshair.
pub(super) fn idle(canvas: &mut Canvas<'_>, radius: f64) {
    let (half_width, half_height) = (radius + 1.0, radius - 0.5);
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let corner = (sx * half_width, sy * half_height);
        canvas.line(
            corner,
            (corner.0 - sx * 3.0, corner.1),
            /*progress*/ 1.0,
            Ink::Bright,
        );
        canvas.line(
            corner,
            (corner.0, corner.1 - sy * 3.0),
            /*progress*/ 1.0,
            Ink::Bright,
        );
    }
    for (sx, sy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        canvas.line(
            (sx * 2.5, sy * 2.5),
            (sx * 5.0, sy * 4.5),
            /*progress*/ 1.0,
            Ink::Line,
        );
    }
    canvas.plot(-0.5, 0.0, Ink::Accent);
    canvas.plot(0.5, 0.0, Ink::Accent);
}

/// A data line that ends in a chevron.
pub(super) fn outlet(canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    let mut y = radius + 1.0;
    let mut row = 0_u32;
    while y <= end - 3.0 {
        let ink = if row.is_multiple_of(3) {
            Ink::Bright
        } else {
            Ink::Line
        };
        canvas.plot(-0.5, y, ink);
        canvas.plot(0.5, y, ink);
        y += 1.0;
        row += 1;
    }
    canvas.polyline(
        &[(-3.0, end - 3.0), (0.0, end), (3.0, end - 3.0)],
        /*progress*/ 1.0,
        Ink::Accent,
    );
}
