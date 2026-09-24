//! Braille drawing surface and path text layout for the presentation-only magic circles.
//!
//! Coordinates are braille dots relative to the canvas centre. A terminal cell is two dots wide
//! and four dots tall, which keeps circles round in the usual 1:2 terminal fonts.

use std::f64::consts::TAU;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::magic_style::Palette;

const BITS: [[u8; 2]; 4] = [[0, 3], [1, 4], [2, 5], [6, 7]];

/// Weight of a stroke. A braille cell has one style, so the strongest ink in it wins; each
/// magic style's palette decides how an ink looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Ink {
    Faint,
    Line,
    Bright,
    Accent,
    Glint,
}

/// Deterministic value in `[0, 1)` for a sequence of integers, used for flicker and scatter.
pub(crate) fn noise(parts: &[u64]) -> f64 {
    let hash = parts.iter().fold(0_u64, |hash, part| splitmix(hash ^ part));
    (hash >> 11) as f64 / (1_u64 << 53) as f64
}

fn splitmix(value: u64) -> u64 {
    let mut mixed = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}

/// Radius of a ring modulated by up to two `(amplitude, frequency, offset)` sine waves.
pub(crate) fn wave_radius(base: f64, waves: [(f64, f64, f64); 2], theta: f64) -> f64 {
    base + waves
        .iter()
        .map(|(amplitude, frequency, offset)| amplitude * (frequency * theta + offset).sin())
        .sum::<f64>()
}

/// A track for strokes and text, as a function of `u` in `[0, 1]`. All paths run clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Path {
    Circle {
        radius: f64,
        phase: f64,
    },
    /// A ring whose radius follows [`wave_radius`].
    Wave {
        base: f64,
        waves: [(f64, f64, f64); 2],
        phase: f64,
    },
    /// A regular polygon with its first corner at `phase`.
    Polygon {
        sides: usize,
        radius: f64,
        phase: f64,
    },
    /// An axis-aligned square that starts `offset` of the way round from its top-left corner.
    Square {
        half: f64,
        offset: f64,
    },
    /// An open spiral from `outer` to `inner` radius.
    Spiral {
        outer: f64,
        inner: f64,
        phase: f64,
        turns: f64,
    },
}

impl Path {
    pub(crate) fn is_closed(self) -> bool {
        !matches!(self, Path::Spiral { .. })
    }

    pub(crate) fn point(self, u: f64) -> (f64, f64) {
        match self {
            Path::Circle { radius, phase } => polar(radius, phase + u * TAU),
            Path::Wave { base, waves, phase } => {
                let theta = phase + u * TAU;
                polar(wave_radius(base, waves, theta), theta)
            }
            Path::Polygon {
                sides,
                radius,
                phase,
            } => {
                let position = u.rem_euclid(1.0) * sides as f64;
                let corner = position.floor();
                let from = polar(radius, phase + corner * TAU / sides as f64);
                let to = polar(radius, phase + (corner + 1.0) * TAU / sides as f64);
                lerp(from, to, position - corner)
            }
            Path::Square { half, offset } => {
                let corners = [(-half, -half), (half, -half), (half, half), (-half, half)];
                let position = (u + offset).rem_euclid(1.0) * 4.0;
                let side = position.floor();
                let index = side as usize % 4;
                lerp(corners[index], corners[(index + 1) % 4], position - side)
            }
            Path::Spiral {
                outer,
                inner,
                phase,
                turns,
            } => polar(outer + (inner - outer) * u, phase + u * TAU * turns),
        }
    }

    fn tangent(self, u: f64) -> (f64, f64) {
        if let Path::Circle { phase, .. } = self {
            let theta = phase + u * TAU;
            return (-theta.sin(), theta.cos());
        }
        let (before, after) = (self.point(u - 1e-4), self.point(u + 1e-4));
        (after.0 - before.0, after.1 - before.1)
    }

    pub(crate) fn length(self) -> f64 {
        if let Path::Circle { radius, .. } = self {
            return TAU * radius.abs();
        }
        let samples = 180;
        (0..samples)
            .map(|step| {
                let from = self.point(step as f64 / samples as f64);
                let to = self.point((step + 1) as f64 / samples as f64);
                (to.0 - from.0).hypot(to.1 - from.1)
            })
            .sum()
    }
}

pub(crate) fn polar(radius: f64, theta: f64) -> (f64, f64) {
    (radius * theta.cos(), radius * theta.sin())
}

fn lerp(from: (f64, f64), to: (f64, f64), t: f64) -> (f64, f64) {
    (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
}

pub(crate) struct Canvas<'a> {
    width: usize,
    height: usize,
    center: (f64, f64),
    dots: Vec<(u8, Option<Ink>)>,
    reserved: Vec<bool>,
    glyphs: Vec<(usize, usize, &'a str, usize, Style)>,
    /// Rows of dots `[from, to)` drawn `dx` dots sideways, for glitch frames.
    shift: Option<(f64, f64, f64)>,
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(width: u16, height: u16, center_y: f64) -> Self {
        let (width, height) = (usize::from(width), usize::from(height));
        Self {
            width,
            height,
            center: (width as f64 - 0.5, center_y),
            dots: vec![(0, None); width * height],
            reserved: vec![false; width * height],
            glyphs: Vec::new(),
            shift: None,
        }
    }

    /// Height of the canvas in dots.
    pub(crate) fn dot_height(&self) -> f64 {
        (self.height * 4) as f64
    }

    /// Displaces a band of dot rows, measured from the top of the canvas, until cleared.
    pub(crate) fn set_shift(&mut self, shift: Option<(f64, f64, f64)>) {
        self.shift = shift;
    }

    fn dot(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        // Round ties toward the centre so mirrored points land on mirrored dots.
        let snap = |offset: f64, centre: f64| centre + offset - offset.signum() * 1e-6;
        let (mut x, y) = (snap(x, self.center.0), snap(y, self.center.1));
        if let Some((from, to, dx)) = self.shift
            && (from..to).contains(&y)
        {
            x += dx;
        }
        let (x, y) = (x.round(), y.round());
        let inside =
            x >= 0.0 && y >= 0.0 && x < (self.width * 2) as f64 && y < (self.height * 4) as f64;
        inside.then_some((x as usize, y as usize))
    }

    pub(crate) fn cell_of(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        self.dot(x, y).map(|(x, y)| (x / 2, y / 4))
    }

    pub(crate) fn plot(&mut self, x: f64, y: f64, ink: Ink) {
        if let Some((x, y)) = self.dot(x, y) {
            let cell = &mut self.dots[(y / 4) * self.width + x / 2];
            cell.0 |= 1 << BITS[y % 4][x % 2];
            cell.1 = cell.1.max(Some(ink));
        }
    }

    /// Plots `sweep` radians of a circle around `center`, starting at `start`.
    ///
    /// Negative sweeps run counter-clockwise; `ink` receives each angle so highlights can travel.
    pub(crate) fn arc(
        &mut self,
        center: (f64, f64),
        radius: f64,
        (start, sweep): (f64, f64),
        ink: impl Fn(f64) -> Ink,
    ) {
        if radius <= 0.0 || sweep.abs() < 1e-9 {
            return;
        }
        // Multiples of four keep full rings symmetric about both axes.
        let steps = (radius * sweep.abs() * 1.3 / 4.0).ceil().max(1.0) as usize * 4;
        for step in 0..=steps {
            let theta = start + sweep * step as f64 / steps as f64;
            self.plot(
                center.0 + radius * theta.cos(),
                center.1 + radius * theta.sin(),
                ink(theta),
            );
        }
    }

    /// Draws the first `progress` fraction of a straight stroke.
    pub(crate) fn line(&mut self, from: (f64, f64), to: (f64, f64), progress: f64, ink: Ink) {
        if progress <= 0.0 {
            return;
        }
        let steps = ((to.0 - from.0).hypot(to.1 - from.1) * progress * 1.3)
            .ceil()
            .max(1.0) as usize;
        for step in 0..=steps {
            let t = progress * step as f64 / steps as f64;
            self.plot(
                from.0 + (to.0 - from.0) * t,
                from.1 + (to.1 - from.1) * t,
                ink,
            );
        }
    }

    /// Draws connected segments, revealing them in order as `progress` goes from 0 to 1.
    pub(crate) fn polyline(&mut self, points: &[(f64, f64)], progress: f64, ink: Ink) {
        let segments = points.len().saturating_sub(1);
        let drawn = progress * segments as f64;
        for (index, pair) in points.windows(2).enumerate() {
            self.line(
                pair[0],
                pair[1],
                (drawn - index as f64).clamp(0.0, 1.0),
                ink,
            );
        }
    }

    /// Like [`Canvas::polyline`], with a closing segment back to the first point.
    pub(crate) fn polygon(&mut self, points: &[(f64, f64)], progress: f64, ink: Ink) {
        let mut closed = points.to_vec();
        closed.extend(points.first().copied());
        self.polyline(&closed, progress, ink);
    }

    /// Draws the first `progress` of a parametric curve sampled `samples` times over `[0, 1]`.
    pub(crate) fn curve(
        &mut self,
        samples: usize,
        progress: f64,
        point: impl Fn(f64) -> (f64, f64),
        ink: impl Fn(f64) -> Ink,
    ) {
        let count = ((samples as f64 * progress).ceil() as usize).max(1);
        let mut last = point(0.0);
        for step in 1..=count {
            let t = progress * step as f64 / count as f64;
            let next = point(t);
            self.line(last, next, /*progress*/ 1.0, ink(t));
            last = next;
        }
    }

    /// Finds cells along `path` for glyphs of the given widths, in order.
    ///
    /// Walking starts at `start` and moves by `direction` (±1) for at most `budget` of the path.
    /// Glyphs pack edge to edge where the path runs horizontally so words stay readable, and
    /// take one row each where it runs steeply. Cells holding earlier text are skipped. Fewer
    /// slots than widths are returned when the budget runs out.
    pub(crate) fn track(
        &self,
        widths: &[usize],
        path: Path,
        (start, direction): (f64, f64),
        budget: f64,
    ) -> Vec<(usize, usize)> {
        let mut reserved = self.reserved.clone();
        let step = 0.5 / path.length().max(1.0);
        let closed = path.is_closed();
        let mut travelled = 0.0;
        let mut last_row = None;
        let mut slots = Vec::with_capacity(widths.len());
        'glyphs: for &width in widths {
            while travelled <= budget {
                let mut u = start + direction * travelled;
                travelled += step;
                if closed {
                    u = u.rem_euclid(1.0);
                } else if !(0.0..=1.0).contains(&u) {
                    break 'glyphs;
                }
                let (x, y) = path.point(u);
                let Some((column, row)) = self.cell_of(x, y) else {
                    continue;
                };
                let Some(left) = column.checked_sub(width / 2) else {
                    continue;
                };
                let (dx, dy) = path.tangent(u);
                let steep = dy.abs() > 2.0 * dx.abs();
                if left + width > self.width || (steep && last_row == Some(row)) {
                    continue;
                }
                let cells = row * self.width + left..row * self.width + left + width;
                if reserved[cells.clone()].iter().any(|taken| *taken) {
                    continue;
                }
                reserved[cells].fill(true);
                slots.push((left, row));
                last_row = Some(row);
                continue 'glyphs;
            }
            break;
        }
        slots
    }

    /// Places a glyph over the braille layer and keeps later text out of its cells.
    pub(crate) fn write(
        &mut self,
        (left, row): (usize, usize),
        glyph: &'a str,
        width: usize,
        style: Style,
    ) {
        let start = row * self.width + left;
        let end = (start + width).min((row + 1) * self.width);
        self.reserved[start..end].fill(true);
        self.glyphs.push((left, row, glyph, end - start, style));
    }

    /// Keeps strokes out of `width` cells from `(left, row)`, where other content goes.
    pub(crate) fn reserve(&mut self, (left, row): (usize, usize), width: usize) {
        if row >= self.height || left >= self.width {
            return;
        }
        let start = row * self.width + left;
        let end = (start + width).min((row + 1) * self.width);
        self.reserved[start..end].fill(true);
    }

    pub(crate) fn paint(self, area: Rect, buf: &mut Buffer, palette: &Palette) {
        for (index, (mask, ink)) in self.dots.into_iter().enumerate() {
            // Text replaces the strokes in its cells instead of inheriting their style.
            let Some(ink) = ink.filter(|_| !self.reserved[index]) else {
                continue;
            };
            let glyph = char::from_u32(0x2800 + u32::from(mask)).unwrap_or(' ');
            let position = (
                area.x + (index % self.width) as u16,
                area.y + (index / self.width) as u16,
            );
            buf[position].set_char(glyph).set_style(palette.ink(ink));
        }
        for (left, row, glyph, width, style) in self.glyphs {
            buf.set_stringn(
                area.x + left as u16,
                area.y + row as u16,
                glyph,
                width,
                style,
            );
        }
    }
}
