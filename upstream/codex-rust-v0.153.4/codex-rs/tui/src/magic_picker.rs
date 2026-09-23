//! The `/magic list` picker: every style, with a preview of the highlighted one.
//!
//! Moving the highlight previews the style everywhere the circle is shown; Enter keeps it and
//! turns the circle on, Esc restores the previous style.

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
    let items = MagicStyle::ALL
        .into_iter()
        .map(|style| SelectionItem {
            name: style.label(),
            description: Some(style.description().to_string()),
            is_current: style == original,
            dismiss_on_select: true,
            search_value: Some(style.label()),
            actions: vec![Box::new(move |tx: &AppEventSender| {
                tx.send(AppEvent::MagicStyleSelected(style));
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
        initial_selected_idx: MagicStyle::ALL.iter().position(|style| *style == original),
        side_content: Box::new(StylePreview {
            settings: settings.clone(),
            circle,
            at: charged,
        }),
        side_content_width: SideContentWidth::Half,
        side_content_min_width: PREVIEW_MIN_WIDTH,
        stacked_side_content: Some(Box::new(())),
        on_selection_changed: Some(Box::new(move |index, tx: &AppEventSender| {
            if let Some(style) = MagicStyle::ALL.get(index) {
                preview.set_style(*style);
                tx.send(AppEvent::MagicStylePreviewed);
            }
        })),
        on_cancel: Some(Box::new(move |tx: &AppEventSender| {
            restore.set_style(original);
            tx.send(AppEvent::MagicStylePreviewed);
        })),
        ..Default::default()
    }
}
