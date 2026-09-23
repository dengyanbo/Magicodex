//! Selectable magic circle styles, their palettes, and the display settings shared by the chat
//! widgets of one app run.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

use ratatui::style::Color;
use ratatui::style::Style;

use crate::magic_canvas::Ink;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MagicStyle {
    #[default]
    Classic,
    Wind,
    Fire,
    Water,
    Thunder,
    Earth,
    Holy,
    Dark,
    Eerie,
    Tech,
}

impl MagicStyle {
    /// Every style in picker order; the index of a style here is its stored value.
    pub(crate) const ALL: [Self; 10] = [
        Self::Classic,
        Self::Wind,
        Self::Fire,
        Self::Water,
        Self::Thunder,
        Self::Earth,
        Self::Holy,
        Self::Dark,
        Self::Eerie,
        Self::Tech,
    ];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Wind => "wind",
            Self::Fire => "fire",
            Self::Water => "water",
            Self::Thunder => "thunder",
            Self::Earth => "earth",
            Self::Holy => "holy",
            Self::Dark => "dark",
            Self::Eerie => "eerie",
            Self::Tech => "tech",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Classic => "经典",
            Self::Wind => "风",
            Self::Fire => "火",
            Self::Water => "水",
            Self::Thunder => "雷",
            Self::Earth => "土",
            Self::Holy => "神圣",
            Self::Dark => "黑暗",
            Self::Eerie => "诡异",
            Self::Tech => "科技",
        }
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Classic => "六芒星与双文字带，prompt 绕外圈，回复绕内圈",
            Self::Wind => "螺旋气旋疾转，文字沿涡旋卷入阵心",
            Self::Fire => "外缘火舌跃动，五芒星与上升火星，文字随热浪闪烁",
            Self::Water => "波浪外缘与池心涟漪，文字随水波起伏",
            Self::Thunder => "锯齿八边形，闪电劈向阵心，文字沿棱边排列",
            Self::Earth => "方形石印，文字刻于四边，坤卦纹与角石，逐格转动",
            Self::Holy => "放射圣光与光环，八芒星，文字疏朗",
            Self::Dark => "深渊漩涡与血色蚀环，回复螺旋坠入虚空",
            Self::Eerie => "蠕动的环与会眨的邪眼，缝线与故障错位",
            Self::Tech => "分段刻度环与雷达扫描，显示真实计时与状态",
        }
    }

    pub(crate) fn label(self) -> String {
        format!("{} {}", self.id(), self.name())
    }

    /// Matches a style id (any case) or its Chinese name.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        Self::ALL
            .into_iter()
            .find(|style| style.id().eq_ignore_ascii_case(text) || style.name() == text)
    }

    /// Elemental styles are an explicit opt-in, so they may use ANSI colours beyond Codex's
    /// magenta and cyan. They stay on the 16 ANSI colours so terminal themes still choose the
    /// shades; the default classic style keeps the Codex palette.
    pub(crate) fn palette(self) -> Palette {
        let plain = Style::default();
        let fg = |color| plain.fg(color);
        match self {
            Self::Classic => Palette {
                faint: fg(Color::Magenta).dim(),
                line: fg(Color::Magenta),
                bright: fg(Color::Magenta).bold(),
                accent: fg(Color::Cyan),
                glint: plain.bold(),
                prompt: fg(Color::Cyan),
                reply: plain,
                glow: [fg(Color::Magenta), fg(Color::Magenta).bold()],
            },
            Self::Wind => Palette {
                faint: fg(Color::Cyan).dim(),
                line: fg(Color::Cyan),
                bright: fg(Color::LightCyan),
                accent: plain,
                glint: plain.bold(),
                prompt: plain.italic(),
                reply: fg(Color::Cyan),
                glow: [fg(Color::Cyan), plain.bold()],
            },
            Self::Fire => Palette {
                faint: fg(Color::Red).dim(),
                line: fg(Color::Red),
                bright: fg(Color::LightRed),
                accent: fg(Color::LightYellow),
                glint: fg(Color::LightYellow).bold(),
                prompt: fg(Color::Yellow),
                reply: plain,
                glow: [fg(Color::Red), fg(Color::LightYellow).bold()],
            },
            Self::Water => Palette {
                faint: fg(Color::Blue),
                line: fg(Color::LightBlue),
                bright: fg(Color::LightCyan),
                accent: fg(Color::Cyan),
                glint: plain.bold(),
                prompt: fg(Color::LightCyan),
                reply: plain,
                glow: [fg(Color::LightBlue), fg(Color::LightCyan).bold()],
            },
            Self::Thunder => Palette {
                faint: plain.dim(),
                line: fg(Color::LightBlue),
                bright: fg(Color::Blue),
                accent: fg(Color::LightYellow),
                glint: plain.bold(),
                prompt: fg(Color::LightYellow),
                reply: plain,
                glow: [fg(Color::LightBlue), fg(Color::LightYellow).bold()],
            },
            Self::Earth => Palette {
                faint: plain.dim(),
                line: fg(Color::Yellow),
                bright: fg(Color::Yellow).bold(),
                accent: fg(Color::Green),
                glint: fg(Color::LightGreen),
                prompt: fg(Color::Green),
                reply: plain,
                glow: [fg(Color::Yellow), fg(Color::LightGreen).bold()],
            },
            Self::Holy => Palette {
                faint: plain.dim(),
                line: plain,
                bright: plain.bold(),
                accent: fg(Color::LightYellow),
                glint: fg(Color::LightYellow).bold(),
                prompt: fg(Color::Yellow),
                reply: plain,
                glow: [fg(Color::Yellow), fg(Color::LightYellow).bold()],
            },
            Self::Dark => Palette {
                faint: plain.dim(),
                line: fg(Color::Magenta).dim(),
                bright: fg(Color::Magenta),
                accent: fg(Color::Red),
                glint: fg(Color::LightRed),
                prompt: fg(Color::Magenta),
                reply: plain.dim(),
                glow: [fg(Color::Magenta), fg(Color::LightRed).bold()],
            },
            Self::Eerie => Palette {
                faint: fg(Color::Green).dim(),
                line: fg(Color::Green),
                bright: fg(Color::LightGreen),
                accent: fg(Color::Magenta),
                glint: fg(Color::LightRed),
                prompt: fg(Color::LightGreen),
                reply: plain,
                glow: [fg(Color::Green), fg(Color::LightGreen).bold()],
            },
            Self::Tech => Palette {
                faint: fg(Color::Cyan).dim(),
                line: fg(Color::Cyan),
                bright: fg(Color::LightCyan),
                accent: fg(Color::Green),
                glint: plain.bold(),
                prompt: fg(Color::LightGreen),
                reply: fg(Color::Cyan),
                glow: [fg(Color::Cyan), fg(Color::LightGreen).bold()],
            },
        }
    }
}

/// How one style paints its inks, its inscriptions, and the glow of answer text as it pours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Palette {
    pub(crate) faint: Style,
    pub(crate) line: Style,
    pub(crate) bright: Style,
    pub(crate) accent: Style,
    pub(crate) glint: Style,
    pub(crate) prompt: Style,
    pub(crate) reply: Style,
    /// Warm then hot styles for the newest provisional answer text.
    pub(crate) glow: [Style; 2],
}

impl Palette {
    pub(crate) fn ink(&self, ink: Ink) -> Style {
        match ink {
            Ink::Faint => self.faint,
            Ink::Line => self.line,
            Ink::Bright => self.bright,
            Ink::Accent => self.accent,
            Ink::Glint => self.glint,
        }
    }
}

/// Magic display settings shared by every chat widget of one app run. Nothing is persisted.
#[derive(Clone, Debug, Default)]
pub(crate) struct MagicSettings {
    enabled: Arc<AtomicBool>,
    style: Arc<AtomicU8>,
}

impl MagicSettings {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn style(&self) -> MagicStyle {
        MagicStyle::ALL
            .get(usize::from(self.style.load(Ordering::Relaxed)))
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn set_style(&self, style: MagicStyle) {
        self.style.store(style as u8, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "magic_style_tests.rs"]
mod tests;
