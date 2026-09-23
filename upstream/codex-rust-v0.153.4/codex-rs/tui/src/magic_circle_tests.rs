use super::*;
use pretty_assertions::assert_eq;

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
    assert!(later.rings > submitted.rings);
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
    assert_eq!(input, original);
    assert!(circle.prompt.graphemes(/*is_extended*/ true).count() <= TEXT_LIMIT);
    assert!(!circle.prompt.contains('\u{1b}'));
}

fn snapshot(circle: &MagicCircle, now: Instant) -> String {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 21,
    );
    let mut buffer = Buffer::empty(area);
    MagicView {
        circle,
        animations_enabled: true,
    }
    .render_at(area, &mut buffer, now);
    buffer
        .content()
        .chunks(/*chunk_size*/ 80)
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

#[test]
fn idle_circle_snapshot() {
    let now = Instant::now();
    let circle = MagicCircle::default();
    insta::assert_snapshot!("magic_idle", snapshot(&circle, now));
}

#[test]
fn submitted_circle_snapshot() {
    let now = Instant::now();
    let mut circle = MagicCircle::default();
    circle.submit("Create a small constellation", now);
    insta::assert_snapshot!("magic_submitted", snapshot(&circle, now));
}

#[test]
fn charging_circle_snapshot() {
    let now = Instant::now();
    let mut circle = MagicCircle::default();
    circle.submit("Create a small constellation", now);
    insta::assert_snapshot!(
        "magic_charging",
        snapshot(&circle, now + Duration::from_secs(/*secs*/ 16))
    );
}

#[test]
fn reply_orbits_snapshot() {
    let now = Instant::now();
    let mut circle = MagicCircle::default();
    circle.submit("Create a small constellation", now);
    circle.reply_delta("Reading the files", now + Duration::from_secs(/*secs*/ 16));
    insta::assert_snapshot!(
        "magic_reply_orbits",
        snapshot(&circle, now + Duration::from_secs(/*secs*/ 16))
    );
}
