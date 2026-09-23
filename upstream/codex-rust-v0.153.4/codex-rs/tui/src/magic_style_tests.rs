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
