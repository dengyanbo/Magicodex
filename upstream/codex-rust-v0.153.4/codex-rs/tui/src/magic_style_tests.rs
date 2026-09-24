use super::*;
use pretty_assertions::assert_eq;
use std::collections::HashSet;

#[test]
fn styles_parse_from_ids_in_any_case_and_chinese_names() {
    for style in MagicStyle::ALL {
        assert_eq!(MagicStyle::parse(style.id()), Some(style));
        assert_eq!(MagicStyle::parse(&style.id().to_uppercase()), Some(style));
        assert_eq!(MagicStyle::parse(&format!(" {} ", style.id())), Some(style));
        assert_eq!(MagicStyle::parse(style.name()), Some(style));
    }
    for other in ["", "on", "off", "list", "ice", "fire 火"] {
        assert_eq!(MagicStyle::parse(other), None, "{other:?}");
    }
}

#[test]
fn every_style_has_a_unique_id_name_and_stored_value() {
    let ids: HashSet<_> = MagicStyle::ALL.iter().map(|style| style.id()).collect();
    let names: HashSet<_> = MagicStyle::ALL.iter().map(|style| style.name()).collect();
    assert_eq!((ids.len(), names.len()), (10, 10));
    for (index, style) in MagicStyle::ALL.into_iter().enumerate() {
        assert_eq!(style as usize, index);
        assert!(!style.description().is_empty());
    }
    assert_eq!(MagicStyle::default(), MagicStyle::Classic);
    assert_eq!(MagicStyle::Fire.label(), "fire 火");
}

#[test]
fn every_style_has_its_own_palette() {
    for (index, style) in MagicStyle::ALL.into_iter().enumerate() {
        for other in &MagicStyle::ALL[index + 1..] {
            assert_ne!(style.palette(), other.palette(), "{style:?} and {other:?}");
        }
    }
}

#[test]
fn settings_are_shared_by_every_clone() {
    let settings = MagicSettings::default();
    let widget = settings.clone();
    assert!(!widget.enabled());
    assert_eq!(widget.style(), MagicStyle::Classic);
    settings.set_enabled(/*enabled*/ true);
    settings.set_style(MagicStyle::Eerie);
    assert!(widget.enabled());
    assert_eq!(widget.style(), MagicStyle::Eerie);
    for style in MagicStyle::ALL {
        widget.set_style(style);
        assert_eq!(settings.style(), style);
    }
}

#[test]
fn the_random_choice_parses_and_draws_another_style() {
    for text in ["random", "RANDOM", " Random ", "随机"] {
        assert_eq!(
            MagicChoice::parse(text),
            Some(MagicChoice::Random),
            "{text:?}"
        );
    }
    assert_eq!(
        MagicChoice::parse("火"),
        Some(MagicChoice::Style(MagicStyle::Fire))
    );
    assert_eq!(MagicChoice::parse("rand"), None);
    assert_eq!(MagicChoice::Random.label(), "random 随机");
    for style in MagicStyle::ALL {
        let others: HashSet<_> = (0..9).map(|roll| style.other(roll) as u8).collect();
        assert_eq!(others.len(), 9, "{style:?} reaches every other style");
        assert!(
            !others.contains(&(style as u8)),
            "{style:?} never draws itself"
        );
    }
}

#[test]
fn a_random_choice_is_shared_rerolled_and_restored() {
    let settings = MagicSettings::default();
    let widget = settings.clone();
    settings.set_choice(MagicChoice::Random);
    assert_eq!(widget.choice(), MagicChoice::Random);
    let drawn = widget.style();
    assert_ne!(
        drawn,
        MagicStyle::Classic,
        "turning random on draws another style"
    );
    settings.set_choice(MagicChoice::Random);
    assert_eq!(
        widget.style(),
        drawn,
        "choosing random again keeps the draw"
    );
    widget.reroll();
    assert_ne!(settings.style(), drawn);
    settings.restore(MagicStyle::Holy, /*random*/ false);
    assert_eq!(widget.choice(), MagicChoice::Style(MagicStyle::Holy));
    widget.reroll();
    assert_eq!(
        settings.style(),
        MagicStyle::Holy,
        "a chosen style never rerolls"
    );
    widget.preview_random();
    assert!(settings.is_random() && settings.style() != MagicStyle::Holy);
    settings.restore(MagicStyle::Holy, /*random*/ true);
    assert_eq!(
        (widget.choice(), widget.style()),
        (MagicChoice::Random, MagicStyle::Holy)
    );
}
