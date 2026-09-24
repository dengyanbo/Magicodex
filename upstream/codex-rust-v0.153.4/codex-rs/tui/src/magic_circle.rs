//! Optional, presentation-only magic circles. No text from this module enters model context.
//!
//! A circle grows with waiting time and gains layers until the first assistant reply freezes
//! it. This module owns that state and the scene layout; `magic_styles` draws each selectable
//! style. The prompt and the latest public assistant text, never reasoning, are inscribed.

use std::time::Duration;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation;

use crate::history_cell::sanitize_user_text;
use crate::magic_canvas::Canvas;
use crate::magic_sides::spells::Chronicle;
use crate::magic_style::MagicStyle;
use crate::magic_styles;
use crate::magic_styles::Clock;
use crate::magic_styles::Frame;
use crate::magic_styles::LAYER_TIMES;
use crate::render::renderable::Renderable;

pub(crate) const USAGE: &str = "Usage: /magic on|off|list|<style>";
pub(crate) const CIRCLE_ROWS: u16 = 21;
/// Rows below an outlet circle for the emission that leads into the answer.
pub(crate) const OUTLET_ROWS: u16 = 3;
const IDLE_ROWS: u16 = 5;
pub(crate) const MAX_WIDTH: u16 = 99;
const TEXT_LIMIT: usize = 192;
const IDLE_RADIUS: f64 = 8.0;
const START_RADIUS: f64 = 20.0;
const MAX_RADIUS: f64 = 40.0;
const GROWTH_SECONDS: f64 = 16.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MagicCircle {
    started: Option<Instant>,
    first_reply: Option<Duration>,
    prompt: String,
    /// Tail of the latest public assistant text.
    reply: String,
    reply_complete: bool,
    /// What this turn has cast, for the pages beside the circle.
    pub(crate) chronicle: Chronicle,
}

#[derive(Debug, PartialEq)]
struct Geometry {
    radius: f64,
    /// Layers unlocked beyond each style's base silhouette.
    layers: usize,
}

impl MagicCircle {
    pub(crate) fn submit(&mut self, text: &str, now: Instant) {
        self.begin(now);
        self.prompt = display_text(text)
            .graphemes(/*is_extended*/ true)
            .take(TEXT_LIMIT)
            .collect();
    }

    pub(crate) fn begin(&mut self, now: Instant) {
        if self.started.is_none() {
            self.started = Some(now);
            self.first_reply = None;
            self.reply.clear();
            self.reply_complete = false;
        }
    }

    pub(crate) fn reply_delta(&mut self, delta: &str, now: Instant) {
        let Some(started) = self.started else {
            return;
        };
        let fragment = display_text(delta);
        let blank = fragment.trim().is_empty();
        if !blank {
            self.first_reply
                .get_or_insert(now.saturating_duration_since(started));
        }
        if self.reply_complete {
            if blank {
                return;
            }
            self.reply.clear();
            self.reply_complete = false;
        }
        self.reply.push_str(&fragment);
        self.reply = keep_tail(&self.reply);
    }

    pub(crate) fn complete_reply(&mut self, text: &str, now: Instant) {
        let Some(started) = self.started else {
            return;
        };
        let text = display_text(text);
        if !text.trim().is_empty() {
            self.first_reply
                .get_or_insert(now.saturating_duration_since(started));
            self.reply = keep_tail(&text);
            self.reply_complete = true;
        }
    }

    pub(crate) fn finish(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn is_active(&self) -> bool {
        self.started.is_some()
    }

    /// Time since the prompt, while a turn is active.
    pub(crate) fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.started
            .map(|started| now.saturating_duration_since(started))
    }

    /// Layers unlocked so far, which the side pages name as stages.
    pub(crate) fn layers(&self, now: Instant) -> usize {
        self.geometry(now).layers
    }

    fn geometry(&self, now: Instant) -> Geometry {
        let Some(started) = self.started else {
            return Geometry {
                radius: IDLE_RADIUS,
                layers: 0,
            };
        };
        let charge = self
            .first_reply
            .unwrap_or_else(|| now.saturating_duration_since(started))
            .as_secs_f64();
        let growth = (charge / GROWTH_SECONDS).min(1.0);
        Geometry {
            radius: START_RADIUS + (MAX_RADIUS - START_RADIUS) * (1.0 - (1.0 - growth).powi(3)),
            layers: LAYER_TIMES.iter().filter(|at| **at <= charge).count(),
        }
    }
}

/// Single-line printable text without control characters or bidi overrides.
pub(crate) fn display_text(text: &str) -> String {
    sanitize_user_text(text.into())
        .replace(['\n', '\r', '\t'], " ")
        .graphemes(/*is_extended*/ true)
        .filter(|glyph| {
            !glyph
                .chars()
                .any(|c| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        })
        .collect()
}

fn keep_tail(text: &str) -> String {
    let count = text.graphemes(/*is_extended*/ true).count();
    text.graphemes(/*is_extended*/ true)
        .skip(count.saturating_sub(TEXT_LIMIT))
        .collect()
}

/// Where a circle is drawn: charging above the composer, or fixed in history as the outlet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MagicScene {
    Live,
    /// Settled drawing with the style's emission below it, leading into the answer.
    Outlet,
}

pub(crate) struct MagicView<'a> {
    pub(crate) circle: &'a MagicCircle,
    pub(crate) style: MagicStyle,
    pub(crate) animations_enabled: bool,
    pub(crate) scene: MagicScene,
}

impl Renderable for MagicView<'_> {
    fn desired_height(&self, width: u16) -> u16 {
        if width < 16 {
            0
        } else if self.circle.is_active() {
            CIRCLE_ROWS
        } else {
            IDLE_ROWS
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_at(area, buf, Instant::now());
    }
}

impl MagicView<'_> {
    pub(crate) fn render_at(&self, area: Rect, buf: &mut Buffer, now: Instant) {
        let outlet = self.scene == MagicScene::Outlet;
        let cone_rows = if outlet { OUTLET_ROWS } else { 0 };
        // Odd sizes put the centre in the middle of a cell, so core glyphs and beams align.
        let rows = odd(area.height.saturating_sub(cone_rows).min(CIRCLE_ROWS));
        let width = odd(area.width.min(MAX_WIDTH));
        if width < 7 || rows < 3 {
            return;
        }
        let height = rows + cone_rows;
        let area = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let geometry = self.circle.geometry(now);
        let radius = geometry
            .radius
            .min(f64::from(rows * 2 - 2))
            .min(f64::from(width - 2));
        let bottom = f64::from(rows * 4) - 1.0;
        let center_y = if outlet {
            bottom - radius
        } else {
            bottom / 2.0
        };
        let palette = self.style.palette();
        let mut canvas = Canvas::new(width, height, center_y);
        match self.circle.started {
            None => magic_styles::idle(self.style, &mut canvas, radius, &palette),
            Some(started) => {
                let age = now.saturating_duration_since(started).as_secs_f64();
                let frame = Frame {
                    radius,
                    layers: geometry.layers,
                    clock: Clock {
                        age,
                        spin: if self.animations_enabled { age } else { 0.0 },
                        settled: outlet || !self.animations_enabled,
                    },
                    prompt: &self.circle.prompt,
                    reply: &self.circle.reply,
                    palette,
                };
                magic_styles::draw(self.style, &mut canvas, &frame);
                if outlet {
                    let end = f64::from(height * 4) - 1.0 - center_y;
                    magic_styles::outlet(self.style, &mut canvas, radius, end);
                }
            }
        }
        canvas.paint(area, buf, &palette);
    }
}

pub(crate) fn odd(size: u16) -> u16 {
    if size.is_multiple_of(2) {
        size.saturating_sub(1)
    } else {
        size
    }
}

#[cfg(test)]
#[path = "magic_circle_tests.rs"]
mod tests;
