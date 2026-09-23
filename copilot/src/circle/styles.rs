//! Renderers for each magic circle style.
//!
//! `magic_circle` owns the circle state, growth and scene layout; every style here draws its own
//! silhouette, motion, text tracks, idle emblem and outlet onto the shared braille canvas. The
//! prompt and public reply text keep their exact graphemes; styles only choose where and how
//! they are written.

mod classic;
mod dark;
mod earth;
mod eerie;
mod fire;
mod holy;
mod tech;
mod thunder;
mod water;
mod wind;

use std::f64::consts::TAU;

use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::circle::canvas::Canvas;
use crate::circle::canvas::Path;
use crate::circle::canvas::polar;
use crate::circle::style::MagicStyle;
use crate::circle::style::Palette;

/// Text bands are two cell rows tall, so their borders never share a row with glyphs at the poles.
pub(crate) const BAND: f64 = 8.0;
/// Charging seconds that unlock each additional layer of a circle.
pub(crate) const LAYER_TIMES: [f64; 4] = [2.5, 5.0, 8.0, 11.0];
const SEPARATOR: &str = " ✦ ";

/// Animation state for one frame. Settled frames draw every unlocked layer completely.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Clock {
    pub(crate) age: f64,
    /// Seconds that drive motion; zero when animations are disabled.
    pub(crate) spin: f64,
    pub(crate) settled: bool,
}

impl Clock {
    /// Drawn fraction of a layer that starts `at` seconds after submission.
    pub(crate) fn reveal(&self, at: f64, duration: f64) -> f64 {
        if self.settled {
            1.0
        } else {
            ((self.age - at) / duration).clamp(0.0, 1.0)
        }
    }

    /// Integer time step for flicker that changes `rate` times a second.
    pub(crate) fn tick(&self, rate: f64) -> u64 {
        (self.spin * rate) as u64
    }
}

/// One frame of an active circle, as handed to a style.
pub(crate) struct Frame<'a> {
    pub(crate) radius: f64,
    /// Layers unlocked beyond each style's base silhouette.
    pub(crate) layers: usize,
    pub(crate) clock: Clock,
    pub(crate) prompt: &'a str,
    pub(crate) reply: &'a str,
    pub(crate) palette: Palette,
}

pub(crate) fn draw<'a>(style: MagicStyle, canvas: &mut Canvas<'a>, frame: &Frame<'a>) {
    match style {
        MagicStyle::Classic => classic::draw(canvas, frame),
        MagicStyle::Wind => wind::draw(canvas, frame),
        MagicStyle::Fire => fire::draw(canvas, frame),
        MagicStyle::Water => water::draw(canvas, frame),
        MagicStyle::Thunder => thunder::draw(canvas, frame),
        MagicStyle::Earth => earth::draw(canvas, frame),
        MagicStyle::Holy => holy::draw(canvas, frame),
        MagicStyle::Dark => dark::draw(canvas, frame),
        MagicStyle::Eerie => eerie::draw(canvas, frame),
        MagicStyle::Tech => tech::draw(canvas, frame),
    }
}

/// The small, static emblem shown before a prompt is submitted.
pub(crate) fn idle(style: MagicStyle, canvas: &mut Canvas<'_>, radius: f64, palette: &Palette) {
    match style {
        MagicStyle::Classic => classic::idle(canvas, radius, palette),
        MagicStyle::Wind => wind::idle(canvas, radius),
        MagicStyle::Fire => fire::idle(canvas, radius),
        MagicStyle::Water => water::idle(canvas, radius),
        MagicStyle::Thunder => thunder::idle(canvas, radius),
        MagicStyle::Earth => earth::idle(canvas, radius),
        MagicStyle::Holy => holy::idle(canvas, radius),
        MagicStyle::Dark => dark::idle(canvas, radius),
        MagicStyle::Eerie => eerie::idle(canvas, radius),
        MagicStyle::Tech => tech::idle(canvas, radius),
    }
}

/// The emission below a settled circle, from its rim at `radius` down to `end` dots.
pub(crate) fn outlet(style: MagicStyle, canvas: &mut Canvas<'_>, radius: f64, end: f64) {
    match style {
        MagicStyle::Classic => classic::outlet(canvas, radius, end),
        MagicStyle::Wind => wind::outlet(canvas, radius, end),
        MagicStyle::Fire => fire::outlet(canvas, radius, end),
        MagicStyle::Water => water::outlet(canvas, radius, end),
        MagicStyle::Thunder => thunder::outlet(canvas, radius, end),
        MagicStyle::Earth => earth::outlet(canvas, radius, end),
        MagicStyle::Holy => holy::outlet(canvas, radius, end),
        MagicStyle::Dark => dark::outlet(canvas, radius, end),
        MagicStyle::Eerie => eerie::outlet(canvas, radius, end),
        MagicStyle::Tech => tech::outlet(canvas, radius, end),
    }
}

/// Corners of a regular polygon, the first at `phase`.
pub(crate) fn corners(sides: usize, radius: f64, phase: f64) -> Vec<(f64, f64)> {
    (0..sides)
        .map(|corner| polar(radius, phase + corner as f64 * TAU / sides as f64))
        .collect()
}

/// How a prompt is written along its track.
#[derive(Clone, Copy, Default)]
pub(crate) struct Inscription<'s> {
    /// Leave a blank cell between glyphs, for stately styles.
    pub(crate) spaced: bool,
    /// Adjusts the style of the glyph at an index, for flicker, fading and glitches.
    pub(crate) tone: Option<&'s dyn Fn(usize, Style) -> Style>,
}

/// Writes the prompt along `path`, repeating short prompts to fill closed tracks.
///
/// Long leftovers keep whole words; short ones would echo the start, so stars close the rest of
/// the track at the separator's spacing. Prompts longer than the track end in an ellipsis. Right
/// after submission the prompt is inscribed glyph by glyph.
pub(crate) fn inscribe_prompt<'a>(
    canvas: &mut Canvas<'a>,
    frame: &Frame<'a>,
    path: Path,
    inscription: Inscription<'_>,
) {
    let mut prompt: Vec<&'a str> = frame
        .prompt
        .graphemes(/*is_extended*/ true)
        .filter(|glyph| glyph.width() > 0)
        .collect();
    if prompt.is_empty() {
        return;
    }
    if inscription.spaced {
        prompt = prompt
            .iter()
            .flat_map(|glyph| [*glyph, " "])
            .take(prompt.len() * 2 - 1)
            .collect();
    }
    let separator: Vec<&str> = SEPARATOR.graphemes(/*is_extended*/ true).collect();
    let unit = prompt.len() + separator.len();
    let capacity = (path.length() / 2.0) as usize + 2;
    let mut glyphs: Vec<&'a str> =
        std::iter::repeat_n(prompt.iter().chain(&separator), capacity / unit + 1)
            .flatten()
            .copied()
            .collect();
    let budget = if path.is_closed() {
        1.0 - 0.3 / TAU
    } else {
        1.0
    };
    let widths: Vec<usize> = glyphs.iter().map(|glyph| glyph.width()).collect();
    let mut slots = canvas.track(&widths, path, (0.0, 1.0), budget);
    let complete = (slots.len() / unit * unit).max(prompt.len());
    if slots.len() < prompt.len() {
        if let Some(last) = slots.len().checked_sub(1) {
            glyphs[last] = "…";
        }
    } else {
        let partial = slots.len() - complete;
        let boundary = if partial * 2 < prompt.len() {
            complete
        } else {
            glyphs[complete..slots.len()]
                .iter()
                .rposition(|glyph| *glyph == " ")
                .map_or(slots.len(), |space| complete + space)
        };
        if boundary < slots.len() {
            let after_space = boundary
                .checked_sub(1)
                .is_some_and(|previous| glyphs[previous] == " ");
            glyphs.truncate(boundary);
            glyphs.extend((0..capacity).map(|offset| {
                if (offset + usize::from(after_space)) % 2 == 1 {
                    "✦"
                } else {
                    " "
                }
            }));
            // Layout is deterministic, so the kept prefix lands on the same cells.
            let widths: Vec<usize> = glyphs.iter().map(|glyph| glyph.width()).collect();
            slots = canvas.track(&widths, path, (0.0, 1.0), budget);
        }
    }
    let total = slots.len();
    let shown = (total as f64 * frame.clock.reveal(/*at*/ 0.1, /*duration*/ 1.2)).ceil() as usize;
    for (index, slot) in slots.into_iter().take(shown).enumerate() {
        let mut style = inscription.tone.map_or(frame.palette.prompt, |tone| {
            tone(index, frame.palette.prompt)
        });
        if shown < total && index + 2 >= shown {
            style = style.bold();
        }
        canvas.write(slot, glyphs[index], glyphs[index].width(), style);
    }
}

/// Writes the newest public reply text at `start` on `path`, with older text trailing behind.
pub(crate) fn inscribe_reply<'a>(
    canvas: &mut Canvas<'a>,
    frame: &Frame<'a>,
    path: Path,
    (start, direction): (f64, f64),
    budget: f64,
    tone: Option<&dyn Fn(usize, Style) -> Style>,
) {
    let glyphs: Vec<&'a str> = frame
        .reply
        .graphemes(/*is_extended*/ true)
        .rev()
        .filter(|glyph| glyph.width() > 0)
        .collect();
    if glyphs.is_empty() {
        return;
    }
    let widths: Vec<usize> = glyphs.iter().map(|glyph| glyph.width()).collect();
    let slots = canvas.track(&widths, path, (start, direction), budget);
    for (index, slot) in slots.into_iter().enumerate() {
        let mut style = tone.map_or(frame.palette.reply, |tone| tone(index, frame.palette.reply));
        if index < 2 && !frame.clock.settled {
            style = style.bold();
        }
        canvas.write(slot, glyphs[index], widths[index], style);
    }
}
