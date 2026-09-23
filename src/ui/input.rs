use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
}

impl Editor {
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub fn insert(&mut self, text: &str) {
        let text: String = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        self.text.insert_str(self.cursor, &text);
        self.cursor += text.len();
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next(&self) -> usize {
        self.cursor
            + self.text[self.cursor..]
                .graphemes(true)
                .next()
                .map_or(0, str::len)
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&c.to_string())
            }
            KeyCode::Backspace => {
                let previous = self.previous();
                self.text.drain(previous..self.cursor);
                self.cursor = previous;
            }
            KeyCode::Delete => {
                self.text.drain(self.cursor..self.next());
            }
            KeyCode::Left => self.cursor = self.previous(),
            KeyCode::Right => self.cursor = self.next(),
            KeyCode::Home => {
                self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
            }
            KeyCode::End => {
                self.cursor += self.text[self.cursor..]
                    .find('\n')
                    .unwrap_or(self.text.len() - self.cursor)
            }
            KeyCode::Up | KeyCode::Down => self.move_line(key.code == KeyCode::Up),
            KeyCode::Enter => self.insert("\n"),
            KeyCode::Tab => self.insert("\t"),
            _ => return false,
        }
        true
    }

    fn move_line(&mut self, up: bool) {
        let start = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
        let col = self.text[start..self.cursor].width();
        let range = if up {
            if start == 0 {
                return;
            }
            let end = start - 1;
            (self.text[..end].rfind('\n').map_or(0, |i| i + 1), end)
        } else {
            let Some(end) = self.text[self.cursor..].find('\n') else {
                return;
            };
            let next = self.cursor + end + 1;
            (
                next,
                next + self.text[next..]
                    .find('\n')
                    .unwrap_or(self.text.len() - next),
            )
        };
        self.cursor = range.0;
        let mut width = 0;
        for glyph in self.text[range.0..range.1].graphemes(true) {
            width += glyph.width();
            if width > col {
                break;
            }
            self.cursor += glyph.len();
        }
    }

    pub fn display(&self, width: usize, height: usize, secret: bool) -> (Vec<String>, u16, u16) {
        let width = width.max(1);
        let mut rows = vec![String::new()];
        let mut col = 0;
        let mut cursor_pos = (0, 0);
        for (index, glyph) in self.text.grapheme_indices(true) {
            if index == self.cursor {
                cursor_pos = (col, rows.len() - 1);
            }
            if glyph == "\n" {
                rows.push(String::new());
                col = 0;
            } else {
                let visible = if secret {
                    "*"
                } else if glyph == "\t" {
                    " "
                } else {
                    glyph
                };
                let glyph_width = visible.width();
                if col + glyph_width >= width {
                    rows.push(String::new());
                    col = 0;
                    if index == self.cursor {
                        cursor_pos = (0, rows.len() - 1);
                    }
                }
                rows.last_mut().expect("row exists").push_str(visible);
                col += glyph_width;
            }
        }
        if self.cursor == self.text.len() {
            cursor_pos = (col, rows.len() - 1);
        }
        let start = cursor_pos.1.saturating_sub(height.saturating_sub(1));
        let rows = rows.into_iter().skip(start).take(height).collect();
        (rows, cursor_pos.0 as u16, (cursor_pos.1 - start) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_graphemes_not_bytes() {
        let mut e = Editor::default();
        e.insert("魔法e\u{301}");
        e.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(e.text, "魔法");
        e.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        e.insert("阵");
        assert_eq!(e.text, "魔阵法");
    }

    #[test]
    fn paste_preserves_lines_without_executing_controls() {
        let mut e = Editor::default();
        e.insert("a\r\n魔\u{1b}\u{7}");
        assert_eq!(e.text, "a\n魔");
        let (lines, x, y) = e.display(10, 2, true);
        assert_eq!(lines, ["*", "*"]);
        assert_eq!((x, y), (1, 1));
    }
}
