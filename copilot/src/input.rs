//! Splits the bytes the real terminal sends into keys, pastes, mouse events and replies.
//!
//! Everything is forwarded to the child as sent, except that mouse rows are shifted by the
//! height of the magic region, and replies to this program's own queries are consumed.

use std::time::Duration;
use std::time::Instant;

/// How long an unfinished escape sequence may wait for the rest of its bytes.
const ESCAPE_TIMEOUT: Duration = Duration::from_millis(30);
/// Large pastes arrive in several reads; wait longer for the end marker.
const PASTE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    /// Printable text and control characters such as Enter (`\r`) and Backspace.
    Text(String),
    /// An escape sequence for a key, e.g. an arrow or an Alt combination; a lone `ESC` is Escape.
    Key(String),
    /// A complete bracketed paste, markers included.
    Paste(String),
    Mouse(Mouse),
    Focus(bool),
    /// An OSC, DCS or device-attribute reply from the terminal.
    Reply(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mouse {
    pub(crate) sgr: bool,
    pub(crate) button: u32,
    /// One-based column and row.
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) release: bool,
}

impl Mouse {
    /// Re-encodes the event `rows` rows higher, or `None` when it lands above the child.
    pub(crate) fn shifted(&self, rows: u16) -> Option<String> {
        let y = self.y.checked_sub(u32::from(rows)).filter(|y| *y >= 1)?;
        if self.sgr {
            let end = if self.release { 'm' } else { 'M' };
            return Some(format!("\x1b[<{};{};{}{end}", self.button, self.x, y));
        }
        let byte = |value: u32| char::from_u32(value + 32).unwrap_or(' ');
        Some(format!(
            "\x1b[M{}{}{}",
            byte(self.button),
            byte(self.x),
            byte(y)
        ))
    }
}

#[derive(Default)]
pub(crate) struct InputParser {
    buffer: String,
    since: Option<Instant>,
}

enum Scan {
    Done(Token, usize),
    Incomplete,
}

fn scan_csi(text: &str) -> Scan {
    // `text` starts with ESC [.
    let bytes = text.as_bytes();
    if bytes.get(2) == Some(&b'M') {
        // Legacy mouse: ESC [ M followed by three characters.
        let mut chars = text[3..].chars();
        let (Some(b), Some(x), Some(y)) = (chars.next(), chars.next(), chars.next()) else {
            return Scan::Incomplete;
        };
        let len = 3 + b.len_utf8() + x.len_utf8() + y.len_utf8();
        let value = |c: char| u32::from(c).saturating_sub(32);
        let mouse = Mouse {
            sgr: false,
            button: value(b),
            x: value(x),
            y: value(y),
            release: false,
        };
        return Scan::Done(Token::Mouse(mouse), len);
    }
    for (offset, &byte) in bytes.iter().enumerate().skip(2) {
        if (0x40..=0x7e).contains(&byte) {
            let len = offset + 1;
            let sequence = &text[..len];
            let body = &sequence[2..len - 1];
            let token = match (byte, body) {
                (b'~', "200") => return scan_paste(text),
                (b'I', "") => Token::Focus(true),
                (b'O', "") => Token::Focus(false),
                (b'M' | b'm', _) if body.starts_with('<') => match sgr_mouse(body, byte == b'm') {
                    Some(mouse) => Token::Mouse(mouse),
                    None => Token::Key(sequence.to_string()),
                },
                (b'c', _) if body.starts_with('?') => Token::Reply(sequence.to_string()),
                _ => Token::Key(sequence.to_string()),
            };
            return Scan::Done(token, len);
        }
        if !(0x20..=0x3f).contains(&byte) {
            // Not a CSI after all; treat ESC [ as Alt+[.
            return Scan::Done(Token::Key(text[..2].to_string()), 2);
        }
    }
    Scan::Incomplete
}

fn sgr_mouse(body: &str, release: bool) -> Option<Mouse> {
    let mut parts = body[1..].split(';').map(|part| part.parse::<u32>().ok());
    let (Some(Some(button)), Some(Some(x)), Some(Some(y)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    Some(Mouse {
        sgr: true,
        button,
        x,
        y,
        release,
    })
}

fn scan_paste(text: &str) -> Scan {
    const END: &str = "\x1b[201~";
    match text.find(END) {
        Some(at) => {
            let len = at + END.len();
            Scan::Done(Token::Paste(text[..len].to_string()), len)
        }
        None => Scan::Incomplete,
    }
}

fn scan_string(text: &str) -> Scan {
    // OSC (ESC ]) or DCS (ESC P), ended by BEL or ST.
    let bytes = text.as_bytes();
    let mut offset = 2;
    while offset < bytes.len() {
        match bytes[offset] {
            7 => return Scan::Done(Token::Reply(text[..=offset].to_string()), offset + 1),
            0x1b if bytes.get(offset + 1) == Some(&b'\\') => {
                return Scan::Done(Token::Reply(text[..offset + 2].to_string()), offset + 2);
            }
            0x1b if offset + 1 < bytes.len() => {
                // Unterminated; give up on it at the next escape.
                return Scan::Done(Token::Reply(text[..offset].to_string()), offset);
            }
            _ => offset += 1,
        }
    }
    Scan::Incomplete
}

fn scan_escape(text: &str) -> Scan {
    let mut chars = text.chars();
    chars.next();
    let Some(next) = chars.next() else {
        return Scan::Incomplete;
    };
    match next {
        '[' => scan_csi(text),
        ']' | 'P' => scan_string(text),
        'O' => match chars.next() {
            Some(c) => {
                let len = 2 + c.len_utf8();
                Scan::Done(Token::Key(text[..len].to_string()), len)
            }
            None => Scan::Incomplete,
        },
        _ => {
            let len = 1 + next.len_utf8();
            Scan::Done(Token::Key(text[..len].to_string()), len)
        }
    }
}

impl InputParser {
    pub(crate) fn push(&mut self, text: &str, now: Instant) -> Vec<Token> {
        self.buffer.push_str(text);
        let tokens = self.drain(/*force*/ false);
        self.since = (!self.buffer.is_empty()).then_some(self.since.unwrap_or(now));
        tokens
    }

    /// Emits a sequence that stayed incomplete for too long, e.g. a lone Escape key press.
    pub(crate) fn flush(&mut self, now: Instant) -> Vec<Token> {
        match self.deadline() {
            Some(deadline) if now >= deadline => {
                let tokens = self.drain(/*force*/ true);
                self.since = None;
                tokens
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        let timeout = if self.buffer.starts_with("\x1b[200~") {
            PASTE_TIMEOUT
        } else {
            ESCAPE_TIMEOUT
        };
        self.since.map(|since| since + timeout)
    }

    fn drain(&mut self, force: bool) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut rest = std::mem::take(&mut self.buffer);
        loop {
            if rest.is_empty() {
                break;
            }
            let Some(escape) = rest.find('\x1b') else {
                push_text(&mut tokens, &rest);
                rest.clear();
                break;
            };
            if escape > 0 {
                push_text(&mut tokens, &rest[..escape]);
                rest.drain(..escape);
                continue;
            }
            match scan_escape(&rest) {
                Scan::Done(token, len) => {
                    tokens.push(token);
                    rest.drain(..len);
                }
                Scan::Incomplete if force => {
                    if rest.starts_with("\x1b[200~") {
                        // A paste whose end marker never came is still delivered as a paste.
                        tokens.push(Token::Paste(rest.clone()));
                    } else {
                        tokens.push(Token::Key(rest.clone()));
                    }
                    rest.clear();
                }
                Scan::Incomplete => break,
            }
        }
        self.buffer = rest;
        tokens
    }
}

fn push_text(tokens: &mut Vec<Token>, text: &str) {
    if let Some(Token::Text(last)) = tokens.last_mut() {
        last.push_str(text);
    } else {
        tokens.push(Token::Text(text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(chunks: &[&str]) -> Vec<Token> {
        let mut parser = InputParser::default();
        let now = Instant::now();
        let mut tokens = Vec::new();
        for chunk in chunks {
            tokens.extend(parser.push(chunk, now));
        }
        tokens.extend(parser.flush(now + ESCAPE_TIMEOUT));
        tokens
    }

    #[test]
    fn keys_text_and_split_sequences() {
        assert_eq!(
            parse(&["ab\r", "\x1b[", "A", "\x1bOB\x1bx中"]),
            vec![
                Token::Text("ab\r".into()),
                Token::Key("\x1b[A".into()),
                Token::Key("\x1bOB".into()),
                Token::Key("\x1bx".into()),
                Token::Text("中".into()),
            ]
        );
    }

    #[test]
    fn lone_escape_waits_for_the_timeout() {
        let mut parser = InputParser::default();
        let now = Instant::now();
        assert_eq!(parser.push("\x1b", now), Vec::new());
        assert_eq!(parser.flush(now + Duration::from_millis(5)), Vec::new());
        assert_eq!(
            parser.flush(now + ESCAPE_TIMEOUT),
            vec![Token::Key("\x1b".into())]
        );
        assert_eq!(parser.deadline(), None);
    }

    #[test]
    fn pastes_stay_whole_even_with_enter_inside() {
        assert_eq!(
            parse(&["\x1b[200~line 1\r", "line 2\x1b[201~x"]),
            vec![
                Token::Paste("\x1b[200~line 1\rline 2\x1b[201~".into()),
                Token::Text("x".into()),
            ]
        );
    }

    #[test]
    fn mouse_rows_shift_below_the_region() {
        let tokens = parse(&["\x1b[<64;10;30M\x1b[<0;3;2m\x1b[M +5"]);
        let mice: Vec<Mouse> = tokens
            .iter()
            .filter_map(|token| match token {
                Token::Mouse(mouse) => Some(*mouse),
                _ => None,
            })
            .collect();
        assert_eq!(mice.len(), 3);
        assert_eq!(mice[0].shifted(21).as_deref(), Some("\x1b[<64;10;9M"));
        assert_eq!(
            mice[1].shifted(5),
            None,
            "clicks on the circle stay with it"
        );
        assert_eq!(mice[1].shifted(0).as_deref(), Some("\x1b[<0;3;2m"));
        assert_eq!(mice[2].y, 21);
        assert_eq!(mice[2].shifted(5).as_deref(), Some("\x1b[M +0"));
    }

    #[test]
    fn replies_and_focus_are_recognised() {
        assert_eq!(
            parse(&["\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\\x1b[?61;4c\x1b[I\x1b]10;rgb:cc/cc/cc\x07"]),
            vec![
                Token::Reply("\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\".into()),
                Token::Reply("\x1b[?61;4c".into()),
                Token::Focus(true),
                Token::Reply("\x1b]10;rgb:cc/cc/cc\x07".into()),
            ]
        );
    }
}
