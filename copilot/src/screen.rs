//! The child's screen: a VT emulator plus the terminal traffic it cannot handle itself.
//!
//! Copilot CLI queries the terminal (device attributes, mode reports, colours, version) and
//! sends terminal-wide commands (titles, progress, default colours, clipboard, keyboard modes).
//! Both are passed to the real terminal unchanged, so the child gets the same answers and
//! effects as without the wrapper; the answers come back through the keyboard input and are
//! forwarded like keys. Only questions about the child's own screen (cursor position, text
//! area size) are answered here. Hyperlinks, shell-integration marks and images are dropped.

use vt100::Callbacks;
use vt100::MouseProtocolEncoding;
use vt100::MouseProtocolMode;
use vt100::Parser;
use vt100::Screen;

fn osc(out: &mut Vec<u8>, params: &[&[u8]]) {
    out.extend_from_slice(b"\x1b]");
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            out.push(b';');
        }
        out.extend_from_slice(param);
    }
    out.extend_from_slice(b"\x1b\\");
}

/// Re-encodes a CSI sequence as the child sent it.
fn csi(out: &mut Vec<u8>, i1: Option<u8>, i2: Option<u8>, params: &[&[u16]], c: char) {
    let (private, intermediate) = match i1 {
        Some(marker @ (b'<' | b'=' | b'>' | b'?')) => (Some(marker), i2),
        other => (None, other.or(i2)),
    };
    out.extend_from_slice(b"\x1b[");
    out.extend(private);
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            out.push(b';');
        }
        for (sub, value) in param.iter().enumerate() {
            if sub > 0 {
                out.push(b':');
            }
            out.extend_from_slice(value.to_string().as_bytes());
        }
    }
    out.extend(intermediate);
    out.push(c as u8);
}

#[derive(Default)]
pub(crate) struct Hooks {
    /// Commands and queries for the real terminal, in order.
    pub(crate) to_terminal: Vec<u8>,
    /// Answers about the child's own screen.
    pub(crate) to_child: Vec<u8>,
    /// The child is between synchronized-output begin and end.
    pub(crate) sync: bool,
    pub(crate) focus_reporting: bool,
    /// `OSC 9;4` progress: busy while `Some(true)`.
    pub(crate) progress: Option<bool>,
}

impl Hooks {
    fn private_mode(&mut self, params: &[&[u16]], on: bool) {
        for param in params {
            match param.first() {
                Some(2026) => self.sync = on,
                Some(1004) => self.focus_reporting = on,
                _ => {}
            }
        }
    }
}

impl Callbacks for Hooks {
    fn audible_bell(&mut self, _: &mut Screen) {
        self.to_terminal.push(7);
    }

    fn set_window_title(&mut self, _: &mut Screen, title: &[u8]) {
        osc(&mut self.to_terminal, &[b"2", title]);
    }

    fn copy_to_clipboard(&mut self, _: &mut Screen, ty: &[u8], data: &[u8]) {
        osc(&mut self.to_terminal, &[b"52", ty, data]);
    }

    fn paste_from_clipboard(&mut self, _: &mut Screen, ty: &[u8]) {
        osc(&mut self.to_terminal, &[b"52", ty, b"?"]);
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|param| param.first()).copied();
        match (i1, i2, c) {
            // Where the cursor is, in the child's coordinates.
            (None, None, 'n') if first == Some(6) => {
                let (row, col) = screen.cursor_position();
                let reply = format!("\x1b[{};{}R", row + 1, col + 1);
                self.to_child.extend_from_slice(reply.as_bytes());
            }
            (Some(b'?'), None, 'n') if first == Some(6) => {
                let (row, col) = screen.cursor_position();
                let reply = format!("\x1b[?{};{};1R", row + 1, col + 1);
                self.to_child.extend_from_slice(reply.as_bytes());
            }
            (None, None, 't') if first == Some(18) => {
                let (rows, cols) = screen.size();
                let reply = format!("\x1b[8;{rows};{cols}t");
                self.to_child.extend_from_slice(reply.as_bytes());
            }
            (Some(b'?'), None, 'h') => self.private_mode(params, true),
            (Some(b'?'), None, 'l') => self.private_mode(params, false),
            // Device attributes and status, mode reports, version, kitty keyboard query and
            // flags, modifyOtherKeys, cursor shape.
            (None | Some(b'>' | b'='), None, 'c')
            | (None | Some(b'?'), None, 'n')
            | (_, Some(b'$'), 'p')
            | (Some(b'$'), None, 'p')
            | (Some(b'>'), None, 'q')
            | (Some(b'?' | b'>' | b'<' | b'='), None, 'u')
            | (Some(b'>'), None, 'm')
            | (Some(b' '), None, 'q') => csi(&mut self.to_terminal, i1, i2, params, c),
            // Title stack push and pop, pixel sizes.
            (None, None, 't') if matches!(first, Some(14 | 16 | 19 | 22 | 23)) => {
                csi(&mut self.to_terminal, i1, i2, params, c);
            }
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _: &mut Screen, params: &[&[u8]]) {
        let Some(&command) = params.first() else {
            return;
        };
        match command {
            // Titles whose text contains `;` are split by the parser.
            b"0" | b"1" | b"2" => {
                let title = params[1..].join(&b';');
                osc(&mut self.to_terminal, &[command, &title]);
            }
            b"9" => {
                if params.get(1) == Some(&&b"4"[..]) {
                    self.progress = Some(params.get(2).is_some_and(|state| *state != b"0"));
                }
                osc(&mut self.to_terminal, params);
            }
            // Colours (queries and changes), clipboard, working directory, pointer, notices.
            b"4" | b"10" | b"11" | b"12" | b"104" | b"110" | b"111" | b"112" | b"52" | b"7"
            | b"22" | b"777" => osc(&mut self.to_terminal, params),
            _ => {}
        }
    }
}

/// The emulated screen of the child and its state beyond the cells.
pub(crate) struct ChildScreen {
    parser: Parser<Hooks>,
}

impl ChildScreen {
    pub(crate) fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: Parser::new_with_callbacks(rows.max(1), cols.max(1), 0, Hooks::default()),
        }
    }

    pub(crate) fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub(crate) fn screen(&self) -> &Screen {
        self.parser.screen()
    }

    pub(crate) fn hooks(&self) -> &Hooks {
        self.parser.callbacks()
    }

    pub(crate) fn hooks_mut(&mut self) -> &mut Hooks {
        self.parser.callbacks_mut()
    }

    pub(crate) fn set_size(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows.max(1), cols.max(1));
    }

    /// Text of one row, with wide characters counted once.
    pub(crate) fn row_text(&self, row: u16) -> String {
        let screen = self.screen();
        let (_, cols) = screen.size();
        let mut text = String::new();
        for col in 0..cols {
            match screen.cell(row, col) {
                Some(cell) if cell.is_wide_continuation() => {}
                Some(cell) if cell.has_contents() => text.push_str(cell.contents()),
                _ => text.push(' '),
            }
        }
        text
    }

    /// Index in [`row_text`](Self::row_text) of the character at column `col`.
    pub(crate) fn char_index(&self, row: u16, col: u16) -> usize {
        let screen = self.screen();
        (0..col)
            .map(|col| match screen.cell(row, col) {
                Some(cell) if cell.is_wide_continuation() => 0,
                Some(cell) if cell.has_contents() => cell.contents().chars().count(),
                _ => 1,
            })
            .sum()
    }

    /// What the child printed on its normal screen, with its colours, while it is not showing
    /// the alternate screen: e.g. Copilot's exit summary with the command that resumes the
    /// session, which a direct run leaves in the terminal.
    pub(crate) fn normal_screen_output(&self) -> Vec<u8> {
        let screen = self.screen();
        if screen.alternate_screen() {
            return Vec::new();
        }
        let (_, cols) = screen.size();
        let rows: Vec<String> = screen.rows(0, cols).collect();
        let Some(last) = rows.iter().rposition(|row| !row.trim().is_empty()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for row in screen.rows_formatted(0, cols).take(last + 1) {
            out.extend_from_slice(&row);
            out.extend_from_slice(b"\x1b[0m\r\n");
        }
        out
    }
}

/// Input modes the real terminal must mirror so that it sends what the child expects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputModes {
    pub(crate) mouse: MouseMode,
    pub(crate) sgr_mouse: bool,
    pub(crate) utf8_mouse: bool,
    pub(crate) bracketed_paste: bool,
    pub(crate) focus: bool,
    pub(crate) application_cursor: bool,
    pub(crate) application_keypad: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MouseMode {
    #[default]
    None,
    Press,
    PressRelease,
    ButtonMotion,
    AnyMotion,
}

impl InputModes {
    pub(crate) fn of(child: &ChildScreen) -> Self {
        let screen = child.screen();
        Self {
            mouse: match screen.mouse_protocol_mode() {
                MouseProtocolMode::None => MouseMode::None,
                MouseProtocolMode::Press => MouseMode::Press,
                MouseProtocolMode::PressRelease => MouseMode::PressRelease,
                MouseProtocolMode::ButtonMotion => MouseMode::ButtonMotion,
                MouseProtocolMode::AnyMotion => MouseMode::AnyMotion,
            },
            sgr_mouse: screen.mouse_protocol_encoding() == MouseProtocolEncoding::Sgr,
            utf8_mouse: screen.mouse_protocol_encoding() == MouseProtocolEncoding::Utf8,
            bracketed_paste: screen.bracketed_paste(),
            focus: child.hooks().focus_reporting,
            application_cursor: screen.application_cursor(),
            application_keypad: screen.application_keypad(),
        }
    }

    /// Sequences that switch the real terminal from `previous` to these modes.
    pub(crate) fn transition(&self, previous: &Self) -> String {
        let mut out = String::new();
        let mut set = |code: &str, on: bool| {
            out.push_str("\x1b[?");
            out.push_str(code);
            out.push(if on { 'h' } else { 'l' });
        };
        if self.mouse != previous.mouse {
            for code in ["9", "1000", "1002", "1003"] {
                set(code, false);
            }
            match self.mouse {
                MouseMode::None => {}
                MouseMode::Press => set("9", true),
                MouseMode::PressRelease => set("1000", true),
                MouseMode::ButtonMotion => set("1002", true),
                MouseMode::AnyMotion => set("1003", true),
            }
        }
        if self.sgr_mouse != previous.sgr_mouse {
            set("1006", self.sgr_mouse);
        }
        if self.utf8_mouse != previous.utf8_mouse {
            set("1005", self.utf8_mouse);
        }
        if self.bracketed_paste != previous.bracketed_paste {
            set("2004", self.bracketed_paste);
        }
        if self.focus != previous.focus {
            set("1004", self.focus);
        }
        if self.application_cursor != previous.application_cursor {
            set("1", self.application_cursor);
        }
        if self.application_keypad != previous.application_keypad {
            out.push_str(if self.application_keypad {
                "\x1b="
            } else {
                "\x1b>"
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child() -> ChildScreen {
        ChildScreen::new(10, 40)
    }

    fn take(bytes: &mut Vec<u8>) -> String {
        String::from_utf8(std::mem::take(bytes)).unwrap()
    }

    #[test]
    fn terminal_queries_go_to_the_real_terminal_and_screen_queries_are_answered() {
        let mut screen = child();
        screen.process(
            b"\x1b[3;5H\x1b[c\x1b[6n\x1b[?2026$p\x1b[?u\x1b[>q\x1b]11;?\x1b\\\x1b]4;1;?\x07\x1b[18t",
        );
        assert_eq!(
            take(&mut screen.hooks_mut().to_child),
            "\x1b[3;5R\x1b[8;10;40t"
        );
        assert_eq!(
            take(&mut screen.hooks_mut().to_terminal),
            "\x1b[0c\x1b[?2026$p\x1b[?0u\x1b[>0q\x1b]11;?\x1b\\\x1b]4;1;?\x1b\\"
        );
    }

    #[test]
    fn keyboard_modes_and_colours_are_passed_through() {
        let mut screen = child();
        screen.process(
            b"\x1b[>4;2m\x1b[>1u\x1b[<u\x1b]11;#0D1117\x1b\\\x1b]111\x07\x1b[2 q\x1b[22;0t",
        );
        assert_eq!(
            take(&mut screen.hooks_mut().to_terminal),
            "\x1b[>4;2m\x1b[>1u\x1b[<0u\x1b]11;#0D1117\x1b\\\x1b]111\x1b\\\x1b[2 q\x1b[22;0t"
        );
    }

    #[test]
    fn titles_progress_and_sync_are_tracked() {
        let mut screen = child();
        screen.process(b"\x1b]0;a;b\x07\x1b]9;4;3;0\x07\x1b[?2026h\x1b[?1004h");
        assert!(screen.hooks().sync && screen.hooks().focus_reporting);
        assert_eq!(screen.hooks().progress, Some(true));
        screen.process(b"\x1b[?2026l\x1b]9;4;0;0\x07\x1b]8;;https://x\x07link\x1b]8;;\x07");
        assert!(!screen.hooks().sync);
        assert_eq!(screen.hooks().progress, Some(false));
        assert_eq!(
            take(&mut screen.hooks_mut().to_terminal),
            "\x1b]0;a;b\x1b\\\x1b]9;4;3;0\x1b\\\x1b]9;4;0;0\x1b\\"
        );
        assert!(screen.row_text(0).starts_with("link"));
    }

    #[test]
    fn input_modes_follow_the_child() {
        let mut screen = child();
        let before = InputModes::of(&screen);
        screen.process(b"\x1b[?1003h\x1b[?1006h\x1b[?2004h\x1b[?1004h");
        let after = InputModes::of(&screen);
        assert_eq!(
            after.transition(&before),
            "\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1003h\x1b[?1006h\x1b[?2004h\x1b[?1004h"
        );
        assert_eq!(after.transition(&after), "");
    }

    #[test]
    fn cursor_columns_map_to_characters_of_the_row_text() {
        let mut screen = child();
        screen.process("┃ /magic 火 ".as_bytes());
        let (row, col) = screen.screen().cursor_position();
        assert_eq!(col, 12, "火 is two columns wide");
        let caret = screen.char_index(row, col);
        assert_eq!(caret, 11);
        assert_eq!(
            screen.row_text(row).chars().take(caret).collect::<String>(),
            "┃ /magic 火 "
        );
    }

    #[test]
    fn the_normal_screen_is_kept_but_not_the_alternate_one() {
        let mut screen = child();
        screen.process(b"\x1b[?1049h\x1b[Hfull-screen UI");
        assert!(screen.normal_screen_output().is_empty());
        screen.process(b"\x1b[?1049l\r\n  \x1b[32m+0\x1b[39m Resume copilot --resume=abc\r\n\r\n");
        let out = String::from_utf8(screen.normal_screen_output()).unwrap();
        assert!(!out.contains("full-screen"), "{out:?}");
        assert!(out.contains("Resume copilot --resume=abc"), "{out:?}");
        assert!(out.contains("32m+0"), "colours are kept: {out:?}");
        assert!(
            out.ends_with("\x1b[0m\r\n") && out.matches("\r\n").count() == 2,
            "{out:?}"
        );
        assert!(child().normal_screen_output().is_empty());
    }
}
