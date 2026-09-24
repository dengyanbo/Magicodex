//! The end of a circle: it dims, scatters outward from its centre and thins to nothing.
//!
//! This works on a finished frame of the region, so every style, its outlet and the art beside
//! it end the same way without knowing about it. Braille dots drift outward and thin out; text
//! crumbles into dust at its own moment; bold goes first, then the colours dim. Without motion
//! the frame only dims, and is gone at the end.

use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use crate::circle::canvas::noise;

/// How much further out the fastest dust ends up, as a share of its distance from the centre.
const SPREAD: f64 = 0.9;
/// Random drift of the dust at the end, in dots.
const DRIFT: f64 = 6.0;
/// Braille bit of each dot, by row and column within a cell.
const BITS: [[u8; 2]; 4] = [[0, 3], [1, 4], [2, 5], [6, 7]];

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The share of the dust still there `progress` into its fade.
fn surviving(progress: f64) -> f64 {
    if progress < 0.2 {
        1.0
    } else {
        ((1.0 - progress) / 0.8).max(0.0).powf(1.5)
    }
}

/// The dimmer shade of a colour among the 16 ANSI colours.
fn base(color: Color) -> Color {
    match color {
        Color::LightRed => Color::Red,
        Color::LightGreen => Color::Green,
        Color::LightYellow => Color::Yellow,
        Color::LightBlue => Color::Blue,
        Color::LightMagenta => Color::Magenta,
        Color::LightCyan => Color::Cyan,
        Color::White => Color::Gray,
        other => other,
    }
}

/// `style` `progress` into the fade: bold goes first, then the colour dims.
fn dim(style: Style, progress: f64) -> Style {
    if progress < 0.1 {
        return style;
    }
    let style = style.remove_modifier(Modifier::BOLD);
    if progress < 0.3 {
        return style;
    }
    let style = match style.fg {
        Some(color) => style.fg(base(color)),
        None => style,
    };
    style.add_modifier(Modifier::DIM)
}

/// Where the dot at `dot` is `progress` into its fade around `center`, or `None` once it has
/// thinned away. `key` makes each dot's speed, drift and end its own.
fn scatter(
    dot: (f64, f64),
    center: (f64, f64),
    progress: f64,
    key: [u64; 3],
) -> Option<(f64, f64)> {
    let chance = |salt: u64| noise(&[key[0], key[1], key[2], salt]);
    if chance(1) >= surviving(progress) {
        return None;
    }
    let ease = smoothstep(progress);
    let scale = 1.0 + SPREAD * ease * (0.5 + chance(2));
    let drift = (
        (chance(3) - 0.5) * DRIFT * ease,
        (chance(4) - 0.5) * DRIFT * ease,
    );
    Some((
        center.0 + (dot.0 - center.0) * scale + drift.0,
        center.1 + (dot.1 - center.1) * scale + drift.1,
    ))
}

/// Dots and text of a fading frame, in cells of its area.
struct Dust {
    width: usize,
    height: usize,
    origin: (f64, f64),
    cells: Vec<(u8, Option<Style>)>,
}

impl Dust {
    fn plot(&mut self, (x, y): (f64, f64), style: Style) {
        let (x, y) = ((x - self.origin.0).round(), (y - self.origin.1).round());
        let inside =
            x >= 0.0 && y >= 0.0 && x < (self.width * 2) as f64 && y < (self.height * 4) as f64;
        if !inside {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        let cell = &mut self.cells[(y / 4) * self.width + x / 2];
        cell.0 |= 1 << BITS[y % 4][x % 2];
        cell.1.get_or_insert(style);
    }
}

/// Draws `frame` into `buf` `progress` (0 to 1) of the way through its fade around `center`,
/// given in dots of the whole buffer. Cells of `buf` the fade leaves empty are not touched.
pub(crate) fn draw(
    frame: &Buffer,
    buf: &mut Buffer,
    center: (f64, f64),
    progress: f64,
    moving: bool,
) {
    if progress >= 1.0 {
        return;
    }
    let area = frame.area;
    let (width, height) = (usize::from(area.width), usize::from(area.height));
    let mut dust = Dust {
        width,
        height,
        origin: (f64::from(area.x) * 2.0, f64::from(area.y) * 4.0),
        cells: vec![(0, None); width * height],
    };
    let mut standing = Vec::new();
    for row in 0..height {
        for column in 0..width {
            let cell = &frame[(area.x + column as u16, area.y + row as u16)];
            let symbol = cell.symbol();
            let style = dim(cell.style(), progress);
            let corner = (
                dust.origin.0 + (column * 2) as f64,
                dust.origin.1 + (row * 4) as f64,
            );
            let key = |dot: usize| [corner.0 as u64, corner.1 as u64, dot as u64];
            let mut mask = symbol
                .chars()
                .next()
                .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
                .map_or(0, |c| (u32::from(c) - 0x2800) as u8);
            let mut local = progress;
            if mask == 0 && !symbol.trim().is_empty() {
                // Text stands until its moment, then crumbles into two specks of dust.
                let vanish = 0.15 + 0.5 * noise(&[corner.0 as u64, corner.1 as u64, 7]);
                if !moving || progress < vanish {
                    standing.push((column, row, symbol.to_string(), style));
                    continue;
                }
                local = (progress - vanish) / (1.0 - vanish);
                for speck in 0..2 {
                    mask |=
                        1 << (noise(&[corner.0 as u64, corner.1 as u64, 11 + speck]) * 8.0) as u8;
                }
            }
            for (dy, bits) in BITS.iter().enumerate() {
                for (dx, bit) in bits.iter().enumerate() {
                    if mask & (1 << bit) == 0 {
                        continue;
                    }
                    let dot = (corner.0 + dx as f64, corner.1 + dy as f64);
                    let moved = if moving {
                        scatter(dot, center, local, key(dy * 2 + dx))
                    } else {
                        Some(dot)
                    };
                    if let Some(moved) = moved {
                        dust.plot(moved, style);
                    }
                }
            }
        }
    }
    let mut covered = vec![false; width * height];
    for (column, row, symbol, _) in &standing {
        let end = (column + symbol.width().max(1)).min(width);
        covered[row * width + column..row * width + end].fill(true);
    }
    for (index, (mask, style)) in dust.cells.into_iter().enumerate() {
        let Some(style) = style.filter(|_| mask != 0 && !covered[index]) else {
            continue;
        };
        let position = (
            area.x + (index % width) as u16,
            area.y + (index / width) as u16,
        );
        let glyph = char::from_u32(0x2800 + u32::from(mask)).unwrap_or(' ');
        buf[position].set_char(glyph).set_style(style);
    }
    for (column, row, symbol, style) in standing {
        let width = symbol.width().max(1);
        buf.set_stringn(
            area.x + column as u16,
            area.y + row as u16,
            symbol,
            width,
            style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn dots(buf: &Buffer) -> Vec<(f64, f64)> {
        let mut found = Vec::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let Some(c) = buf[(x, y)].symbol().chars().next() else {
                    continue;
                };
                if !('\u{2801}'..='\u{28ff}').contains(&c) {
                    continue;
                }
                let mask = (u32::from(c) - 0x2800) as u8;
                for (dy, bits) in BITS.iter().enumerate() {
                    for (dx, bit) in bits.iter().enumerate() {
                        if mask & (1 << bit) != 0 {
                            found.push((
                                f64::from(x) * 2.0 + dx as f64,
                                f64::from(y) * 4.0 + dy as f64,
                            ));
                        }
                    }
                }
            }
        }
        found
    }

    /// A ring of bold magenta dots around the centre of a 60 × 21 area, with a label inside.
    fn ring() -> (Buffer, (f64, f64)) {
        let area = Rect::new(0, 0, 60, 21);
        let mut frame = Buffer::empty(area);
        let center = (60.0, 42.0);
        let style = Style::new()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD);
        for step in 0..720 {
            let theta = f64::from(step) * std::f64::consts::TAU / 720.0;
            let (x, y) = (center.0 + 36.0 * theta.cos(), center.1 + 36.0 * theta.sin());
            let (column, row) = ((x / 2.0) as u16, (y / 4.0) as u16);
            let bit = BITS[(y as usize) % 4][(x as usize) % 2];
            let cell = &mut frame[(column, row)];
            let mask = cell
                .symbol()
                .chars()
                .next()
                .filter(|c| ('\u{2800}'..='\u{28ff}').contains(c))
                .map_or(0, |c| (u32::from(c) - 0x2800) as u8);
            cell.set_char(char::from_u32(0x2800 + u32::from(mask | (1 << bit))).unwrap_or(' '))
                .set_style(style);
        }
        let text = Style::new().fg(Color::Cyan);
        frame.set_stringn(24, 10, "咏唱", 4, text);
        frame.set_stringn(29, 10, "prompt", 6, text);
        (frame, center)
    }

    fn faded(frame: &Buffer, center: (f64, f64), progress: f64, moving: bool) -> Buffer {
        let mut buf = Buffer::empty(frame.area);
        draw(frame, &mut buf, center, progress, moving);
        buf
    }

    fn reach(points: &[(f64, f64)], center: (f64, f64)) -> f64 {
        points
            .iter()
            .map(|(x, y)| (x - center.0).hypot(y - center.1))
            .fold(0.0, f64::max)
    }

    #[test]
    fn the_fade_starts_from_the_frame_itself() {
        let (frame, center) = ring();
        assert_eq!(faded(&frame, center, 0.0, true), frame);
        assert_eq!(faded(&frame, center, 0.0, false), frame);
    }

    #[test]
    fn the_dust_dims_spreads_and_thins_to_nothing() {
        let (frame, center) = ring();
        let before = dots(&frame);
        let middle = faded(&frame, center, 0.6, true);
        let after = dots(&middle);
        assert!(
            after.len() * 2 < before.len(),
            "{} of {} dots",
            after.len(),
            before.len()
        );
        assert!(
            reach(&after, center) > reach(&before, center) + 8.0,
            "the dust spreads out"
        );
        for cell in middle.content() {
            if cell.symbol().trim().is_empty() {
                continue;
            }
            assert!(cell.modifier.contains(Modifier::DIM), "{cell:?}");
            assert!(!cell.modifier.contains(Modifier::BOLD), "{cell:?}");
            assert_ne!(cell.fg, Color::LightMagenta, "bright colours dim: {cell:?}");
        }
        let text: String = middle.content().iter().map(|cell| cell.symbol()).collect();
        assert!(!text.contains("prompt"), "text crumbles: {text}");
        let late = faded(&frame, center, 0.97, true);
        assert!(dots(&late).len() * 20 < before.len());
        assert_eq!(faded(&frame, center, 1.0, true), Buffer::empty(frame.area));
    }

    #[test]
    fn without_motion_the_frame_only_dims() {
        let (frame, center) = ring();
        let still = faded(&frame, center, 0.6, false);
        assert_eq!(dots(&still), dots(&frame));
        let text: String = still.content().iter().map(|cell| cell.symbol()).collect();
        assert!(
            text.contains('咏') && text.contains('唱') && text.contains("prompt"),
            "{text}"
        );
        assert!(
            still.content().iter().all(
                |cell| cell.symbol().trim().is_empty() || cell.modifier.contains(Modifier::DIM)
            )
        );
        assert_eq!(faded(&frame, center, 1.0, false), Buffer::empty(frame.area));
    }
}
