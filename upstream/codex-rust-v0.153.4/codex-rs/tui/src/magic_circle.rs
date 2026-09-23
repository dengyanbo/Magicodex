//! Optional, presentation-only magic circle. No text from this module enters model context.

use std::collections::VecDeque;
use std::f64::consts::TAU;
use std::time::Duration;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::history_cell::sanitize_user_text;
use crate::render::renderable::Renderable;

pub(crate) const STYLES: &[(&str, &str)] = &[("classic", "Concentric circles with orbiting text")];
pub(crate) const USAGE: &str = "Usage: /magic on|off|list";
const TEXT_LIMIT: usize = 192;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MagicCircle {
    started: Option<Instant>,
    first_reply: Option<Duration>,
    prompt: String,
    current_reply: String,
    completed_replies: VecDeque<String>,
}

#[derive(Debug, PartialEq)]
struct Geometry {
    radius: f64,
    rings: usize,
}

impl MagicCircle {
    pub(crate) fn submit(&mut self, text: &str, now: Instant) {
        self.begin(now);
        self.prompt = display_fragment(text);
    }

    pub(crate) fn begin(&mut self, now: Instant) {
        if self.started.is_none() {
            self.started = Some(now);
            self.first_reply = None;
            self.current_reply.clear();
            self.completed_replies.clear();
        }
    }

    pub(crate) fn reply_delta(&mut self, delta: &str, now: Instant) {
        if let Some(started) = self.started {
            let fragment = display_fragment(delta);
            if !fragment.trim().is_empty() {
                self.first_reply
                    .get_or_insert(now.saturating_duration_since(started));
            }
            self.current_reply.push_str(&fragment);
            self.current_reply = self
                .current_reply
                .graphemes(/*is_extended*/ true)
                .rev()
                .take(TEXT_LIMIT)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
        }
    }

    pub(crate) fn complete_reply(&mut self, text: &str, now: Instant) {
        if self.started.is_some() {
            self.reply_delta(text, now);
            let text = display_fragment(text);
            if !text.trim().is_empty() {
                self.completed_replies.push_back(text);
                while self.completed_replies.len() > 2 {
                    self.completed_replies.pop_front();
                }
            }
            self.current_reply.clear();
        }
    }

    pub(crate) fn finish(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn is_active(&self) -> bool {
        self.started.is_some()
    }

    fn geometry(&self, now: Instant) -> Geometry {
        let Some(started) = self.started else {
            return Geometry {
                radius: 4.0,
                rings: 1,
            };
        };
        let age = self
            .first_reply
            .unwrap_or_else(|| now.saturating_duration_since(started))
            .as_secs_f64();
        Geometry {
            radius: (14.0 + age).min(40.0),
            rings: 2 + (age / 4.0).floor().min(5.0) as usize,
        }
    }
}

fn display_fragment(text: &str) -> String {
    sanitize_user_text(text.into())
        .replace(['\n', '\r', '\t'], " ")
        .graphemes(/*is_extended*/ true)
        .filter(|glyph| {
            !glyph
                .chars()
                .any(|c| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        })
        .take(TEXT_LIMIT)
        .collect()
}

pub(crate) struct MagicView<'a> {
    pub(crate) circle: &'a MagicCircle,
    pub(crate) animations_enabled: bool,
}

impl Renderable for MagicView<'_> {
    fn desired_height(&self, width: u16) -> u16 {
        if width < 16 {
            0
        } else if self.circle.is_active() {
            21
        } else {
            3
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_at(area, buf, Instant::now());
    }
}

impl MagicView<'_> {
    fn render_at(&self, area: Rect, buf: &mut Buffer, now: Instant) {
        if area.width < 8 || area.height < 3 {
            return;
        }
        let width = area.width.min(100);
        let height = area.height.min(21);
        let area = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        let geometry = self.circle.geometry(now);
        let radius = geometry
            .radius
            .min(f64::from(height * 2 - 2))
            .min(f64::from(width - 2));
        let rotation = if self.animations_enabled {
            self.circle.started.map_or(/*default*/ 0.0, |started| {
                now.saturating_duration_since(started).as_secs_f64() * 0.3
            })
        } else {
            0.0
        };
        let mut pixels = vec![0_u8; usize::from(width) * usize::from(height)];
        let cx = (f64::from(width) * 2.0 - 1.0) / 2.0;
        let cy = (f64::from(height) * 4.0 - 1.0) / 2.0;
        let mut plot = |x: f64, y: f64| {
            let x = (x + cx).round() as i32;
            let y = (y + cy).round() as i32;
            if x >= 0 && y >= 0 && x < i32::from(width) * 2 && y < i32::from(height) * 4 {
                let bits = [[0, 3], [1, 4], [2, 5], [6, 7]];
                pixels[(y as usize / 4) * usize::from(width) + x as usize / 2] |=
                    1 << bits[y as usize % 4][x as usize % 2];
            }
        };
        for ring in 0..geometry.rings {
            let r = radius * (1.0 - ring as f64 * 0.72 / geometry.rings as f64);
            for step in 0..240 {
                let theta = f64::from(step) * TAU / 240.0 + rotation;
                plot(r * theta.cos(), r * theta.sin());
            }
        }
        if self.circle.is_active() {
            for vertex in 0..geometry.rings * 2 + 2 {
                let a = vertex as f64 * TAU / (geometry.rings * 2 + 2) as f64 + rotation;
                let b = a + TAU * 2.0 / (geometry.rings * 2 + 2) as f64;
                for step in 0..40 {
                    let t = f64::from(step) / 40.0;
                    plot(
                        radius * 0.8 * (a.cos() * (1.0 - t) + b.cos() * t),
                        radius * 0.8 * (a.sin() * (1.0 - t) + b.sin() * t),
                    );
                }
            }
        }
        for (index, mask) in pixels.into_iter().enumerate() {
            if mask != 0 {
                let glyph = char::from_u32(0x2800 + u32::from(mask)).unwrap_or(' ');
                buf[(
                    area.x + (index % usize::from(width)) as u16,
                    area.y + (index / usize::from(width)) as u16,
                )]
                    .set_char(glyph)
                    .set_fg(Color::Magenta);
            }
        }
        if self.circle.is_active() {
            let replies = std::iter::once(self.circle.current_reply.as_str())
                .filter(|text| !text.is_empty())
                .chain(
                    self.circle
                        .completed_replies
                        .iter()
                        .rev()
                        .map(String::as_str),
                );
            let tracks = std::iter::once(self.circle.prompt.as_str()).chain(replies.take(2));
            let mut occupied = vec![false; usize::from(width) * usize::from(height)];
            for (track, text) in tracks.enumerate() {
                let r = radius * [1.0, 0.64, 0.4][track];
                let direction = if track % 2 == 0 { 1.0 } else { -1.0 };
                let mut angle = direction * rotation + track as f64 * TAU / 3.0;
                let start_angle = angle;
                for glyph in text.graphemes(/*is_extended*/ true) {
                    let cells = glyph.width();
                    if cells == 0 || cells > usize::from(width) {
                        continue;
                    }
                    let x = ((cx + r * angle.cos()) / 2.0).round() as i32 - cells as i32 / 2;
                    let y = ((cy + r * angle.sin()) / 4.0).round() as i32;
                    angle += (cells as f64 + 1.0) * 2.0 / r.max(1.0);
                    if angle - start_angle > TAU - 0.25 {
                        break;
                    }
                    if x < 0
                        || y < 0
                        || x as usize + cells > usize::from(width)
                        || y >= i32::from(height)
                    {
                        continue;
                    }
                    let index = y as usize * usize::from(width) + x as usize;
                    if occupied[index..index + cells].iter().any(|used| *used) {
                        continue;
                    }
                    occupied[index..index + cells].fill(true);
                    buf.set_stringn(
                        area.x + x as u16,
                        area.y + y as u16,
                        glyph,
                        cells,
                        Style::default().fg(if track == 0 {
                            Color::Cyan
                        } else {
                            Color::Reset
                        }),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "magic_circle_tests.rs"]
mod tests;
