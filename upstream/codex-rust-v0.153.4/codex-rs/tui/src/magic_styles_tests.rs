use super::*;
use crate::magic_circle::CIRCLE_ROWS;
use crate::magic_circle::MagicCircle;
use crate::magic_circle::MagicScene;
use crate::magic_circle::MagicView;
use crate::magic_circle::OUTLET_ROWS;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::time::Duration;
use std::time::Instant;

const PROMPT: &str = "Create a small constellation";
const REPLY: &str = "Reading the files";
const WIDTH: u16 = 80;

fn charged() -> (MagicCircle, Instant, Instant) {
    let start = Instant::now();
    let replied = start + Duration::from_secs(/*secs*/ 16);
    let mut circle = MagicCircle::default();
    circle.submit(PROMPT, start);
    circle.reply_delta(REPLY, replied);
    (circle, start, replied)
}

fn render(
    style: MagicStyle,
    circle: &MagicCircle,
    scene: MagicScene,
    now: Instant,
    animations_enabled: bool,
) -> Buffer {
    let height = match scene {
        MagicScene::Live if !circle.is_active() => 5,
        MagicScene::Live => CIRCLE_ROWS,
        MagicScene::Outlet => CIRCLE_ROWS + OUTLET_ROWS,
    };
    let area = Rect::new(/*x*/ 0, /*y*/ 0, WIDTH, height);
    let mut buffer = Buffer::empty(area);
    MagicView {
        circle,
        style,
        animations_enabled,
        scene,
    }
    .render_at(area, &mut buffer, now);
    buffer
}

fn is_braille(symbol: &str) -> bool {
    symbol
        .chars()
        .all(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
}

fn text(buffer: &Buffer) -> String {
    buffer
        .content()
        .chunks(usize::from(buffer.area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strokes(buffer: &Buffer) -> Vec<&str> {
    buffer
        .content()
        .iter()
        .map(|cell| {
            if is_braille(cell.symbol()) {
                cell.symbol()
            } else {
                ""
            }
        })
        .collect()
}

fn silhouette(buffer: &Buffer) -> HashSet<usize> {
    buffer
        .content()
        .iter()
        .enumerate()
        .filter(|(_, cell)| !cell.symbol().trim().is_empty())
        .map(|(index, _)| index)
        .collect()
}

/// Share of drawn cells that only one of two drawings uses; colour plays no part.
fn distance(a: &HashSet<usize>, b: &HashSet<usize>) -> f64 {
    let union = a.union(b).count().max(1);
    1.0 - a.intersection(b).count() as f64 / union as f64
}

/// Every scene of every style: the idle emblem, the charged circle with a reply, and the outlet.
fn scenes() -> Vec<(&'static str, Vec<Buffer>)> {
    let (circle, _, replied) = charged();
    let idle = MagicCircle::default();
    let draw = |circle: &MagicCircle, scene| {
        MagicStyle::ALL
            .into_iter()
            .map(|style| {
                render(
                    style, circle, scene, replied, /*animations_enabled*/ true,
                )
            })
            .collect()
    };
    vec![
        ("idle", draw(&idle, MagicScene::Live)),
        ("charged", draw(&circle, MagicScene::Live)),
        ("outlet", draw(&circle, MagicScene::Outlet)),
    ]
}

#[test]
fn styles_differ_in_form_not_only_in_colour() {
    let mut report = String::new();
    let mut closest = Vec::new();
    for (scene, buffers) in scenes() {
        let shapes: Vec<HashSet<usize>> = buffers.iter().map(silhouette).collect();
        let mut nearest = (f64::MAX, "", "");
        let _ = writeln!(report, "{scene}");
        for (row, style) in MagicStyle::ALL.into_iter().enumerate() {
            let _ = write!(report, "{:>8}", style.id());
            for (column, other) in MagicStyle::ALL.into_iter().enumerate() {
                let apart = distance(&shapes[row], &shapes[column]);
                let _ = write!(report, " {apart:.2}");
                if column > row {
                    assert_ne!(
                        text(&buffers[row]),
                        text(&buffers[column]),
                        "{style:?} and {other:?} draw the same {scene}"
                    );
                    if apart < nearest.0 {
                        nearest = (apart, style.id(), other.id());
                    }
                }
            }
            report.push('\n');
        }
        let _ = writeln!(
            report,
            "closest: {} / {} {:.2}\n",
            nearest.1, nearest.2, nearest.0
        );
        closest.push((scene, nearest.0));
    }
    insta::assert_snapshot!("magic_style_distances", report);
    for (scene, nearest) in closest {
        // Idle emblems are nine cells wide, so even unrelated shapes share their centre cells.
        let floor = if scene == "idle" { 0.25 } else { 0.3 };
        assert!(
            nearest >= floor,
            "two {scene} drawings share most cells:\n{report}"
        );
    }
}

#[test]
fn every_style_moves_while_charging() {
    let (circle, _, replied) = charged();
    for style in MagicStyle::ALL {
        let now = render(
            style,
            &circle,
            MagicScene::Live,
            replied,
            /*animations_enabled*/ true,
        );
        let later = render(
            style,
            &circle,
            MagicScene::Live,
            replied + Duration::from_millis(/*millis*/ 1_300),
            /*animations_enabled*/ true,
        );
        assert_ne!(strokes(&now), strokes(&later), "{style:?} stands still");
    }
}

#[test]
fn reduced_motion_freezes_every_style() {
    let (circle, _, replied) = charged();
    for style in MagicStyle::ALL {
        let now = render(
            style,
            &circle,
            MagicScene::Live,
            replied,
            /*animations_enabled*/ false,
        );
        let later = render(
            style,
            &circle,
            MagicScene::Live,
            replied + Duration::from_millis(/*millis*/ 31_300),
            /*animations_enabled*/ false,
        );
        assert_eq!(strokes(&now), strokes(&later), "{style:?} moves");
        if style != MagicStyle::Tech {
            assert_eq!(text(&now), text(&later), "{style:?} moves");
        }
    }
}

#[test]
fn every_style_inscribes_the_prompt_and_the_reply() {
    let (circle, _, replied) = charged();
    for style in MagicStyle::ALL {
        let rendered = text(&render(
            style,
            &circle,
            MagicScene::Live,
            replied,
            /*animations_enabled*/ true,
        ));
        let glyphs: HashSet<char> = rendered.chars().collect();
        for glyph in PROMPT.chars().chain(REPLY.chars()) {
            assert!(
                glyph == ' ' || glyphs.contains(&glyph),
                "{style:?} lost {glyph:?}:\n{rendered}"
            );
        }
    }
}

#[test]
fn every_outlet_pours_from_the_centre_into_the_answer() {
    let (circle, _, replied) = charged();
    for style in MagicStyle::ALL {
        let buffer = render(
            style,
            &circle,
            MagicScene::Outlet,
            replied,
            /*animations_enabled*/ true,
        );
        let bottom = buffer.area.height - 1;
        let centre = (WIDTH - 1) / 2;
        assert!(
            (centre - 4..=centre + 4).any(|column| is_braille(buffer[(column, bottom)].symbol())),
            "{style:?} does not reach the answer:\n{}",
            text(&buffer)
        );
    }
}

#[test]
fn tech_readouts_report_real_time_and_reply_state() {
    let start = Instant::now();
    let mut circle = MagicCircle::default();
    circle.submit(PROMPT, start);
    let waiting = text(&render(
        MagicStyle::Tech,
        &circle,
        MagicScene::Live,
        start + Duration::from_millis(/*millis*/ 8_400),
        /*animations_enabled*/ true,
    ));
    assert!(
        waiting.contains("T+08.4s") && waiting.contains("WAIT"),
        "{waiting}"
    );
    circle.reply_delta(REPLY, start + Duration::from_secs(/*secs*/ 16));
    let replied = text(&render(
        MagicStyle::Tech,
        &circle,
        MagicScene::Live,
        start + Duration::from_millis(/*millis*/ 123_460),
        /*animations_enabled*/ false,
    ));
    assert!(
        replied.contains("T+123.5s") && replied.contains("RECV"),
        "{replied}"
    );
}

#[test]
fn style_gallery_snapshots() {
    let (circle, start, replied) = charged();
    for style in MagicStyle::ALL {
        let idle = text(&render(
            style,
            &MagicCircle::default(),
            MagicScene::Live,
            start,
            /*animations_enabled*/ true,
        ));
        let mut waiting = MagicCircle::default();
        waiting.submit(PROMPT, start);
        let charging = text(&render(
            style,
            &waiting,
            MagicScene::Live,
            start + Duration::from_secs(/*secs*/ 6),
            /*animations_enabled*/ true,
        ));
        let charged = text(&render(
            style,
            &circle,
            MagicScene::Live,
            replied,
            /*animations_enabled*/ true,
        ));
        let outlet = text(&render(
            style,
            &circle,
            MagicScene::Outlet,
            replied,
            /*animations_enabled*/ true,
        ));
        insta::assert_snapshot!(
            format!("magic_style_{}", style.id()),
            format!(
                "idle\n{idle}\n\ncharging 6s\n{charging}\n\nreply at 16s\n{charged}\n\noutlet\n{outlet}"
            )
        );
    }
}
