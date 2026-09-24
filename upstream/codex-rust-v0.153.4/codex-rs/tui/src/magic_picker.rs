//! The `/magic list` picker: every style and the random choice, with a preview of the
//! highlighted one.
//!
//! Moving the highlight previews the style everywhere the circle is shown (the random choice
//! previews a fresh draw); Enter keeps it and turns the circle on, Esc restores the previous
//! style.

use std::time::Duration;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::SideContentWidth;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::magic_circle::MagicCircle;
use crate::magic_circle::MagicScene;
use crate::magic_circle::MagicView;
use crate::magic_style::MagicChoice;
use crate::magic_style::MagicSettings;
use crate::magic_style::MagicStyle;
use crate::render::renderable::Renderable;

const PREVIEW_MIN_WIDTH: u16 = 36;
pub(crate) const VIEW_ID: &str = "magic-style-picker";

/// A fully charged, settled circle in whichever style is highlighted.
struct StylePreview {
    settings: MagicSettings,
    circle: MagicCircle,
    at: Instant,
}

impl Renderable for StylePreview {
    fn desired_height(&self, _width: u16) -> u16 {
        u16::MAX
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        MagicView {
            circle: &self.circle,
            style: self.settings.style(),
            animations_enabled: false,
            scene: MagicScene::Live,
        }
        .render_at(area, buf, self.at);
    }
}

pub(crate) fn picker_params(settings: &MagicSettings) -> SelectionViewParams {
    let original = settings.style();
    let original_random = settings.is_random();
    let choices = MagicStyle::ALL
        .into_iter()
        .map(MagicChoice::Style)
        .chain([MagicChoice::Random]);
    let items = choices
        .map(|choice| SelectionItem {
            name: choice.label(),
            description: Some(
                match choice {
                    MagicChoice::Style(style) => style.description(),
                    MagicChoice::Random => MagicChoice::RANDOM_DESCRIPTION,
                }
                .to_string(),
            ),
            is_current: choice == settings.choice(),
            dismiss_on_select: true,
            search_value: Some(choice.label()),
            actions: vec![Box::new(move |tx: &AppEventSender| {
                tx.send(AppEvent::MagicStyleSelected(choice));
            })],
            ..Default::default()
        })
        .collect();
    let start = Instant::now();
    let charged = start + Duration::from_secs(/*secs*/ 12);
    let mut circle = MagicCircle::default();
    circle.submit("Magicodex 法阵预览", start);
    circle.reply_delta("选择样式后立即生效", charged);
    let preview = settings.clone();
    let restore = settings.clone();
    SelectionViewParams {
        view_id: Some(VIEW_ID),
        title: Some("Magic circle styles".to_string()),
        subtitle: Some("移动即预览 · Enter 选用并开启 · Esc 恢复原样式".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        initial_selected_idx: Some(if original_random {
            MagicStyle::ALL.len()
        } else {
            MagicStyle::ALL
                .iter()
                .position(|style| *style == original)
                .unwrap_or(0)
        }),
        side_content: Box::new(StylePreview {
            settings: settings.clone(),
            circle,
            at: charged,
        }),
        side_content_width: SideContentWidth::Half,
        side_content_min_width: PREVIEW_MIN_WIDTH,
        stacked_side_content: Some(Box::new(())),
        on_selection_changed: Some(Box::new(move |index, tx: &AppEventSender| {
            match MagicStyle::ALL.get(index) {
                Some(style) => preview.set_style(*style),
                // The random choice previews what it would draw.
                None => preview.preview_random(),
            }
            tx.send(AppEvent::MagicStylePreviewed);
        })),
        on_cancel: Some(Box::new(move |tx: &AppEventSender| {
            restore.restore(original, original_random);
            tx.send(AppEvent::MagicStylePreviewed);
        })),
        ..Default::default()
    }
}
