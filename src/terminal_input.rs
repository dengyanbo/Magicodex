use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant},
};

const MAX_PASTE_BYTES: usize = 1024 * 1024;
const PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Default)]
pub struct Decoder {
    bytes: Vec<u8>,
    events: VecDeque<Event>,
    paste: bool,
    escape_at: Option<Instant>,
}

impl Decoder {
    pub fn feed(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.bytes.len() + bytes.len() > MAX_PASTE_BYTES {
            return Err(io::Error::other(
                "输入超过 1 MiB；未提交任何输入，已停止终端读取",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        self.parse()
    }

    fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.events
            .push_back(Event::Key(KeyEvent::new(code, modifiers)));
    }

    fn parse(&mut self) -> io::Result<()> {
        while !self.bytes.is_empty() {
            if self.paste {
                let Some(end) = self
                    .bytes
                    .windows(PASTE_END.len())
                    .position(|w| w == PASTE_END)
                else {
                    return Ok(());
                };
                let text = String::from_utf8(self.bytes[..end].to_vec())
                    .map_err(|_| io::Error::other("粘贴内容不是有效 UTF-8；未提交"))?;
                self.bytes.drain(..end + PASTE_END.len());
                self.events.push_back(Event::Paste(text));
                self.paste = false;
                continue;
            }
            if self.bytes.starts_with(b"\x1b[200~") {
                self.bytes.drain(..6);
                self.paste = true;
                self.escape_at = None;
                continue;
            }
            if self.bytes[0] == 0x1b {
                self.escape_at.get_or_insert_with(Instant::now);
                if self.bytes.len() < 2 {
                    break;
                }
                if self.bytes[1] == b'[' || self.bytes[1] == b'O' {
                    let Some(end) = self.bytes[2..]
                        .iter()
                        .position(|b| (0x40..=0x7e).contains(b))
                        .map(|p| p + 2)
                    else {
                        break;
                    };
                    let sequence = self.bytes[2..=end].to_vec();
                    self.bytes.drain(..=end);
                    self.sequence(&sequence);
                } else {
                    let Some((character, size)) = decode_character(&self.bytes[1..])? else {
                        break;
                    };
                    self.bytes.drain(..size + 1);
                    self.character(character, KeyModifiers::ALT);
                }
                self.escape_at = None;
                continue;
            }
            let Some((character, size)) = decode_character(&self.bytes)? else {
                break;
            };
            self.bytes.drain(..size);
            self.character(character, KeyModifiers::NONE);
        }
        Ok(())
    }

    fn character(&mut self, character: char, mut modifiers: KeyModifiers) {
        let code = match character {
            '\r' | '\n' => KeyCode::Enter,
            '\t' => KeyCode::Tab,
            '\x7f' | '\x08' => KeyCode::Backspace,
            '\x01'..='\x1a' => {
                modifiers |= KeyModifiers::CONTROL;
                KeyCode::Char(char::from_u32(u32::from(character) + 96).expect("ASCII control"))
            }
            c if !c.is_control() => KeyCode::Char(c),
            _ => return,
        };
        self.key(code, modifiers);
    }

    fn sequence(&mut self, sequence: &[u8]) {
        let Some((&last, numbers)) = sequence.split_last() else {
            return;
        };
        let numbers = std::str::from_utf8(numbers).unwrap_or("");
        let parts: Vec<_> = numbers
            .split(';')
            .filter_map(|v| v.parse::<u16>().ok())
            .collect();
        let modifier = parts.get(1).copied().unwrap_or(1).saturating_sub(1);
        let mut modifiers = KeyModifiers::NONE;
        if modifier & 1 != 0 {
            modifiers |= KeyModifiers::SHIFT;
        }
        if modifier & 2 != 0 {
            modifiers |= KeyModifiers::ALT;
        }
        if modifier & 4 != 0 {
            modifiers |= KeyModifiers::CONTROL;
        }
        let code = match last {
            b'A' => KeyCode::Up,
            b'B' => KeyCode::Down,
            b'C' => KeyCode::Right,
            b'D' => KeyCode::Left,
            b'H' => KeyCode::Home,
            b'F' => KeyCode::End,
            b'P' => KeyCode::F(1),
            b'Q' => KeyCode::F(2),
            b'R' => KeyCode::F(3),
            b'S' => KeyCode::F(4),
            b'Z' => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::BackTab
            }
            b'~' => match parts.first().copied() {
                Some(1 | 7) => KeyCode::Home,
                Some(4 | 8) => KeyCode::End,
                Some(2) => KeyCode::Insert,
                Some(3) => KeyCode::Delete,
                Some(5) => KeyCode::PageUp,
                Some(6) => KeyCode::PageDown,
                Some(n @ 11..=15) => KeyCode::F((n - 10) as u8),
                Some(n @ 17..=21) => KeyCode::F((n - 11) as u8),
                Some(23 | 24) => KeyCode::F((parts[0] - 12) as u8),
                _ => return,
            },
            b'u' => {
                if let Some(c) = parts.first().and_then(|n| char::from_u32(u32::from(*n))) {
                    self.character(c, modifiers);
                }
                return;
            }
            b'I' => {
                self.events.push_back(Event::FocusGained);
                return;
            }
            b'O' => {
                self.events.push_back(Event::FocusLost);
                return;
            }
            _ => return,
        };
        self.key(code, modifiers);
    }

    pub fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    fn expire_escape(&mut self) {
        if self.bytes == b"\x1b"
            && self
                .escape_at
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(50))
        {
            self.bytes.clear();
            self.escape_at = None;
            self.key(KeyCode::Esc, KeyModifiers::NONE);
        }
    }
}

fn decode_character(bytes: &[u8]) -> io::Result<Option<(char, usize)>> {
    let width = match bytes[0] {
        0..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Err(io::Error::other("终端输入包含无效 UTF-8")),
    };
    if bytes.len() < width {
        return Ok(None);
    }
    let character = std::str::from_utf8(&bytes[..width])
        .map_err(|_| io::Error::other("终端输入包含无效 UTF-8"))?
        .chars()
        .next()
        .expect("nonempty character");
    Ok(Some((character, width)))
}

pub struct InputReader {
    #[cfg(windows)]
    rx: std::sync::mpsc::Receiver<io::Result<Vec<u8>>>,
    #[cfg(windows)]
    decoder: Decoder,
    #[cfg(windows)]
    size: (u16, u16),
}

impl InputReader {
    pub fn new() -> io::Result<Self> {
        #[cfg(windows)]
        {
            use std::io::Read;
            let (tx, rx) = std::sync::mpsc::sync_channel(4);
            std::thread::spawn(move || {
                let mut input = io::stdin().lock();
                let mut buffer = [0; 8192];
                loop {
                    let result = match input.read(&mut buffer) {
                        Ok(0) => Err(io::Error::other("终端输入已关闭")),
                        Ok(count) => Ok(buffer[..count].to_vec()),
                        Err(error) => Err(error),
                    };
                    let failed = result.is_err();
                    if tx.send(result).is_err() || failed {
                        break;
                    }
                }
            });
            Ok(Self {
                rx,
                decoder: Decoder::default(),
                size: crossterm::terminal::size()?,
            })
        }
        #[cfg(not(windows))]
        Ok(Self {})
    }

    pub fn next_event(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        #[cfg(windows)]
        {
            let size = crossterm::terminal::size()?;
            if size != self.size {
                self.size = size;
                return Ok(Some(Event::Resize(size.0, size.1)));
            }
            self.decoder.expire_escape();
            if let Some(event) = self.decoder.next_event() {
                return Ok(Some(event));
            }
            match self.rx.recv_timeout(timeout) {
                Ok(bytes) => self.decoder.feed(&bytes?)?,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => self.decoder.expire_escape(),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("终端输入线程已退出"));
                }
            }
            Ok(self.decoder.next_event())
        }
        #[cfg(not(windows))]
        {
            if crossterm::event::poll(timeout)? {
                crossterm::event::read().map(Some)
            } else {
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_bracketed_paste_is_one_event_and_cannot_submit() {
        let mut decoder = Decoder::default();
        let data = "\x1b[200~中文\n下一行\x1b[201~";
        for byte in data.bytes() {
            decoder.feed(&[byte]).unwrap();
        }
        assert_eq!(
            decoder.next_event(),
            Some(Event::Paste("中文\n下一行".into()))
        );
        assert!(decoder.next_event().is_none());
        decoder.feed(b"\r").unwrap();
        assert!(matches!(
            decoder.next_event(),
            Some(Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }))
        ));
    }

    #[test]
    fn navigation_modifiers_and_unicode_are_preserved() {
        let mut d = Decoder::default();
        d.feed("\x1bOS\x1b[6~\x1b\r\x03魔".as_bytes()).unwrap();
        let keys: Vec<_> = std::iter::from_fn(|| d.next_event()).collect();
        assert_eq!(
            keys[0],
            Event::Key(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE))
        );
        assert_eq!(
            keys[1],
            Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
        );
        assert_eq!(
            keys[2],
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT))
        );
        assert_eq!(
            keys[3],
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            keys[4],
            Event::Key(KeyEvent::new(KeyCode::Char('魔'), KeyModifiers::NONE))
        );
    }

    #[test]
    fn oversized_paste_fails_closed() {
        let mut d = Decoder::default();
        d.feed(b"\x1b[200~").unwrap();
        assert!(d.feed(&vec![b'x'; MAX_PASTE_BYTES + 1]).is_err());
        assert!(d.next_event().is_none());
    }
}
