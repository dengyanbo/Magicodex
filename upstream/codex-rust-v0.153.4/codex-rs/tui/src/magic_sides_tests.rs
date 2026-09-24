use super::*;
use crate::magic_circle::MagicScene;
use crate::magic_circle::MagicView;

/// A circle that charged fully before its first reply, and the moment `age` after its prompt.
fn charged(age: f64) -> (MagicCircle, Instant) {
    let start = Instant::now();
    let mut circle = MagicCircle::default();
    circle.submit("Magicodex 法阵两侧：魔导书、法阵柱与使魔", start);
    circle.complete_reply(
        "先检查渲染循环，再展开法阵",
        start + Duration::from_secs(20),
    );
    (circle, start + Duration::from_secs_f64(age))
}

fn cast(circle: &mut MagicCircle) {
    let chronicle = &mut circle.chronicle;
    chronicle.cast("1", "glob", "*.md", false, false);
    chronicle.resolve("1", true);
    chronicle.cast("2", "view", "app.rs", false, false);
    chronicle.resolve("2", false);
    chronicle.cast("3", "powershell", "cargo test", false, false);
    chronicle.oracle();
}

fn circle_only(
    circle: &MagicCircle,
    style: MagicStyle,
    area: Rect,
    now: Instant,
    outlet: bool,
    animations: bool,
) -> Buffer {
    let mut buf = Buffer::empty(area);
    let scene = if outlet {
        MagicScene::Outlet
    } else {
        MagicScene::Live
    };
    MagicView {
        circle,
        style,
        animations_enabled: animations,
        scene,
    }
    .render_at(area, &mut buf, now);
    buf
}

fn with_sides(
    circle: &MagicCircle,
    style: MagicStyle,
    area: Rect,
    now: Instant,
    animations: bool,
    outlet: Option<Outlet>,
) -> Buffer {
    let mut buf = circle_only(circle, style, area, now, outlet.is_some(), animations);
    SideView {
        circle,
        style,
        animations,
        outlet,
    }
    .render(area, &mut buf, now);
    buf
}

fn text(buf: &Buffer, columns: std::ops::Range<u16>) -> String {
    let mut text = String::new();
    for y in 0..buf.area.height {
        let mut skip = 0;
        for x in columns.clone() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let symbol = buf[(x, y)].symbol();
            text.push_str(symbol);
            skip = symbol.width().saturating_sub(1);
        }
        text.push('\n');
    }
    text
}

#[test]
fn every_style_stays_inside_the_footprint() {
    let center = 59;
    for style in MagicStyle::ALL {
        for age in [21.0, 25.3, 33.7] {
            for (height, outlet) in [(21, false), (24, true)] {
                let (circle, now) = charged(age);
                let buf = circle_only(
                    &circle,
                    style,
                    Rect::new(0, 0, 120, height),
                    now,
                    outlet,
                    true,
                );
                for y in 0..height {
                    for x in 0..120 {
                        if buf[(x, y)].symbol() != " " {
                            assert!(
                                x.abs_diff(center) <= FOOTPRINT,
                                "{} at age {age} draws column {x}",
                                style.id()
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_width_decides_what_fits() {
    let tier = |width| Layout::of(Rect::new(0, 0, width, 21), false).map(|layout| layout.tier);
    assert_eq!(tier(60), None);
    assert_eq!(tier(80), Some(Tier::Decor));
    assert_eq!(tier(100), Some(Tier::Compact));
    assert_eq!(tier(120), Some(Tier::Full));
    assert_eq!(Layout::of(Rect::new(0, 0, 120, MIN_ROWS - 1), false), None);
    let layout = Layout::of(Rect::new(0, 0, 120, 21), false).unwrap();
    assert_eq!(layout.left.right(), 59 - FOOTPRINT - GAP);
    assert_eq!(layout.right.x, 59 + FOOTPRINT + GAP + 1);
}

#[test]
fn the_sides_leave_the_circle_alone() {
    let area = Rect::new(0, 0, 120, 21);
    for style in MagicStyle::ALL {
        let (mut circle, now) = charged(24.0);
        cast(&mut circle);
        let plain = circle_only(&circle, style, area, now, false, true);
        let full = with_sides(&circle, style, area, now, true, None);
        for y in 0..area.height {
            for x in 59 - FOOTPRINT - GAP..=59 + FOOTPRINT + GAP {
                assert_eq!(
                    full[(x, y)],
                    plain[(x, y)],
                    "{} cell ({x}, {y})",
                    style.id()
                );
            }
        }
        for side in [0..35, 84..120] {
            assert!(
                text(&full, side)
                    .chars()
                    .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
                "{} art",
                style.id()
            );
        }
    }
}

#[test]
fn the_pages_tell_the_turn() {
    let (mut circle, now) = charged(24.0);
    cast(&mut circle);
    let full = with_sides(
        &circle,
        MagicStyle::Fire,
        Rect::new(0, 0, 120, 21),
        now,
        true,
        None,
    );
    let (left, right) = (text(&full, 0..35), text(&full, 84..120));
    for expected in [
        "咏唱记录",
        "寻踪术",
        "*.md",
        "√",
        "×",
        "召唤仪式",
        "cargo test",
    ] {
        assert!(left.contains(expected), "{expected} missing:\n{left}");
    }
    for expected in [
        "施法状态",
        "T+24.0s",
        "满盈",
        "法术 3 · 使魔 0 · 神谕 1",
        "( °o° )",
        "使魔 · 跑腿 召唤仪式",
    ] {
        assert!(right.contains(expected), "{expected} missing:\n{right}");
    }
    let compact = with_sides(
        &circle,
        MagicStyle::Fire,
        Rect::new(0, 0, 100, 21),
        now,
        true,
        None,
    );
    let (left, right) = (text(&compact, 0..25), text(&compact, 74..100));
    assert!(left.contains("寻踪术") && !left.contains("*.md"), "{left}");
    assert!(
        right.contains("阶段 · 满盈") && !right.contains("使魔 ·"),
        "{right}"
    );
    let narrow = with_sides(
        &circle,
        MagicStyle::Fire,
        Rect::new(0, 0, 80, 21),
        now,
        true,
        None,
    );
    assert!(
        !text(&narrow, 0..80).contains("寻踪术"),
        "only art fits in 80 columns"
    );
    let settled = Some(Outlet {
        took: Duration::from_secs(23),
        progress: 0.1,
    });
    let outlet = with_sides(
        &circle,
        MagicStyle::Fire,
        Rect::new(0, 0, 120, 24),
        now,
        true,
        settled,
    );
    let right = text(&outlet, 84..120);
    assert!(
        right.contains("神谕降临 · 用时 23.0s") && right.contains("使魔 · 欢呼！"),
        "{right}"
    );
}

#[test]
fn without_animations_the_sides_stand_still() {
    // Only the sides: the tech circle shows a real clock even without animations.
    let area = Rect::new(0, 0, 80, 21);
    let sides = |buf: &Buffer| -> Vec<ratatui::buffer::Cell> {
        let band = 39 - FOOTPRINT - GAP..=39 + FOOTPRINT + GAP;
        (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| (x, y)))
            .filter(|(x, _)| !band.contains(x))
            .map(|position| buf[position].clone())
            .collect()
    };
    for style in MagicStyle::ALL {
        let (circle, now) = charged(24.0);
        let later = now + Duration::from_millis(1700);
        let still = sides(&with_sides(&circle, style, area, now, false, None));
        assert_eq!(
            still,
            sides(&with_sides(&circle, style, area, later, false, None)),
            "{}",
            style.id()
        );
        let moving = sides(&with_sides(&circle, style, area, now, true, None));
        assert_ne!(
            moving,
            sides(&with_sides(&circle, style, area, later, true, None)),
            "{} moves",
            style.id()
        );
    }
}
