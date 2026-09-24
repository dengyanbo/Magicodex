//! What the magic region shows and how tall it is, driven by the child's session events.

use std::time::Duration;
use std::time::Instant;

use crate::circle::sides::Outlet;
use crate::circle::state::CIRCLE_ROWS;
use crate::circle::state::MagicCircle;
use crate::circle::state::OUTLET_ROWS;
use crate::circle::state::USAGE;
use crate::circle::style::MagicStyle;
use crate::session::Event;

/// Rows of the small emblem shown before a prompt.
pub(crate) const IDLE_ROWS: u16 = 5;
/// Copilot keeps at least this many rows for its conversation, input box and footer.
pub(crate) const MIN_CHILD_ROWS: u16 = 14;
/// How long the settled circle's outlet stays after the final answer.
const OUTLET_TIME: Duration = Duration::from_millis(2600);
/// A final answer is final once no new model turn starts within this time.
const FINAL_GRACE: Duration = Duration::from_millis(1200);
const TOAST_TIME: Duration = Duration::from_millis(3500);
const TOO_SHORT: &str = "窗口太矮，放不下样式列表 · 可直接输入 /magic <类型>";
/// Shown while `/magic` is typed: Copilot's command list, which opens at the same time, only
/// lists Copilot's own commands.
const COMMAND_HINT: &str =
    "/magic on|off|list|<类型>\nmagicopilot 的命令，Copilot 列表里没有，回车即可";

/// Whether a terminal `rows` tall has room for the style picker above Copilot.
fn picker_fits(rows: u16) -> bool {
    rows.saturating_sub(MIN_CHILD_ROWS) >= IDLE_ROWS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Idle,
    Charging,
    Outlet { since: Instant },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Picker {
    pub(crate) index: usize,
    original: MagicStyle,
    original_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickerKey {
    Up,
    Down,
    Accept,
    Cancel,
    /// A digit: `1`–`9` pick those items, `0` the tenth.
    Digit(u8),
}

pub(crate) struct Magic {
    pub(crate) enabled: bool,
    pub(crate) style: MagicStyle,
    pub(crate) animations: bool,
    pub(crate) circle: MagicCircle,
    pub(crate) phase: Phase,
    pub(crate) picker: Option<Picker>,
    toast: Option<(String, Instant)>,
    command_hint: bool,
    last_reply: Option<(String, bool)>,
    finish_at: Option<Instant>,
    attention: bool,
    busy: Option<bool>,
}

impl Magic {
    pub(crate) fn new(enabled: bool, style: MagicStyle, animations: bool) -> Self {
        Self {
            enabled,
            style,
            animations,
            circle: MagicCircle::default(),
            phase: Phase::Idle,
            picker: None,
            toast: None,
            command_hint: false,
            last_reply: None,
            finish_at: None,
            attention: false,
            busy: None,
        }
    }

    /// The style drawn right now: the highlighted one while the picker is open.
    pub(crate) fn shown_style(&self) -> MagicStyle {
        self.picker
            .map_or(self.style, |picker| MagicStyle::ALL[picker.index])
    }

    pub(crate) fn toast(&self, now: Instant) -> Option<&str> {
        self.toast
            .as_ref()
            .filter(|(_, until)| now < *until)
            .map(|(text, _)| text.as_str())
    }

    fn say(&mut self, text: String, now: Instant) {
        self.toast = Some((text, now + TOAST_TIME));
    }

    /// Shows the usage of `/magic` while the input box holds the start of it.
    pub(crate) fn set_command_hint(&mut self, typing: bool) {
        self.command_hint = typing;
    }

    /// The text at the top of the region: the command hint while the circle is shown, or else
    /// a toast. Lines after the first explain it.
    pub(crate) fn notice(&self, now: Instant) -> Option<&str> {
        if self.command_hint && self.enabled && self.picker.is_none() {
            return Some(COMMAND_HINT);
        }
        self.toast(now)
    }

    /// The notice shown when the wrapper starts with the circle on.
    pub(crate) fn greet(&mut self, now: Instant) {
        self.say(
            format!(
                "Magic circle on · {} · /magic list 选择样式",
                self.style.label()
            ),
            now,
        );
    }

    /// Rows of the region above the child for a terminal `rows` tall.
    pub(crate) fn region_rows(&self, rows: u16, now: Instant) -> u16 {
        let room = rows.saturating_sub(MIN_CHILD_ROWS);
        let want = if self.picker.is_some() {
            CIRCLE_ROWS
        } else if !self.enabled {
            return u16::from(self.toast(now).is_some() && room >= 1);
        } else {
            match self.phase {
                Phase::Idle => IDLE_ROWS,
                // A waiting permission prompt gets the room back.
                Phase::Charging if self.attention => IDLE_ROWS,
                Phase::Charging => CIRCLE_ROWS,
                Phase::Outlet { .. } => CIRCLE_ROWS + OUTLET_ROWS,
            }
        };
        let rows = want.min(room);
        if rows >= IDLE_ROWS {
            rows
        } else {
            u16::from(self.toast(now).is_some() && room >= 1)
        }
    }

    /// Whether frames must keep coming for motion or timers.
    pub(crate) fn animating(&self, now: Instant) -> bool {
        let moving = self.enabled && self.animations && self.phase == Phase::Charging;
        moving
            || matches!(self.phase, Phase::Outlet { .. })
            || self.finish_at.is_some()
            || self.toast(now).is_some()
    }

    pub(crate) fn handle(&mut self, event: Event, now: Instant) {
        match event {
            Event::Prompt(text) => {
                self.circle.finish();
                self.circle.submit(&text, now);
                self.phase = Phase::Charging;
                self.last_reply = None;
                self.finish_at = None;
                self.attention = false;
            }
            Event::Reply { text, tools } => {
                if self.phase != Phase::Charging {
                    self.circle.finish();
                    self.circle.begin(now);
                    self.phase = Phase::Charging;
                }
                if !text.trim().is_empty() {
                    self.circle.complete_reply(&text, now);
                    self.circle.chronicle.oracle();
                }
                self.last_reply = Some((text, tools));
            }
            Event::TurnStart => self.finish_at = None,
            Event::TurnEnd => {
                if self.final_answer().is_some() {
                    self.finish_at = Some(now + FINAL_GRACE);
                }
            }
            Event::Idle => {
                if self.phase == Phase::Charging {
                    self.finish_at = Some(now);
                }
            }
            Event::Attention(waiting) => self.attention = waiting,
            Event::Aborted | Event::Reset => {
                if self.phase == Phase::Charging {
                    self.last_reply = None;
                    self.finish_at = Some(now);
                }
            }
            Event::Spell {
                id,
                tool,
                detail,
                mcp,
                nested,
            } => {
                if self.phase == Phase::Charging {
                    self.circle.chronicle.cast(&id, &tool, &detail, mcp, nested);
                }
            }
            Event::SpellDone { id, ok } => self.circle.chronicle.resolve(&id, ok),
            Event::Summon { id, name } => {
                if self.phase == Phase::Charging {
                    self.circle.chronicle.summon(&id, &name);
                }
            }
            Event::Tome(name) => {
                if self.phase == Phase::Charging {
                    self.circle.chronicle.tome(&name);
                }
            }
            Event::Intent(intent) => self.circle.chronicle.intend(&intent),
        }
    }

    /// The answer's arrival while its outlet shows, for the sides of the circle.
    pub(crate) fn outlet(&self, now: Instant) -> Option<Outlet> {
        let Phase::Outlet { since } = self.phase else {
            return None;
        };
        let run = now.saturating_duration_since(since).as_secs_f64();
        Some(Outlet {
            took: self.circle.elapsed(since).unwrap_or_default(),
            progress: (run / OUTLET_TIME.as_secs_f64()).min(1.0),
        })
    }

    /// Terminal progress from the child: a busy indicator that clears also ends the turn.
    pub(crate) fn progress(&mut self, busy: Option<bool>, now: Instant) {
        if self.busy == Some(true) && busy == Some(false) && self.phase == Phase::Charging {
            self.finish_at.get_or_insert(now + FINAL_GRACE);
        }
        self.busy = busy;
    }

    fn final_answer(&self) -> Option<&str> {
        match &self.last_reply {
            Some((text, false)) if !text.trim().is_empty() => Some(text),
            _ => None,
        }
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        if let Some(at) = self.finish_at
            && now >= at
        {
            self.finish_at = None;
            if self.phase == Phase::Charging {
                if self.final_answer().is_some() {
                    self.phase = Phase::Outlet { since: now };
                } else {
                    // Interrupted or failed: no casting effect.
                    self.circle.finish();
                    self.phase = Phase::Idle;
                }
            }
        }
        if let Phase::Outlet { since } = self.phase
            && now.duration_since(since) >= OUTLET_TIME
        {
            self.circle.finish();
            self.phase = Phase::Idle;
            self.last_reply = None;
        }
    }

    /// Runs `/magic <args>` in a terminal `rows` tall.
    pub(crate) fn command(&mut self, args: &str, rows: u16, now: Instant) {
        let args = args.trim();
        match args {
            "" | "list" if !picker_fits(rows) => self.say(TOO_SHORT.to_string(), now),
            "" | "list" => {
                self.picker = Some(Picker {
                    index: MagicStyle::ALL
                        .iter()
                        .position(|style| *style == self.style)
                        .unwrap_or(0),
                    original: self.style,
                    original_enabled: self.enabled,
                });
            }
            "on" => {
                self.enabled = true;
                self.say(format!("Magic circle on · {}", self.style.label()), now);
            }
            "off" => {
                self.enabled = false;
                self.say(format!("Magic circle off · {}", self.style.label()), now);
            }
            _ => match MagicStyle::parse(args) {
                Some(style) => {
                    self.style = style;
                    self.enabled = true;
                    self.say(format!("Magic circle on · {}", style.label()), now);
                }
                None => self.say(USAGE.to_string(), now),
            },
        }
    }

    pub(crate) fn picker_key(&mut self, key: PickerKey, now: Instant) {
        let Some(mut picker) = self.picker else {
            return;
        };
        let count = MagicStyle::ALL.len();
        match key {
            PickerKey::Up => picker.index = (picker.index + count - 1) % count,
            PickerKey::Down => picker.index = (picker.index + 1) % count,
            PickerKey::Digit(digit) => {
                let index = if digit == 0 {
                    9
                } else {
                    usize::from(digit) - 1
                };
                if index < count {
                    return self.accept(index, now);
                }
            }
            PickerKey::Accept => return self.accept(picker.index, now),
            PickerKey::Cancel => {
                self.style = picker.original;
                self.enabled = picker.original_enabled;
                self.picker = None;
                return;
            }
        }
        self.picker = Some(picker);
    }

    fn accept(&mut self, index: usize, now: Instant) {
        self.picker = None;
        self.style = MagicStyle::ALL[index];
        self.enabled = true;
        self.say(format!("Magic circle on · {}", self.style.label()), now);
    }

    /// Closes the picker, undoing its preview, when the terminal became too short to show it:
    /// an invisible picker would still take every key.
    pub(crate) fn fit(&mut self, rows: u16, now: Instant) {
        if self.picker.is_some() && !picker_fits(rows) {
            self.picker_key(PickerKey::Cancel, now);
            self.say(TOO_SHORT.to_string(), now);
        }
    }

    /// The next moment something changes without new input.
    pub(crate) fn deadline(&self, now: Instant) -> Option<Instant> {
        let mut next = self.finish_at;
        if let Phase::Outlet { since } = self.phase {
            next = Some(next.map_or(since + OUTLET_TIME, |at| at.min(since + OUTLET_TIME)));
        }
        if let Some((_, until)) = &self.toast
            && now < *until
        {
            next = Some(next.map_or(*until, |at| at.min(*until)));
        }
        next
    }
}

/// Copilot's single-line input box on the cursor's row: where the text after its prompt marker
/// starts in `line`, and that text.
///
/// Copilot 1.0 draws its input either as `┃ text` between `╻▄▄▄` and `╹▀▀▀` edges, or as
/// `❯ text` between two `────` rules, depending on the terminal.
fn input_box<'a>(above: &str, line: &'a str, below: &str) -> Option<(usize, &'a str)> {
    fn is_edge(row: &str) -> bool {
        let row = row.trim();
        !row.is_empty() && row.chars().all(|c| "─━═▄▀▔▁╻╹╭╮╰╯┌┐└┘".contains(c))
    }
    if !is_edge(above) || !is_edge(below) {
        return None;
    }
    let body = line
        .trim_start()
        .strip_prefix(['┃', '❯', '›', '>', '│'])?
        .trim_start();
    Some((line.chars().count() - body.chars().count(), body))
}

/// Recognises `/magic ...` typed into Copilot's input box. `caret` is the cursor's character
/// index in `line`. Returns the arguments and the number of characters to erase, which
/// includes spaces typed after the command when the cursor is behind them.
pub(crate) fn typed_command(
    above: &str,
    line: &str,
    below: &str,
    caret: usize,
) -> Option<(String, usize)> {
    let (start, body) = input_box(above, line, below)?;
    let text = body.trim_end();
    let rest = text.strip_prefix("/magic")?;
    if !(rest.is_empty() || rest.starts_with(' ')) || rest.trim().contains(char::is_whitespace) {
        return None;
    }
    // The padding after the text looks like typed spaces; only the cursor tells them apart.
    let typed = text.chars().count().max(caret.saturating_sub(start));
    Some((rest.trim().to_string(), typed))
}

/// Whether the text before the cursor in Copilot's input box can still become `/magic`: `/`,
/// `/ma`, `/magic li`, ... Text after the cursor is ignored: Copilot previews the highlighted
/// entry of its command list there.
pub(crate) fn typing_command(above: &str, line: &str, below: &str, caret: usize) -> bool {
    let Some((start, body)) = input_box(above, line, below) else {
        return false;
    };
    let typed: String = body.chars().take(caret.saturating_sub(start)).collect();
    typed.starts_with('/') && ("/magic".starts_with(typed.as_str()) || typed.starts_with("/magic "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn charged(magic: &mut Magic, now: Instant) {
        magic.handle(Event::Prompt("Create a constellation".into()), now);
        magic.handle(Event::TurnStart, now);
    }

    #[test]
    fn a_turn_charges_pours_and_returns_to_idle() {
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Fire, true);
        assert_eq!(magic.region_rows(40, now), IDLE_ROWS);
        charged(&mut magic, now);
        assert_eq!(magic.region_rows(40, now), CIRCLE_ROWS);
        magic.handle(
            Event::Reply {
                text: "Checking files".into(),
                tools: true,
            },
            now,
        );
        magic.handle(Event::TurnEnd, now);
        magic.tick(now + FINAL_GRACE * 2);
        assert_eq!(magic.phase, Phase::Charging, "tool turns continue");
        magic.handle(Event::TurnStart, now);
        magic.handle(
            Event::Reply {
                text: "Done".into(),
                tools: false,
            },
            now,
        );
        magic.handle(Event::TurnEnd, now);
        magic.tick(now + FINAL_GRACE / 2);
        assert_eq!(magic.phase, Phase::Charging);
        let settled = now + FINAL_GRACE;
        magic.tick(settled);
        assert_eq!(magic.phase, Phase::Outlet { since: settled });
        assert_eq!(magic.region_rows(40, settled), CIRCLE_ROWS + OUTLET_ROWS);
        magic.tick(settled + OUTLET_TIME);
        assert_eq!(magic.phase, Phase::Idle);
        assert!(!magic.circle.is_active());
    }

    #[test]
    fn a_new_turn_within_the_grace_period_keeps_charging() {
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Classic, true);
        charged(&mut magic, now);
        magic.handle(
            Event::Reply {
                text: "Almost".into(),
                tools: false,
            },
            now,
        );
        magic.handle(Event::TurnEnd, now);
        magic.handle(Event::TurnStart, now + Duration::from_millis(300));
        magic.tick(now + FINAL_GRACE * 3);
        assert_eq!(magic.phase, Phase::Charging);
    }

    #[test]
    fn a_charging_turn_records_its_spells() {
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Classic, true);
        let spell = |id: &str| Event::Spell {
            id: id.into(),
            tool: "glob".into(),
            detail: "*.md".into(),
            mcp: false,
            nested: false,
        };
        magic.handle(spell("early"), now);
        assert_eq!(magic.circle.chronicle.cast, 0, "no turn yet");
        charged(&mut magic, now);
        magic.handle(spell("1"), now);
        magic.handle(
            Event::SpellDone {
                id: "1".into(),
                ok: true,
            },
            now,
        );
        magic.handle(Event::Intent("Exploring".into()), now);
        magic.handle(
            Event::Reply {
                text: "Checking".into(),
                tools: true,
            },
            now,
        );
        let chronicle = &magic.circle.chronicle;
        assert_eq!((chronicle.cast, chronicle.oracles), (1, 1));
        assert_eq!(chronicle.intent.as_deref(), Some("Exploring"));
        magic.handle(Event::Prompt("Next".into()), now);
        assert_eq!(magic.circle.chronicle.cast, 0, "a new prompt starts afresh");
    }

    #[test]
    fn interruptions_end_without_the_outlet() {
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Classic, true);
        charged(&mut magic, now);
        magic.handle(
            Event::Reply {
                text: "Partial".into(),
                tools: false,
            },
            now,
        );
        magic.handle(Event::Aborted, now);
        magic.tick(now);
        assert_eq!(magic.phase, Phase::Idle);
        charged(&mut magic, now);
        magic.progress(Some(true), now);
        magic.progress(Some(false), now);
        magic.tick(now + FINAL_GRACE);
        assert_eq!(
            magic.phase,
            Phase::Idle,
            "progress clearing without an answer"
        );
    }

    #[test]
    fn permissions_and_small_terminals_shrink_the_region() {
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Classic, true);
        charged(&mut magic, now);
        magic.handle(Event::Attention(true), now);
        assert_eq!(magic.region_rows(40, now), IDLE_ROWS);
        magic.handle(Event::Attention(false), now);
        assert_eq!(magic.region_rows(30, now), 16);
        assert_eq!(magic.region_rows(MIN_CHILD_ROWS + 4, now), 0);
        magic.command("off", 40, now);
        assert_eq!(magic.region_rows(40, now), 1, "the notice stays visible");
        assert_eq!(magic.region_rows(40, now + TOAST_TIME), 0);
    }

    #[test]
    fn the_picker_needs_room_to_be_seen() {
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Fire, true);
        let short = MIN_CHILD_ROWS + IDLE_ROWS - 1;
        magic.command("list", short, now);
        assert!(
            magic.picker.is_none(),
            "an invisible picker would take every key"
        );
        assert_eq!(magic.toast(now), Some(TOO_SHORT));
        magic.command("list", short + 1, now);
        assert!(magic.picker.is_some());
        assert_eq!(magic.region_rows(short + 1, now), IDLE_ROWS);
        magic.picker_key(PickerKey::Down, now);
        magic.fit(40, now);
        assert_eq!(magic.shown_style(), MagicStyle::Water);
        magic.fit(short, now);
        assert!(magic.picker.is_none());
        assert_eq!(
            (magic.style, magic.enabled),
            (MagicStyle::Fire, true),
            "the preview is undone"
        );
        assert_eq!(magic.toast(now), Some(TOO_SHORT));
    }

    #[test]
    fn commands_and_the_picker() {
        let now = Instant::now();
        let mut magic = Magic::new(false, MagicStyle::Classic, true);
        magic.command("火", 40, now);
        assert!(magic.enabled);
        assert_eq!(magic.style, MagicStyle::Fire);
        assert_eq!(magic.toast(now), Some("Magic circle on · fire 火"));
        magic.command("ice", 40, now);
        assert_eq!(magic.toast(now), Some(USAGE));
        assert_eq!(magic.style, MagicStyle::Fire);

        magic.command("off", 40, now);
        magic.command("list", 40, now);
        assert_eq!(magic.region_rows(40, now), CIRCLE_ROWS);
        magic.picker_key(PickerKey::Down, now);
        assert_eq!(magic.shown_style(), MagicStyle::Water);
        magic.picker_key(PickerKey::Cancel, now);
        assert_eq!((magic.style, magic.enabled), (MagicStyle::Fire, false));

        magic.command("", 40, now);
        magic.picker_key(PickerKey::Up, now);
        magic.picker_key(PickerKey::Up, now);
        magic.picker_key(PickerKey::Up, now);
        assert_eq!(magic.shown_style(), MagicStyle::Tech);
        magic.picker_key(PickerKey::Accept, now);
        assert_eq!((magic.style, magic.enabled), (MagicStyle::Tech, true));
        magic.command("list", 40, now);
        magic.picker_key(PickerKey::Digit(8), now);
        assert_eq!(magic.style, MagicStyle::Dark);
        assert!(magic.picker.is_none());
    }

    #[test]
    fn typed_commands_are_read_from_the_input_box() {
        let top = "╻▄▄▄▄▄▄";
        let bottom = "╹▀▀▀▀▀▀";
        let at_end = |line: &str| line.trim_end().chars().count();
        let typed = |line: &str| typed_command(top, line, bottom, at_end(line));
        assert_eq!(typed("┃ /magic on     "), Some(("on".into(), 9)));
        assert_eq!(typed("┃ /magic 火"), Some(("火".into(), 8)));
        assert_eq!(typed("┃ /magic"), Some((String::new(), 6)));
        assert_eq!(typed("┃ /magical"), None);
        assert_eq!(typed("┃ /magic on now"), None);
        assert_eq!(typed("┃ tell me /magic"), None);
        assert_eq!(
            typed_command("┃ first line", "┃ /magic on", bottom, 11),
            None
        );
        let rule = "────────────────";
        assert_eq!(
            typed_command(rule, "❯ /magic list   ", rule, 13),
            Some(("list".into(), 11))
        );
        assert_eq!(
            typed_command(rule, "❯ /magic list", "❯ second line", 13),
            None
        );
    }

    #[test]
    fn spaces_typed_after_the_command_are_erased_too() {
        let (top, bottom) = ("╻▄▄▄▄▄▄", "╹▀▀▀▀▀▀");
        // `/magic ` with the cursor after the space: all seven characters go.
        assert_eq!(
            typed_command(top, "┃ /magic         ", bottom, 9),
            Some((String::new(), 7))
        );
        assert_eq!(
            typed_command(top, "┃ /magic 火       ", bottom, 11),
            Some(("火".into(), 9))
        );
        // With the cursor moved back into the text, End moves it behind the typed text.
        assert_eq!(
            typed_command(top, "┃ /magic off     ", bottom, 5),
            Some(("off".into(), 10))
        );
    }

    #[test]
    fn the_start_of_the_command_is_recognised_while_typing() {
        let (top, bottom) = ("╻▄▄▄▄▄▄", "╹▀▀▀▀▀▀");
        let typing = |line: &str, caret: usize| typing_command(top, line, bottom, caret);
        // After `/`, Copilot previews the highlighted command behind the cursor.
        assert!(typing("┃ /add-dir", 3));
        assert!(typing("┃ /ma", 5));
        assert!(typing("┃ /magic", 8));
        assert!(typing("┃ /magic li   ", 11));
        assert!(!typing("┃ /model", 8));
        assert!(!typing("┃ /magical", 10));
        assert!(!typing("┃ tell me /ma", 13));
        assert!(!typing("┃ ", 2));
        assert!(!typing_command("┃ first line", "┃ /ma", bottom, 5));
        let rule = "────────────────";
        assert!(typing_command(rule, "❯ /mag", rule, 6));
    }

    #[test]
    fn the_command_hint_shows_while_the_circle_is_visible() {
        let now = Instant::now();
        let mut magic = Magic::new(true, MagicStyle::Classic, true);
        magic.set_command_hint(true);
        assert_eq!(magic.notice(now), Some(COMMAND_HINT));
        assert_eq!(magic.region_rows(40, now), IDLE_ROWS, "no extra rows");
        magic.command("off", 40, now);
        assert_eq!(
            magic.notice(now),
            magic.toast(now),
            "a hidden circle shows only its own notices"
        );
        assert_eq!(magic.region_rows(40, now + TOAST_TIME), 0);
        magic.command("on", 40, now);
        magic.set_command_hint(false);
        assert_eq!(magic.notice(now), magic.toast(now));
    }
}
