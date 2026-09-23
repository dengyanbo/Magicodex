use super::*;
use crate::magic_style::MagicStyle;
use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Modifier;

#[test]
fn grows_until_first_reply_and_returns_to_idle() {
    let now = Instant::now();
    let mut circle = MagicCircle::default();
    let idle = circle.geometry(now);
    circle.submit("prompt", now);
    let submitted = circle.geometry(now);
    let later = circle.geometry(now + Duration::from_secs(/*secs*/ 12));
    assert!(submitted.radius > idle.radius);
    assert!(later.radius > submitted.radius);
    assert!(later.layers > submitted.layers);
    circle.reply_delta("first reply", now + Duration::from_secs(/*secs*/ 12));
    assert_eq!(
        circle.geometry(now + Duration::from_secs(/*secs*/ 120)),
        later
    );
    circle.finish();
    assert_eq!(
        circle.geometry(now + Duration::from_secs(/*secs*/ 121)),
        idle
    );
}

#[test]
fn waiting_is_bounded_and_blank_deltas_do_not_end_growth() {
    let now = Instant::now();
    let mut circle = MagicCircle::default();
    circle.begin(now);
    circle.reply_delta("\n ", now + Duration::from_secs(/*secs*/ 1));
    assert_eq!(
        circle.geometry(now + Duration::from_secs(/*secs*/ 1_000)),
        circle.geometry(now + Duration::from_secs(/*secs*/ 10_000))
    );
    assert!(circle.first_reply.is_none());
}

#[test]
fn display_fragments_are_bounded_and_do_not_change_the_input() {
    let input = format!("\u{1b}[31m{}中文", "long text ".repeat(/*n*/ 500));
    let original = input.clone();
    let mut circle = MagicCircle::default();
    circle.submit(&input, Instant::now());
    circle.reply_delta(&input, Instant::now());
    assert_eq!(input, original);
    assert!(circle.prompt.graphemes(/*is_extended*/ true).count() <= TEXT_LIMIT);
    assert!(circle.reply.graphemes(/*is_extended*/ true).count() <= TEXT_LIMIT);
    assert!(
        circle.reply.ends_with("中文"),
        "replies keep their newest text"
    );
    assert!(!circle.prompt.contains('\u{1b}'));
}

#[test]
fn completed_reply_stays_until_the_next_public_text() {
    let now = Instant::now();
    let mut circle = MagicCircle::default();
    circle.begin(now);
    circle.reply_delta("Reading", now);
    circle.complete_reply("Reading the files", now);
    circle.reply_delta("\n", now);
    assert_eq!(circle.reply, "Reading the files");
    circle.reply_delta("Next", now);
    assert_eq!(circle.reply, "Next");
}

fn render(circle: &MagicCircle, now: Instant, animations_enabled: bool) -> Buffer {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 21,
    );
    let mut buffer = Buffer::empty(area);
    MagicView {
        circle,
        style: MagicStyle::Classic,
        animations_enabled,
        scene: MagicScene::Live,
    }
    .render_at(area, &mut buffer, now);
    buffer
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

fn snapshot(circle: &MagicCircle, now: Instant) -> String {
    text(&render(circle, now, /*animations_enabled*/ true))
}

fn submitted(prompt: &str) -> (MagicCircle, Instant) {
    let now = Instant::now();
    let mut circle = MagicCircle::default();
    circle.submit(prompt, now);
    (circle, now)
}

#[test]
fn idle_circle_snapshot() {
    let now = Instant::now();
    let circle = MagicCircle::default();
    insta::assert_snapshot!("magic_idle", snapshot(&circle, now));
}

#[test]
fn idle_circle_is_mirror_symmetric() {
    // An odd width keeps the centre inside one cell, so the whole row can be mirrored.
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 79, /*height*/ 5,
    );
    let mut buffer = Buffer::empty(area);
    MagicView {
        circle: &MagicCircle::default(),
        style: MagicStyle::Classic,
        animations_enabled: true,
        scene: MagicScene::Live,
    }
    .render_at(area, &mut buffer, Instant::now());
    assert!(text(&buffer).contains('✦'));
    for row in buffer.content().chunks(usize::from(buffer.area.width)) {
        let dots: Vec<[u8; 4]> = row
            .iter()
            .flat_map(|cell| {
                let mask = cell
                    .symbol()
                    .chars()
                    .next()
                    .map(u32::from)
                    .filter(|code| (0x2800..=0x28ff).contains(code))
                    .map_or(0, |code| (code - 0x2800) as u8);
                let column = |bits: [u8; 4]| bits.map(|bit| (mask >> bit) & 1);
                [column([0, 1, 2, 6]), column([3, 4, 5, 7])]
            })
            .collect();
        let mirrored: Vec<[u8; 4]> = dots.iter().rev().copied().collect();
        assert_eq!(dots, mirrored);
    }
}

#[test]
fn inscribing_circle_snapshot() {
    let (circle, now) = submitted("Create a small constellation");
    insta::assert_snapshot!(
        "magic_submitted",
        snapshot(&circle, now + Duration::from_millis(/*millis*/ 700))
    );
}

#[test]
fn charging_circle_snapshot() {
    let (circle, now) = submitted("Create a small constellation");
    insta::assert_snapshot!(
        "magic_charging",
        snapshot(&circle, now + Duration::from_secs(/*secs*/ 16))
    );
}

#[test]
fn reply_orbits_snapshot() {
    let (mut circle, now) = submitted("Create a small constellation");
    circle.reply_delta("Reading the files", now + Duration::from_secs(/*secs*/ 16));
    insta::assert_snapshot!(
        "magic_reply_orbits",
        snapshot(&circle, now + Duration::from_secs(/*secs*/ 16))
    );
}

#[test]
fn long_mixed_prompt_closes_the_band_without_echoing_its_start() {
    let prompt = "帮我把 terminal 渲染改成魔法阵，等待模型时逐渐展开 Arcane circle";
    let (circle, now) = submitted(prompt);
    let rendered = snapshot(&circle, now + Duration::from_secs(/*secs*/ 4));
    assert_eq!(rendered.matches("帮").count(), 1, "{rendered}");
    insta::assert_snapshot!("magic_long_prompt", rendered);
}

#[test]
fn reduced_motion_draws_every_unlocked_layer_at_once() {
    let (circle, now) = submitted("Create a small constellation");
    let buffer = render(&circle, now, /*animations_enabled*/ false);
    let rendered = text(&buffer);
    let strokes = rendered
        .chars()
        .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
        .count();
    assert!(strokes > 40, "rings are drawn without a reveal: {rendered}");
    assert!(
        rendered.contains("Cre"),
        "the prompt is inscribed at once: {rendered}"
    );
}

#[test]
fn layers_use_codex_colours_with_depth() {
    let (mut circle, now) = submitted("Create a small constellation");
    circle.reply_delta("Reading the files", now + Duration::from_secs(/*secs*/ 16));
    let buffer = render(
        &circle,
        now + Duration::from_secs(/*secs*/ 17),
        /*animations_enabled*/ true,
    );
    let inks = |symbol: fn(&str) -> bool| {
        let mut styles: Vec<(Color, Modifier)> = buffer
            .content()
            .iter()
            .filter(|cell| symbol(cell.symbol()))
            .map(|cell| (cell.fg, cell.modifier))
            .collect();
        styles.sort_by_key(|style| format!("{style:?}"));
        styles.dedup();
        styles
    };
    let braille = |symbol: &str| {
        symbol
            .chars()
            .all(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
    };
    let strokes = inks(braille);
    for expected in [
        (Color::Magenta, Modifier::DIM),
        (Color::Magenta, Modifier::empty()),
        (Color::Magenta, Modifier::BOLD),
        (Color::Reset, Modifier::BOLD),
    ] {
        assert!(
            strokes.contains(&expected),
            "missing {expected:?} in {strokes:?}"
        );
    }
    let prompt = inks(|symbol| symbol == "C");
    assert_eq!(prompt, vec![(Color::Cyan, Modifier::empty())]);
}
